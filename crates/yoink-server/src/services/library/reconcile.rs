use std::collections::{HashMap, HashSet};

use tracing::info;

use crate::{db, services::downloads::sanitize_path_component, state::AppState};

use super::{album_dir_has_downloaded_audio, update_wanted};

pub(crate) async fn reconcile_library_files(state: &AppState) -> Result<usize, String> {
    let artists = state.monitored_artists.read().await.clone();
    let artist_names: HashMap<String, String> = artists
        .into_iter()
        .map(|a| (a.id.clone(), a.name))
        .collect();
    let albums_snapshot = state.monitored_albums.read().await.clone();

    let mut missing_ids = HashSet::new();
    for album in albums_snapshot.iter().filter(|a| a.acquired) {
        let Some(artist_name) = artist_names.get(&album.artist_id) else {
            continue;
        };
        let release_suffix = album
            .release_date
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        let album_dir = state
            .music_root
            .join(sanitize_path_component(artist_name))
            .join(sanitize_path_component(&format!(
                "{} ({})",
                album.title, release_suffix
            )));

        if !album_dir_has_downloaded_audio(&album_dir).await {
            missing_ids.insert(album.id.clone());
        }
    }

    if missing_ids.is_empty() {
        return Ok(0);
    }

    let mut changed = 0usize;
    let mut albums = state.monitored_albums.write().await;
    for album in albums.iter_mut() {
        if missing_ids.contains(&album.id) && album.acquired {
            album.acquired = false;
            update_wanted(album);
            let _ = db::update_album_flags(
                &state.db,
                &album.id,
                album.monitored,
                album.acquired,
                album.wanted,
            )
            .await;
            changed += 1;
        }
    }

    if changed > 0 {
        info!(
            updated_albums = changed,
            "Reconciled missing files in library"
        );
        state.notify_sse();
    }

    Ok(changed)
}
