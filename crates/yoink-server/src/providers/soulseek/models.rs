//! Serde model types for slskd JSON API requests and responses.
//!
//! These structs mirror the JSON shapes used by the slskd REST API.
//! Higher-level logic lives in [`super`].

use serde::{Deserialize, Serialize};

// ── Authentication ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponse {
    pub token: String,
}

// ── Search ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchRequest {
    pub id: Option<String>,
    pub search_text: String,
    pub search_timeout: Option<u32>,
    pub response_limit: Option<u32>,
    pub file_limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Search {
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchStatus {
    #[serde(default)]
    pub is_complete: bool,
    #[serde(default)]
    pub response_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResponse {
    pub username: String,
    pub files: Vec<SearchFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchFile {
    pub filename: String,
    pub size: i64,
    #[serde(default)]
    pub length: Option<u32>,
    #[serde(default)]
    pub bit_rate: Option<u32>,
    #[serde(default)]
    pub extension: Option<String>,
}

// ── Downloads / Transfers ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct QueueDownloadRequest {
    pub filename: String,
    pub size: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferUserResponse {
    pub directories: Vec<TransferDirectory>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferDirectory {
    #[serde(default)]
    pub directory: Option<String>,
    pub files: Vec<Transfer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Transfer {
    pub filename: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub state_description: Option<String>,
    #[serde(default)]
    pub exception: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub bytes_remaining: Option<i64>,
    #[serde(default)]
    pub bytes_transferred: Option<i64>,
}
