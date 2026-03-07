use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tokio::fs;
use tracing::info;
use uuid::Uuid;

use yoink_shared::{
    ImportAlbumCandidate, ImportConfirmation, ImportMatchStatus, ImportPreviewItem,
    ImportResultSummary,
};

use crate::{
    db,
    error::{AppError, AppResult},
    models::MonitoredArtist,
    state::AppState,
};

use super::{
    album_dir_has_downloaded_audio, normalize_text, parse_release_year, sync::sync_artist_albums,
    update_wanted,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScanImportSummary {
    pub(crate) discovered_albums: usize,
    pub(crate) imported_albums: usize,
    pub(crate) artists_added: usize,
    pub(crate) unmatched_albums: usize,
}

pub(crate) async fn scan_and_import_library(state: &AppState) -> AppResult<ScanImportSummary> {
    let discovered = discover_local_albums(state).await?;
    if discovered.is_empty() {
        return Ok(ScanImportSummary {
            discovered_albums: 0,
            imported_albums: 0,
            artists_added: 0,
            unmatched_albums: 0,
        });
    }

    let mut imported_albums = 0usize;
    let mut artists_added = 0usize;
    let mut unmatched_albums = 0usize;
    let mut synced_artists = HashSet::new();

    for local in &discovered {
        let (artist_id, added_artist) = match ensure_monitored_artist(state, &local.artist_name)
            .await
        {
            Ok(Some(value)) => value,
            Ok(None) => {
                unmatched_albums += 1;
                continue;
            }
            Err(err) => {
                info!(artist = %local.artist_name, error = %err, "Failed to resolve artist during import");
                unmatched_albums += 1;
                continue;
            }
        };

        if added_artist {
            artists_added += 1;
        }

        if !synced_artists.contains(&artist_id)
            && sync_artist_albums(state, artist_id).await.is_ok()
        {
            synced_artists.insert(artist_id);
        }

        if import_local_album(state, artist_id, &local.album_title, local.year.as_deref()).await? {
            imported_albums += 1;
        } else {
            unmatched_albums += 1;
        }
    }

    if imported_albums > 0 {
        state.notify_sse();
    }

    Ok(ScanImportSummary {
        discovered_albums: discovered.len(),
        imported_albums,
        artists_added,
        unmatched_albums,
    })
}

/// Preview what a library scan would import, returning match candidates for
/// each discovered local album folder.
pub(crate) async fn preview_import_library(state: &AppState) -> AppResult<Vec<ImportPreviewItem>> {
    let discovered = discover_local_albums(state).await?;
    if discovered.is_empty() {
        return Ok(Vec::new());
    }

    let artists = state.monitored_artists.read().await.clone();
    let albums = state.monitored_albums.read().await.clone();
    let artist_names_lower: HashMap<String, (Uuid, String)> = artists
        .iter()
        .map(|a| (normalize_text(&a.name), (a.id, a.name.clone())))
        .collect();

    let mut items = Vec::new();
    for local in &discovered {
        let item_id = format!(
            "{:x}",
            md5_hash(&format!("{}/{}", local.artist_name, local.album_title))
        );
        let relative_path = if let Some(year) = &local.year {
            format!("{}/{} ({})", local.artist_name, local.album_title, year)
        } else {
            format!("{}/{}", local.artist_name, local.album_title)
        };

        let album_dir = build_local_album_path(
            state,
            &local.artist_name,
            &local.album_title,
            local.year.as_deref(),
        );
        let audio_count = count_audio_files(&album_dir).await;

        let needle = normalize_text(&local.artist_name);
        let mut candidates = Vec::new();
        let mut match_status = ImportMatchStatus::Unmatched;

        if let Some((artist_id, artist_name)) = artist_names_lower.get(&needle) {
            let target_title = normalize_text(&local.album_title);

            for album in albums
                .iter()
                .filter(|a| a.artist_id == *artist_id || a.artist_ids.contains(artist_id))
            {
                let album_title_norm = normalize_text(&album.title);
                let album_year = album.release_date.as_deref().and_then(parse_release_year);

                let title_match = album_title_norm == target_title;
                let year_match = match (&local.year, &album_year) {
                    (Some(ly), Some(ay)) => ly == ay,
                    (None, _) | (_, None) => true,
                };

                if title_match && year_match {
                    let confidence = if local.year.is_some() && album_year.is_some() {
                        100
                    } else {
                        85
                    };
                    candidates.push(ImportAlbumCandidate {
                        album_id: Some(album.id),
                        artist_id: *artist_id,
                        artist_name: artist_name.clone(),
                        album_title: album.title.clone(),
                        release_date: album.release_date.clone(),
                        cover_url: album.cover_url.clone(),
                        album_type: album.album_type.clone(),
                        explicit: album.explicit,
                        monitored: album.monitored,
                        acquired: album.acquired,
                        confidence,
                    });
                } else if title_match {
                    candidates.push(ImportAlbumCandidate {
                        album_id: Some(album.id),
                        artist_id: *artist_id,
                        artist_name: artist_name.clone(),
                        album_title: album.title.clone(),
                        release_date: album.release_date.clone(),
                        cover_url: album.cover_url.clone(),
                        album_type: album.album_type.clone(),
                        explicit: album.explicit,
                        monitored: album.monitored,
                        acquired: album.acquired,
                        confidence: 70,
                    });
                } else {
                    let sim = strsim::jaro_winkler(&target_title, &album_title_norm);
                    if sim > 0.85 {
                        let confidence = (sim * 80.0) as u8;
                        candidates.push(ImportAlbumCandidate {
                            album_id: Some(album.id),
                            artist_id: *artist_id,
                            artist_name: artist_name.clone(),
                            album_title: album.title.clone(),
                            release_date: album.release_date.clone(),
                            cover_url: album.cover_url.clone(),
                            album_type: album.album_type.clone(),
                            explicit: album.explicit,
                            monitored: album.monitored,
                            acquired: album.acquired,
                            confidence,
                        });
                    }
                }
            }

            if candidates.is_empty() {
                candidates.push(ImportAlbumCandidate {
                    album_id: None,
                    artist_id: *artist_id,
                    artist_name: artist_name.clone(),
                    album_title: local.album_title.clone(),
                    release_date: local.year.clone(),
                    cover_url: None,
                    album_type: None,
                    explicit: false,
                    monitored: false,
                    acquired: false,
                    confidence: 50,
                });
                match_status = ImportMatchStatus::Partial;
            }
        }

        candidates.sort_by(|a, b| b.confidence.cmp(&a.confidence));

        if !candidates.is_empty() && match_status != ImportMatchStatus::Partial {
            if candidates[0].confidence >= 85 {
                match_status = ImportMatchStatus::Matched;
            } else {
                match_status = ImportMatchStatus::Partial;
            }
        }

        // A folder is "already imported" when its best match is already acquired.
        // One folder = one album on disk. If the top candidate is acquired, it's done.
        let already_imported = candidates.first().map(|c| c.acquired).unwrap_or(false);

        let selected_candidate = if !candidates.is_empty() && !already_imported {
            Some(0)
        } else {
            None
        };

        items.push(ImportPreviewItem {
            id: item_id,
            relative_path,
            discovered_artist: local.artist_name.clone(),
            discovered_album: local.album_title.clone(),
            discovered_year: local.year.clone(),
            match_status,
            candidates,
            selected_candidate,
            already_imported,
            audio_file_count: audio_count,
        });
    }

    // Sort: unmatched first, then partial, then matched, then already imported
    items.sort_by(|a, b| {
        let rank = |item: &ImportPreviewItem| -> u8 {
            if item.already_imported {
                return 3;
            }
            match item.match_status {
                ImportMatchStatus::Unmatched => 0,
                ImportMatchStatus::Partial => 1,
                ImportMatchStatus::Matched => 2,
            }
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.discovered_artist.cmp(&b.discovered_artist))
            .then_with(|| a.discovered_album.cmp(&b.discovered_album))
    });

    Ok(items)
}

/// Execute a confirmed import: for each user-confirmed item, ensure the artist
/// exists, sync their albums, and mark the matched album as acquired.
pub(crate) async fn confirm_import_library(
    state: &AppState,
    items: Vec<ImportConfirmation>,
) -> AppResult<ImportResultSummary> {
    let mut imported = 0usize;
    let mut artists_added = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();
    let mut synced_artists = HashSet::new();

    for item in &items {
        let (artist_id, added_artist) = if let Some(aid) = item.artist_id {
            (aid, false)
        } else {
            match ensure_monitored_artist(state, &item.artist_name).await {
                Ok(Some((id, added))) => (id, added),
                Ok(None) => {
                    errors.push(format!(
                        "Could not find artist '{}' for album '{}'",
                        item.artist_name, item.album_title
                    ));
                    failed += 1;
                    continue;
                }
                Err(err) => {
                    errors.push(format!(
                        "Error resolving artist '{}': {}",
                        item.artist_name, err
                    ));
                    failed += 1;
                    continue;
                }
            }
        };

        if added_artist {
            artists_added += 1;
        }

        if !synced_artists.contains(&artist_id)
            && sync_artist_albums(state, artist_id).await.is_ok()
        {
            synced_artists.insert(artist_id);
        }

        if let Some(album_id) = item.album_id {
            let mut albums = state.monitored_albums.write().await;
            if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
                if !album.monitored {
                    album.monitored = true;
                }
                if !album.acquired {
                    album.acquired = true;
                }
                update_wanted(album);
                db::update_album_flags(
                    &state.db,
                    album.id,
                    album.monitored,
                    album.acquired,
                    album.wanted,
                )
                .await?;
                imported += 1;
            } else {
                errors.push(format!(
                    "Album ID '{}' not found for '{}'",
                    album_id, item.album_title
                ));
                failed += 1;
            }
        } else if import_local_album(state, artist_id, &item.album_title, item.year.as_deref())
            .await?
        {
            imported += 1;
        } else {
            errors.push(format!(
                "Could not match album '{}' by '{}' to any known album",
                item.album_title, item.artist_name
            ));
            failed += 1;
        }
    }

    // Always notify SSE so the import page refreshes after confirm,
    // even when all selected albums were already acquired.
    state.notify_sse();

    Ok(ImportResultSummary {
        total_selected: items.len(),
        imported,
        artists_added,
        failed,
        errors,
    })
}

