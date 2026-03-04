use tracing::info;

use crate::{
    error::{AppError, AppResult},
    services,
    state::AppState,
};

pub(super) async fn retag_library(state: &AppState) -> AppResult<()> {
    let s = state.clone();
    tokio::spawn(async move {
        match services::retag_existing_files(&s).await {
            Ok((tagged, missing, albums)) => {
                info!(
                    tagged_files = tagged,
                    missing_files = missing,
                    scanned_albums = albums,
                    "Completed manual library retag"
                );
            }
            Err(err) => {
                info!(error = %err, "Library retag failed");
            }
        }
    });
    Ok(())
}

pub(super) async fn scan_import_library(state: &AppState) -> AppResult<()> {
    let s = state.clone();
    tokio::spawn(async move {
        match services::scan_and_import_library(&s).await {
            Ok(summary) => {
                info!(
                    discovered = summary.discovered_albums,
                    imported = summary.imported_albums,
                    artists_added = summary.artists_added,
                    unmatched = summary.unmatched_albums,
                    "Completed scan/import pass"
                );
            }
            Err(err) => {
                info!(error = %err, "Scan/import failed");
            }
        }
    });
    Ok(())
}

pub(super) async fn confirm_import(
    state: &AppState,
    items: Vec<yoink_shared::ImportConfirmation>,
) -> AppResult<()> {
    let summary = services::confirm_import_library(state, items).await?;
    info!(
        total = summary.total_selected,
        imported = summary.imported,
        artists_added = summary.artists_added,
        failed = summary.failed,
        "Confirmed import completed"
    );
    if !summary.errors.is_empty() {
        return Err(AppError::conflict(format!(
            "Imported {}/{} albums. Errors: {}",
            summary.imported,
            summary.total_selected,
            summary.errors.join("; ")
        )));
    }
    Ok(())
}
