use std::collections::{HashMap, HashSet};

use tokio::fs;
use tracing::info;
use uuid::Uuid;

use crate::{db, services::downloads::sanitize_path_component, state::AppState};

use super::{
    album_dir_has_downloaded_audio, normalize_text, parse_release_year, recompute_partially_wanted,
    update_wanted,
};

pub(crate) async fn reconcile_library_files(state: &AppState) -> Result<usize, String> {
    let artists = state.monitored_artists.read().await.clone();
    let artist_names: HashMap<Uuid, String> = artists.into_iter().map(|a| (a.id, a.name)).collect();
    let albums_snapshot = state.monitored_albums.read().await.clone();

    // ── Track-level reconciliation ──────────────────────────────────
    // For every track marked as acquired, verify the file still exists.
    let mut track_changes = 0usize;
    for album in albums_snapshot.iter() {
        let tracks = db::load_tracks_for_album(&state.db, album.id)
            .await
            .unwrap_or_default();

        for track in tracks.iter().filter(|t| t.acquired) {
            let file_exists = if let Some(ref rel_path) = track.file_path {
                let abs = state.music_root.join(rel_path);
                fs::try_exists(&abs).await.unwrap_or(false)
            } else {
                false
            };

            if !file_exists {
                let _ = db::update_track_flags(&state.db, track.id, track.monitored, false).await;
                track_changes += 1;
            }
        }
    }

    if track_changes > 0 {
        info!(
            updated_tracks = track_changes,
            "Reconciled missing track files"
        );
    }

    // ── Album-level reconciliation ──────────────────────────────────
    let mut missing_ids = HashSet::new();
    for album in albums_snapshot.iter().filter(|a| a.acquired) {
        let Some(artist_name) = artist_names.get(&album.artist_id) else {
            continue;
        };
        let release_suffix = album
            .release_date
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        // Primary path: exact match using sanitized components (matches download worker)
        let album_dir = state
            .music_root
            .join(sanitize_path_component(artist_name))
            .join(sanitize_path_component(&format!(
                "{} ({})",
                album.title, release_suffix
            )));

        if album_dir_has_downloaded_audio(&album_dir).await {
            // Directory exists — but for partially-wanted albums, verify via
            // track-level flags (some tracks may have been removed).
            if !album.monitored {
                let all_ok = db::all_monitored_tracks_acquired(&state.db, album.id)
                    .await
                    .unwrap_or(true);
                if !all_ok {
                    missing_ids.insert(album.id);
                }
            }
            continue;
        }

        // Fallback: try year-only suffix (common for manually imported folders)
        let year_only = parse_release_year(&release_suffix);
        if let Some(ref year) = year_only {
            let year_dir = state
                .music_root
                .join(sanitize_path_component(artist_name))
                .join(sanitize_path_component(&format!(
                    "{} ({})",
                    album.title, year
                )));
            if album_dir_has_downloaded_audio(&year_dir).await {
                continue;
            }
        }

        // Fallback: scan the artist directory for a case-insensitive / fuzzy match
        if find_album_dir_fuzzy(
            state,
            artist_name,
            &album.title,
            album.release_date.as_deref(),
        )
        .await
        {
            continue;
        }

        missing_ids.insert(album.id);
    }

    if missing_ids.is_empty() && track_changes == 0 {
        return Ok(0);
    }

    let mut changed = 0usize;
    let mut albums = state.monitored_albums.write().await;
    for album in albums.iter_mut() {
        if missing_ids.contains(&album.id) && album.acquired {
            album.acquired = false;
            update_wanted(album);
            recompute_partially_wanted(&state.db, album).await;
            let _ = db::update_album_flags(
                &state.db,
                album.id,
                album.monitored,
                album.acquired,
                album.wanted,
            )
            .await;
            changed += 1;
        }
    }

    let total_changes = changed + track_changes;
    if total_changes > 0 {
        info!(
            updated_albums = changed,
            updated_tracks = track_changes,
            "Reconciled missing files in library"
        );
        state.notify_sse();
    }

    Ok(total_changes)
}

/// Scan the artist directory for a folder that matches the album title and
/// year using case-insensitive / normalized comparison. This handles manually
/// imported folders whose names may differ from the provider's metadata.
async fn find_album_dir_fuzzy(
    state: &AppState,
    artist_name: &str,
    album_title: &str,
    release_date: Option<&str>,
) -> bool {
    let target_title = normalize_text(album_title);
    let target_year = release_date.and_then(parse_release_year);

    // Try both the sanitized artist name and the raw name (for user-created folders)
    let sanitized_artist_dir = state.music_root.join(sanitize_path_component(artist_name));
    let raw_artist_dir = state.music_root.join(artist_name);

    for artist_dir in [&sanitized_artist_dir, &raw_artist_dir] {
        let mut entries = match fs::read_dir(artist_dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(folder_name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };

            let (folder_title, folder_year) = split_album_folder_name(folder_name);
            let folder_title_norm = normalize_text(&folder_title);

            if folder_title_norm != target_title {
                continue;
            }

            // Title matches; check year compatibility
            let year_ok = match (&target_year, &folder_year) {
                (Some(ty), Some(fy)) => ty == fy,
                _ => true,
            };

            if year_ok && album_dir_has_downloaded_audio(&path).await {
                return true;
            }
        }
    }

    false
}

fn split_album_folder_name(name: &str) -> (String, Option<String>) {
    if let Some((title, tail)) = name.rsplit_once(" (") {
        let inner = tail.trim_end_matches(')').trim();
        let year_str = inner.split('-').next().unwrap_or("");
        if year_str.len() == 4 && year_str.chars().all(|c| c.is_ascii_digit()) {
            return (title.trim().to_string(), Some(year_str.to_string()));
        }
    }
    (name.trim().to_string(), None)
}
