use chrono::Utc;
use tracing::info;
use uuid::Uuid;
use yoink_shared::Quality;

use crate::{
    db,
    error::{AppError, AppResult},
    services,
    state::AppState,
};

use super::helpers;

pub(super) async fn add_track(
    state: &AppState,
    provider: String,
    external_track_id: String,
    external_album_id: String,
    artist_external_id: String,
    artist_name: String,
) -> AppResult<()> {
    // 1. Find or create lightweight (unmonitored) artist.
    let artist_id = helpers::find_or_create_lightweight_artist(
        state,
        &provider,
        &artist_external_id,
        &artist_name,
    )
    .await?;

    // 2. Fetch album metadata to create the parent album.
    let prov = state.registry.metadata_provider(&provider).ok_or_else(|| {
        AppError::unavailable(
            "metadata provider",
            format!("unknown provider '{provider}'"),
        )
    })?;

    let albums = prov.fetch_albums(&artist_external_id).await?;

    let prov_album = albums
        .into_iter()
        .find(|a| a.external_id == external_album_id)
        .ok_or_else(|| {
            AppError::not_found(
                "provider album",
                Some(format!("{provider}:{external_album_id}")),
            )
        })?;

    // 3. Find or create the album.
    let existing_album_id =
        db::find_album_by_provider_link(&state.db, &provider, &external_album_id)
            .await
            .ok()
            .flatten();

    let album_id = if let Some(id) = existing_album_id {
        id
    } else {
        let new_id = Uuid::now_v7();
        let album = yoink_shared::MonitoredAlbum {
            id: new_id,
            artist_id,
            artist_ids: vec![artist_id],
            artist_credits: prov_album
                .artists
                .iter()
                .map(|a| yoink_shared::ArtistCredit {
                    name: a.name.clone(),
                    provider: Some(provider.clone()),
                    external_id: Some(a.external_id.clone()),
                })
                .collect(),
            title: prov_album.title.clone(),
            album_type: prov_album.album_type.clone(),
            release_date: prov_album.release_date.clone(),
            cover_url: prov_album
                .cover_ref
                .as_deref()
                .map(|c| yoink_shared::provider_image_url(&provider, c, 640)),
            explicit: prov_album.explicit,
            quality_override: None,
            monitored: false, // album-level not monitored; only the specific track
            acquired: false,
            wanted: false,
            partially_wanted: true, // will have a monitored track
            added_at: Utc::now(),
        };
        db::upsert_album(&state.db, &album).await?;

        let link = db::AlbumProviderLink {
            id: Uuid::now_v7(),
            album_id: new_id,
            provider: provider.clone(),
            external_id: external_album_id.clone(),
            external_url: prov_album.url.clone(),
            external_title: Some(prov_album.title.clone()),
            cover_ref: prov_album.cover_ref.clone(),
        };
        db::upsert_album_provider_link(&state.db, &link).await?;
        db::add_album_artist(&state.db, new_id, artist_id).await?;

        {
            let mut albums = state.monitored_albums.write().await;
            albums.push(album);
        }
        new_id
    };

    // 4. Fetch and store tracks (none monitored by default).
    helpers::store_album_tracks(state, &provider, &external_album_id, album_id, false).await?;

    // 5. Find the target track and mark it as monitored.
    if let Ok(Some(track_id)) =
        db::find_track_by_provider_link(&state.db, &provider, &external_track_id).await
    {
        db::update_track_flags(&state.db, track_id, true, false).await?;

        // Recompute partially_wanted
        {
            let mut albums = state.monitored_albums.write().await;
            if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
                services::recompute_partially_wanted(&state.db, album).await;
                if album.partially_wanted {
                    let album_clone = album.clone();
                    drop(albums);
                    services::enqueue_album_download(state, &album_clone).await;
                }
            }
        }
    }

    info!(%album_id, %provider, %external_track_id, "Added track from search");
    state.notify_sse();
    Ok(())
}

