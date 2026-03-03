use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::DownloadJob;

use super::parse_status;

/// DB row — maps 1:1 to the download_jobs table columns.
struct JobRow {
    id: Uuid,
    album_id: Uuid,
    source: String,
    album_title: String,
    artist_name: String,
    status: String,
    quality: String,
    total_tracks: i64,
    completed_tracks: i64,
    error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<JobRow> for DownloadJob {
    fn from(r: JobRow) -> Self {
        Self {
            id: r.id,
            album_id: r.album_id,
            source: r.source,
            album_title: r.album_title,
            artist_name: r.artist_name,
            status: parse_status(&r.status),
            quality: r.quality,
            total_tracks: r.total_tracks as usize,
            completed_tracks: r.completed_tracks as usize,
            error: r.error,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub(crate) async fn load_jobs(pool: &SqlitePool) -> Result<Vec<DownloadJob>, sqlx::Error> {
    let rows = sqlx::query_as!(
        JobRow,
        r#"SELECT
            id as "id!: Uuid",
            album_id as "album_id!: Uuid",
            source, album_title, artist_name, status, quality,
            total_tracks, completed_tracks, error,
            created_at as "created_at!: chrono::DateTime<chrono::Utc>",
            updated_at as "updated_at!: chrono::DateTime<chrono::Utc>"
         FROM download_jobs ORDER BY created_at DESC"#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(DownloadJob::from).collect())
}

pub(crate) async fn insert_job(pool: &SqlitePool, job: &DownloadJob) -> Result<Uuid, sqlx::Error> {
    let status = job.status.as_str();
    let total = job.total_tracks as i32;
    let completed = job.completed_tracks as i32;
    sqlx::query!(
        "INSERT INTO download_jobs (id, album_id, source, album_title, artist_name, status, quality,
                                    total_tracks, completed_tracks, error, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        job.id, job.album_id, job.source, job.album_title, job.artist_name, status, job.quality,
        total, completed, job.error, job.created_at, job.updated_at,
    )
    .execute(pool)
    .await?;
    Ok(job.id)
}

pub(crate) async fn update_job(pool: &SqlitePool, job: &DownloadJob) -> Result<(), sqlx::Error> {
    let status = job.status.as_str();
    let total = job.total_tracks as i32;
    let completed = job.completed_tracks as i32;
    sqlx::query!(
        "UPDATE download_jobs SET status = $1, total_tracks = $2, completed_tracks = $3,
                                  error = $4, updated_at = $5
         WHERE id = $6",
        status,
        total,
        completed,
        job.error,
        job.updated_at,
        job.id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn delete_job(pool: &SqlitePool, job_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM download_jobs WHERE id = $1", job_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_completed_jobs(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!("DELETE FROM download_jobs WHERE status = 'completed'")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
