use std::collections::HashMap;

use uuid::Uuid;

use crate::{DownloadJob, DownloadStatus, MonitoredAlbum, MonitoredArtist};

// ── Data helpers (pure transforms) ──────────────────────────

/// Group albums by artist_id, sorted newest-first within each group.
/// Albums with multiple artists appear under each artist's group.
pub fn build_albums_by_artist(albums: Vec<MonitoredAlbum>) -> HashMap<Uuid, Vec<MonitoredAlbum>> {
    let mut map: HashMap<Uuid, Vec<MonitoredAlbum>> = HashMap::new();
    for album in albums {
        if album.artist_ids.is_empty() {
            // Fallback: use the primary artist_id
            map.entry(album.artist_id).or_default().push(album);
        } else {
            for &aid in &album.artist_ids {
                map.entry(aid).or_default().push(album.clone());
            }
        }
    }
    for albums in map.values_mut() {
        albums.sort_by(|a, b| {
            b.release_date
                .cmp(&a.release_date)
                .then_with(|| a.title.cmp(&b.title))
        });
    }
    map
}

/// For each album_id, keep only the most recently updated job.
pub fn build_latest_jobs(jobs: Vec<DownloadJob>) -> HashMap<Uuid, DownloadJob> {
    let mut map: HashMap<Uuid, DownloadJob> = HashMap::new();
    for job in jobs {
        map.entry(job.album_id)
            .and_modify(|existing| {
                if job.updated_at > existing.updated_at {
                    *existing = job.clone();
                }
            })
            .or_insert(job);
    }
    map
}

/// Map artist id -> name for display.
pub fn build_artist_names(artists: &[MonitoredArtist]) -> HashMap<Uuid, String> {
    artists.iter().map(|a| (a.id, a.name.clone())).collect()
}

// ── Display helpers ─────────────────────────────────────────

pub fn status_label_text(status: &DownloadStatus, completed: usize, total: usize) -> String {
    match status {
        DownloadStatus::Queued => "Queued".to_string(),
        DownloadStatus::Resolving => "Resolving".to_string(),
        DownloadStatus::Downloading => {
            if total > 0 {
                format!("Downloading {completed}/{total}")
            } else {
                "Downloading".to_string()
            }
        }
        DownloadStatus::Completed => "Completed".to_string(),
        DownloadStatus::Failed => "Failed".to_string(),
    }
}

pub fn status_class(status: &DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Queued => "pill status-queued",
        DownloadStatus::Resolving => "pill status-resolving",
        DownloadStatus::Downloading => "pill status-downloading",
        DownloadStatus::Completed => "pill status-completed",
        DownloadStatus::Failed => "pill status-failed",
    }
}

// ── Asset/URL helpers ───────────────────────────────────────

/// Build an image proxy URL for a given provider and image reference.
pub fn provider_image_url(provider: &str, image_ref: &str, size: u16) -> String {
    format!("/api/image/{provider}/{image_ref}/{size}")
}

/// Get the cover URL for an album (already a full URL or None).
pub fn album_cover_url(album: &MonitoredAlbum, _size: u16) -> Option<String> {
    album.cover_url.clone()
}

pub fn album_type_label(album_type: Option<&str>, title: &str) -> &'static str {
    if let Some(kind) = album_type {
        let k = kind.to_ascii_lowercase();
        if k.contains("ep") {
            return "EP";
        }
        if k.contains("single") {
            return "Single";
        }
        if k.contains("album") {
            return "Album";
        }
    }
    let t = title.to_ascii_lowercase();
    if t.contains(" ep") || t.ends_with("ep") || t.contains("(ep") {
        return "EP";
    }
    if t.contains(" single") || t.ends_with("single") || t.contains("(single") {
        return "Single";
    }
    "Album"
}

pub fn album_type_rank(album_type: Option<&str>, title: &str) -> u8 {
    match album_type_label(album_type, title) {
        "Album" => 0,
        "EP" => 1,
        "Single" => 2,
        _ => 3,
    }
}

/// Human-readable display name for a provider ID.
pub fn provider_display_name(provider: &str) -> String {
    match provider {
        "tidal" => "Tidal".to_string(),
        "musicbrainz" => "MusicBrainz".to_string(),
        "deezer" => "Deezer".to_string(),
        "soulseek" => "SoulSeek".to_string(),
        other => {
            let mut c = other.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        }
    }
}