pub(super) async fn toggle_track_monitor(
    state: &AppState,
    track_id: Uuid,
    album_id: Uuid,
    monitored: bool,
) -> AppResult<()> {
    // Update the track's monitored flag in DB
    let current_acquired = {
        let tracks = db::load_tracks_for_album(&state.db, album_id)
            .await
            .unwrap_or_default();
        tracks
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| t.acquired)
            .unwrap_or(false)
    };
    db::update_track_flags(&state.db, track_id, monitored, current_acquired).await?;

    // Recompute the album's partially_wanted flag
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
            services::recompute_partially_wanted(&state.db, album).await;
            // If track became wanted, trigger album download (the download
            // worker will handle track-level filtering in Phase 3)
            if monitored && !current_acquired && (album.wanted || album.partially_wanted) {
                let album_clone = album.clone();
                drop(albums);
                services::enqueue_album_download(state, &album_clone).await;
            }
        }
    }
    info!(%track_id, %album_id, monitored, "Toggled track monitored status");
    state.notify_sse();
    Ok(())
}

pub(super) async fn set_track_quality(
    state: &AppState,
    album_id: Uuid,
    track_id: Uuid,
    quality: Option<Quality>,
) -> AppResult<()> {
    db::update_track_quality_override(&state.db, track_id, quality).await?;
    info!(%album_id, %track_id, quality = ?quality, "Updated track quality override");
    state.notify_sse();
    Ok(())
}

pub(super) async fn bulk_toggle_track_monitor(
    state: &AppState,
    album_id: Uuid,
    monitored: bool,
) -> AppResult<()> {
    // Update all tracks for the album
    let tracks = db::load_tracks_for_album(&state.db, album_id)
        .await
        .unwrap_or_default();

    for track in &tracks {
        db::update_track_flags(&state.db, track.id, monitored, track.acquired).await?;
    }

    // Recompute the album's partially_wanted flag and potentially enqueue
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
            services::recompute_partially_wanted(&state.db, album).await;
            if album.wanted || album.partially_wanted {
                let album_clone = album.clone();
                drop(albums);
                services::enqueue_album_download(state, &album_clone).await;
            }
        }
    }

    let count = tracks.len();
    info!(%album_id, monitored, count, "Bulk toggled track monitoring");
    state.notify_sse();
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::db;
    use crate::test_helpers::*;
    use yoink_shared::Quality;

    #[tokio::test]
    async fn toggle_track_monitor_updates_flags_and_partially_wanted() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Artist").await;
        let mut album = seed_album(&state.db, artist.id, "Album").await;
        album.monitored = false;
        album.wanted = false;
        db::upsert_album(&state.db, &album).await.unwrap();

        let tracks = seed_tracks(&state.db, album.id, 2).await;

        state.monitored_artists.write().await.push(artist.clone());
        state.monitored_albums.write().await.push(album.clone());

        super::toggle_track_monitor(&state, tracks[0].id, album.id, true)
            .await
            .unwrap();

        let db_tracks = db::load_tracks_for_album(&state.db, album.id)
            .await
            .unwrap();
        let t = db_tracks.iter().find(|t| t.id == tracks[0].id).unwrap();
        assert!(t.monitored);

        let albums = state.monitored_albums.read().await;
        let a = albums.iter().find(|a| a.id == album.id).unwrap();
        assert!(a.partially_wanted);
    }

    #[tokio::test]
    async fn set_track_quality_persists_without_enqueuing() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Artist").await;
        let album = seed_album(&state.db, artist.id, "Album").await;
        let tracks = seed_tracks(&state.db, album.id, 1).await;

        state.monitored_artists.write().await.push(artist);
        state.monitored_albums.write().await.push(album.clone());

        super::set_track_quality(&state, album.id, tracks[0].id, Some(Quality::Lossless))
            .await
            .unwrap();

        let loaded = db::load_tracks_for_album(&state.db, album.id)
            .await
            .unwrap();
        assert_eq!(loaded[0].quality_override, Some(Quality::Lossless));
        assert!(state.download_jobs.read().await.is_empty());
    }
}
