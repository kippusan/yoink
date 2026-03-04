use uuid::Uuid;

use crate::{
    db,
    error::{AppError, AppResult},
    state::AppState,
};

use super::update_wanted;

pub(crate) async fn merge_albums(
    state: &AppState,
    target_album_id: Uuid,
    source_album_id: Uuid,
    result_title: Option<&str>,
    result_cover_url: Option<&str>,
) -> AppResult<()> {
    if target_album_id == source_album_id {
        return Err(AppError::validation(
            Some("source_album_id"),
            "target and source albums must be different",
        ));
    }

    let (target_artist_ids, source_artist_ids, source_flags) = {
        let albums = state.monitored_albums.read().await;
        let Some(target) = albums.iter().find(|a| a.id == target_album_id) else {
            return Err(AppError::not_found(
                "target album",
                Some(target_album_id.to_string()),
            ));
        };
        let Some(source) = albums.iter().find(|a| a.id == source_album_id) else {
            return Err(AppError::not_found(
                "source album",
                Some(source_album_id.to_string()),
            ));
        };
        (
            target.artist_ids.clone(),
            source.artist_ids.clone(),
            (source.monitored, source.acquired, source.wanted),
        )
    };

    // Check that the albums share at least one artist.
    let share_artist = target_artist_ids
        .iter()
        .any(|id| source_artist_ids.contains(id));
    if !share_artist {
        return Err(AppError::conflict(
            "can only merge albums that share at least one artist",
        ));
    }

    let source_links = db::load_album_provider_links(&state.db, source_album_id)
        .await?;

    for link in source_links {
        let moved = db::AlbumProviderLink {
            id: Uuid::now_v7(),
            album_id: target_album_id,
            provider: link.provider,
            external_id: link.external_id,
            external_url: link.external_url,
            external_title: link.external_title,
            cover_ref: link.cover_ref,
        };
        db::upsert_album_provider_link(&state.db, &moved).await?;
    }

    db::reassign_tracks_to_album(&state.db, source_album_id, target_album_id)
        .await?;
    db::reassign_jobs_to_album(&state.db, source_album_id, target_album_id)
        .await?;

    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(target) = albums.iter_mut().find(|a| a.id == target_album_id) {
            target.monitored = target.monitored || source_flags.0;
            target.acquired = target.acquired || source_flags.1;
            update_wanted(target);

            if let Some(title) = result_title {
                target.title = title.to_string();
            }
            if let Some(cover) = result_cover_url {
                target.cover_url = Some(cover.to_string());
            }

            db::upsert_album(&state.db, target).await?;
        }
        albums.retain(|a| a.id != source_album_id);
    }

    db::delete_album(&state.db, source_album_id)
        .await?;

    Ok(())
}
