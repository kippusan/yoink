use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ImportConfirmation;

/// A user-initiated action that the server can execute.
///
/// Serialized by the WASM client, sent to the `dispatch_action` server function,
/// and executed on the server where `AppState` is available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerAction {
    ToggleAlbumMonitor {
        album_id: Uuid,
        monitored: bool,
    },
    BulkMonitor {
        artist_id: Uuid,
        monitored: bool,
    },
    SyncArtistAlbums {
        artist_id: Uuid,
    },
    RemoveArtist {
        artist_id: Uuid,
        remove_files: bool,
    },
    AddArtist {
        name: String,
        provider: String,
        external_id: String,
        image_url: Option<String>,
        external_url: Option<String>,
    },
    LinkArtistProvider {
        artist_id: Uuid,
        provider: String,
        external_id: String,
        external_url: Option<String>,
        external_name: Option<String>,
        image_ref: Option<String>,
    },
    UnlinkArtistProvider {
        artist_id: Uuid,
        provider: String,
        external_id: String,
    },
    CancelDownload {
        job_id: Uuid,
    },
    ClearCompleted,
    RetryDownload {
        album_id: Uuid,
    },
    RemoveAlbumFiles {
        album_id: Uuid,
        unmonitor: bool,
    },
    AcceptMatchSuggestion {
        suggestion_id: Uuid,
    },
    DismissMatchSuggestion {
        suggestion_id: Uuid,
    },
    RefreshMatchSuggestions {
        artist_id: Uuid,
    },
    MergeAlbums {
        target_album_id: Uuid,
        source_album_id: Uuid,
        /// If provided, override the surviving album's title.
        result_title: Option<String>,
        /// If provided, override the surviving album's cover URL.
        result_cover_url: Option<String>,
    },
    AddAlbumArtist {
        album_id: Uuid,
        artist_id: Uuid,
    },
    RemoveAlbumArtist {
        album_id: Uuid,
        artist_id: Uuid,
    },
    RetagLibrary,
    ScanImportLibrary,
    ConfirmImport {
        items: Vec<ImportConfirmation>,
    },
}
