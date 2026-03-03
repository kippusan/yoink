mod import;
mod merge;
mod reconcile;
mod sync;

use crate::models::MonitoredAlbum;

pub(crate) use import::{confirm_import_library, preview_import_library, scan_and_import_library};
pub(crate) use merge::merge_albums;
pub(crate) use reconcile::reconcile_library_files;
pub(crate) use sync::sync_artist_albums;

/// Recompute the derived `wanted` flag for an album.
///
/// An album is fully wanted when it is album-level monitored and not yet acquired.
/// `partially_wanted` is not updated here — it depends on track-level state and
/// is computed via a DB subquery in `load_albums` or explicitly via
/// `recompute_partially_wanted`.
pub(crate) fn update_wanted(album: &mut MonitoredAlbum) {
    album.wanted = album.monitored && !album.acquired;
}

/// Recompute the `partially_wanted` flag for an album by checking its tracks.
/// Call this after toggling individual track monitoring.
pub(crate) async fn recompute_partially_wanted(
    db: &sqlx::SqlitePool,
    album: &mut MonitoredAlbum,
) {
    if album.monitored {
        // Fully monitored albums are never "partially" wanted
        album.partially_wanted = false;
    } else {
        album.partially_wanted = crate::db::has_wanted_tracks(db, album.id)
            .await
            .unwrap_or(false);
    }
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_release_year(release_date: &str) -> Option<String> {
    let year = release_date.chars().take(4).collect::<String>();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        Some(year)
    } else {
        None
    }
}

async fn album_dir_has_downloaded_audio(path: &std::path::Path) -> bool {
    use tokio::fs;

    if !fs::try_exists(path).await.unwrap_or(false) {
        return false;
    }

    let mut entries = match fs::read_dir(path).await {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let p = entry.path();
        if p.extension()
            .and_then(|e| e.to_str())
            .map(|ext| {
                ext.eq_ignore_ascii_case("flac")
                    || ext.eq_ignore_ascii_case("m4a")
                    || ext.eq_ignore_ascii_case("mp4")
            })
            .unwrap_or(false)
        {
            return true;
        }
    }

    false
}
