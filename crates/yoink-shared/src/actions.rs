use serde::{Deserialize, Serialize};

use crate::ImportConfirmation;

/// A user-initiated action that the server can execute.
///
/// Serialized by the WASM client, sent to the `dispatch_action` server function,
/// and executed on the server where `AppState` is available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerAction {
    ToggleAlbumMonitor {
        album_id: String,
        monitored: bool,
    },
    BulkMonitor {
        artist_id: String,
        monitored: bool,
    },
    SyncArtistAlbums {
        artist_id: String,
    },
    RemoveArtist {
        artist_id: String,
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
        artist_id: String,
        provider: String,
        external_id: String,
        external_url: Option<String>,
        external_name: Option<String>,
        image_ref: Option<String>,
    },
    UnlinkArtistProvider {
        artist_id: String,
        provider: String,
        external_id: String,
    },
    CancelDownload {
        job_id: String,
    },
    ClearCompleted,
    RetryDownload {
        album_id: String,
    },
    RemoveAlbumFiles {
        album_id: String,
        unmonitor: bool,
    },
    AcceptMatchSuggestion {
        suggestion_id: String,
    },
    DismissMatchSuggestion {
        suggestion_id: String,
    },
    RefreshMatchSuggestions {
        artist_id: String,
    },
    MergeAlbums {
        target_album_id: String,
        source_album_id: String,
        /// If provided, override the surviving album's title.
        result_title: Option<String>,
        /// If provided, override the surviving album's cover URL.
        result_cover_url: Option<String>,
    },
    RetagLibrary,
    ScanImportLibrary,
    ConfirmImport {
        items: Vec<ImportConfirmation>,
    },
}