// ── Private helpers ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct LocalAlbumDir {
    artist_name: String,
    album_title: String,
    year: Option<String>,
}

async fn discover_local_albums(state: &AppState) -> AppResult<Vec<LocalAlbumDir>> {
    if !fs::try_exists(&state.music_root).await.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let mut artist_dirs = fs::read_dir(&state.music_root).await.map_err(|err| {
        AppError::filesystem(
            "read music root",
            state.music_root.display().to_string(),
            err,
        )
    })?;

    while let Some(artist_entry) = artist_dirs.next_entry().await? {
        let artist_path = artist_entry.path();
        if !artist_path.is_dir() {
            continue;
        }
        let Some(artist_name) = artist_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };

        let mut album_dirs = match fs::read_dir(&artist_path).await {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        while let Some(album_entry) = album_dirs.next_entry().await.unwrap_or(None) {
            let album_path = album_entry.path();
            if !album_path.is_dir() || !album_dir_has_downloaded_audio(&album_path).await {
                continue;
            }
            let Some(album_folder) = album_path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let (album_title, year) = split_album_folder_name(album_folder);
            out.push(LocalAlbumDir {
                artist_name: artist_name.to_string(),
                album_title,
                year,
            });
        }
    }

    Ok(out)
}

/// Ensure an artist is monitored. Searches all metadata providers to find a match.
async fn ensure_monitored_artist(
    state: &AppState,
    artist_name: &str,
) -> AppResult<Option<(Uuid, bool)>> {
    let needle = normalize_text(artist_name);
    {
        let artists = state.monitored_artists.read().await;
        if let Some(artist) = artists
            .iter()
            .find(|a| normalize_text(&a.name) == needle)
            .cloned()
        {
            return Ok(Some((artist.id, false)));
        }
    }

    let all_results = state.registry.search_artists_all(artist_name).await;

    let mut best_match: Option<(String, crate::providers::ProviderArtist)> = None;
    for (provider_id, candidates) in &all_results {
        if let Some(exact) = candidates
            .iter()
            .find(|a| normalize_text(&a.name) == needle)
            .cloned()
        {
            best_match = Some((provider_id.clone(), exact));
            break;
        }
        if best_match.is_none()
            && let Some(first) = candidates.first().cloned()
        {
            best_match = Some((provider_id.clone(), first));
        }
    }

    let Some((provider_id, artist)) = best_match else {
        return Ok(None);
    };

    let new_id = Uuid::now_v7();
    let image_url = artist
        .image_ref
        .as_deref()
        .map(|r| yoink_shared::provider_image_url(&provider_id, r, 640));

    let monitored = MonitoredArtist {
        id: new_id,
        name: artist.name.clone(),
        image_url,
        bio: None,
        monitored: true, // Imported artists are fully monitored
        added_at: Utc::now(),
    };
    db::upsert_artist(&state.db, &monitored).await?;

    let link = db::ArtistProviderLink {
        id: Uuid::now_v7(),
        artist_id: new_id,
        provider: provider_id,
        external_id: artist.external_id,
        external_url: artist.url,
        external_name: Some(artist.name),
        image_ref: artist.image_ref,
    };
    db::upsert_artist_provider_link(&state.db, &link).await?;

    {
        let mut artists = state.monitored_artists.write().await;
        if artists.iter().all(|a| a.id != monitored.id) {
            artists.push(monitored.clone());
        }
    }

    Ok(Some((new_id, true)))
}

