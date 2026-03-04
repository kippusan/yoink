use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::{db, services, state::AppState};

pub(super) async fn cancel_download(state: &AppState, job_id: Uuid) -> Result<(), String> {
    let mut jobs = state.download_jobs.write().await;
    if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id)
        && matches!(job.status, yoink_shared::DownloadStatus::Queued)
    {
        job.status = yoink_shared::DownloadStatus::Failed;
        job.error = Some("Cancelled by user".to_string());
        job.updated_at = Utc::now();
        let _ = db::update_job(&state.db, job).await;
        info!(%job_id, "Cancelled download job");
    }
    drop(jobs);
    state.notify_sse();
    Ok(())
}

pub(super) async fn clear_completed(state: &AppState) -> Result<(), String> {
    let _ = db::delete_completed_jobs(&state.db).await;
    {
        let mut jobs = state.download_jobs.write().await;
        jobs.retain(|j| j.status != yoink_shared::DownloadStatus::Completed);
    }
    info!("Cleared completed download jobs");
    state.notify_sse();
    Ok(())
}

pub(super) async fn retry_download(state: &AppState, album_id: Uuid) -> Result<(), String> {
    {
        let mut jobs = state.download_jobs.write().await;
        if let Some(job) = jobs.iter_mut().find(|j| {
            j.album_id == album_id && j.status == yoink_shared::DownloadStatus::Failed
        }) {
            let previous_quality = job.quality;
            job.status = yoink_shared::DownloadStatus::Queued;
            job.quality = state.default_quality;
            job.error = None;
            job.updated_at = Utc::now();
            let _ = db::update_job(&state.db, job).await;
            info!(
                %album_id,
                job_id = %job.id,
                previous_quality = %previous_quality,
                retry_quality = %job.quality,
                "Retrying failed download job"
            );
            state.download_notify.notify_one();
            state.notify_sse();
            return Ok(());
        }
    }
    let album = {
        let albums = state.monitored_albums.read().await;
        albums.iter().find(|a| a.id == album_id).cloned()
    };
    if let Some(album) = album {
        info!(album_id = %album.id, title = %album.title, "Creating retry download job");
        services::enqueue_album_download(state, &album).await;
    }
    state.notify_sse();
    Ok(())
}
