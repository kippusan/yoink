//! Serves the statically built frontend SPA from files embedded at compile
//! time via [`rust_embed`].
//!
//! In **release builds** the files are baked into the binary.  In **debug
//! builds** they are read from disk, so a rebuilt Vite bundle in
//! `frontend/` is picked up without recompilation.
//!
//! The handler is intended to be used as the Axum router **fallback** so that
//! any request that does not match an API route is either served as a static
//! asset or falls back to the SPA shell (`index.html`).

use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

// ── Embedded frontend assets ────────────────────────────────────────

/// The build output of the frontend SPA (`frontend/dist/`).
///
/// `build.rs` verifies that the SPA shell exists before compilation so
/// release builds fail fast if the frontend has not been built.
#[derive(RustEmbed)]
#[folder = "../../frontend/dist/"]
struct FrontendAssets;

/// Name of the SPA shell entry point produced by Vite.
const SPA_SHELL: &str = "index.html";

// ── Axum fallback handler ───────────────────────────────────────────

/// Fallback handler that serves embedded frontend assets or the SPA shell.
///
/// Resolution order:
/// 1. Exact file match (e.g. `/assets/main-xxx.js` → embedded file).
/// 2. `index.html` inside a directory path (not used by this frontend but
///    kept for completeness).
/// 3. SPA fallback → serve `index.html` so TanStack Router can handle
///    client-side routing.
/// 4. If the SPA shell itself is missing, return a hard server error because
///    the build should have failed before shipping this binary.
pub(crate) async fn serve_frontend(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // 1. Try to serve the exact file.
    if let Some(file) = FrontendAssets::get(path) {
        return serve_file(path, &file);
    }

    // 2. Try path/index.html (directory index).
    if !path.is_empty() {
        let index_path = format!("{path}/index.html");
        if let Some(file) = FrontendAssets::get(&index_path) {
            return serve_file(&index_path, &file);
        }
    }

    // 3. SPA fallback — serve the shell for any unmatched path so the
    //    client-side router can take over.
    if let Some(shell) = FrontendAssets::get(SPA_SHELL) {
        return serve_file(SPA_SHELL, &shell);
    }

    // 4. This should be unreachable because `build.rs` requires the SPA
    //    shell to exist before the crate can compile.
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "embedded frontend assets are missing from the server binary",
    )
        .into_response()
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Build an HTTP response for an embedded file with appropriate headers.
fn serve_file(path: &str, file: &rust_embed::EmbeddedFile) -> Response {
    let content_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    let cache_control = if path.starts_with("assets/") {
        // Vite hashes asset filenames — they are safe to cache forever.
        "public, max-age=31536000, immutable"
    } else {
        // The SPA shell and other root files should always be revalidated.
        "no-cache"
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache_control.to_string()),
        ],
        file.data.to_vec(),
    )
        .into_response()
}