async fn import_local_album(
    state: &AppState,
    artist_id: Uuid,
    album_title: &str,
    year_hint: Option<&str>,
) -> AppResult<bool> {
    let target_title = normalize_text(album_title);
    let mut albums = state.monitored_albums.write().await;

    let hint_year = year_hint.and_then(parse_release_year);

    let mut matched_index = None;
    for (idx, album) in albums.iter().enumerate() {
        if album.artist_id != artist_id && !album.artist_ids.contains(&artist_id) {
            continue;
        }
        if normalize_text(&album.title) != target_title {
            continue;
        }
        if let Some(ref year) = hint_year {
            let album_year = album.release_date.as_deref().and_then(parse_release_year);
            if album_year.as_ref() != Some(year) {
                continue;
            }
        }
        matched_index = Some(idx);
        break;
    }

    let Some(idx) = matched_index else {
        return Ok(false);
    };

    let album = &mut albums[idx];
    let mut changed = false;
    if !album.monitored {
        album.monitored = true;
        changed = true;
    }
    if !album.acquired {
        album.acquired = true;
        changed = true;
    }
    update_wanted(album);
    if changed {
        db::update_album_flags(
            &state.db,
            album.id,
            album.monitored,
            album.acquired,
            album.wanted,
        )
        .await?;
    }
    Ok(changed)
}

fn build_local_album_path(
    state: &AppState,
    artist_name: &str,
    album_title: &str,
    year: Option<&str>,
) -> std::path::PathBuf {
    let folder = if let Some(y) = year {
        format!("{} ({})", album_title, y)
    } else {
        album_title.to_string()
    };
    state.music_root.join(artist_name).join(folder)
}

async fn count_audio_files(path: &std::path::Path) -> usize {
    let mut count = 0;
    if let Ok(mut entries) = fs::read_dir(path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| {
                    ext.eq_ignore_ascii_case("flac")
                        || ext.eq_ignore_ascii_case("m4a")
                        || ext.eq_ignore_ascii_case("mp4")
                        || ext.eq_ignore_ascii_case("mp3")
                        || ext.eq_ignore_ascii_case("aac")
                        || ext.eq_ignore_ascii_case("ogg")
                        || ext.eq_ignore_ascii_case("opus")
                        || ext.eq_ignore_ascii_case("wav")
                })
                .unwrap_or(false)
            {
                count += 1;
            }
        }
    }
    count
}

/// Simple string hash for generating stable IDs from paths.
fn md5_hash(input: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
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
