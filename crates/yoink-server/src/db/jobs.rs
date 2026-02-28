use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::models::DownloadJob;

use super::{new_uuid, parse_dt, parse_status, parse_uuid};

pub(crate) async fn load_jobs(pool: &SqlitePool) -> Result<Vec<DownloadJob>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, album_id, source, album_title, artist_name, status, quality,
                total_tracks, completed_tracks, error, created_at, updated_at
         FROM download_jobs ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let album_id: Vec<u8> = r.get("album_id");
            DownloadJob {
                id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
                album_id: Uuid::from_slice(&album_id).unwrap_or_default().to_string(),
                source: r.get("source"),
                album_title: r.get("album_title"),
                artist_name: r.get("artist_name"),
                status: parse_status(&r.get::<String, _>("status")),
                quality: r.get("quality"),
                total_tracks: r.get::<i32, _>("total_tracks") as usize,
                completed_tracks: r.get::<i32, _>("completed_tracks") as usize,
                error: r.get("error"),
                created_at: parse_dt(r.get::<String, _>("created_at")),
                updated_at: parse_dt(r.get::<String, _>("updated_at")),
            }
        })
        .collect())
}

pub(crate) async fn insert_job(
    pool: &SqlitePool,
    job: &DownloadJob,
) -> Result<String, sqlx::Error> {
    let uuid = parse_uuid(&job.id).unwrap_or_else(|_| new_uuid());
    let album_uuid = parse_uuid(&job.album_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO download_jobs (id, album_id, source, album_title, artist_name, status, quality,
                                    total_tracks, completed_tracks, error, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(uuid.as_bytes().as_slice())
    .bind(album_uuid.as_bytes().as_slice())
    .bind(&job.source)
    .bind(&job.album_title)
    .bind(&job.artist_name)
    .bind(job.status.as_str())
    .bind(&job.quality)
    .bind(job.total_tracks as i32)
    .bind(job.completed_tracks as i32)
    .bind(&job.error)
    .bind(job.created_at.to_rfc3339())
    .bind(job.updated_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(uuid.to_string())
}

pub(crate) async fn update_job(pool: &SqlitePool, job: &DownloadJob) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(&job.id).unwrap_or_default();
    sqlx::query(
        "UPDATE download_jobs SET status = $1, total_tracks = $2, completed_tracks = $3,
                                  error = $4, updated_at = $5
         WHERE id = $6",
    )
    .bind(job.status.as_str())
    .bind(job.total_tracks as i32)
    .bind(job.completed_tracks as i32)
    .bind(&job.error)
    .bind(job.updated_at.to_rfc3339())
    .bind(uuid.as_bytes().as_slice())
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn delete_job(pool: &SqlitePool, job_id: &str) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(job_id).unwrap_or_default();
    sqlx::query("DELETE FROM download_jobs WHERE id = $1")
        .bind(uuid.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_completed_jobs(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM download_jobs WHERE status = 'completed'")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
