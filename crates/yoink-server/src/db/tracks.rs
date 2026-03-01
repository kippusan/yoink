use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::TrackInfo;

/// Row struct for tracks — `duration_display` is computed after load.
struct TrackRow {
    id: Uuid,
    title: String,
    version: Option<String>,
    disc_number: i64,
    track_number: i64,
    duration_secs: Option<i64>,
    explicit: bool,
    isrc: Option<String>,
}

impl From<TrackRow> for TrackInfo {
    fn from(r: TrackRow) -> Self {
        let secs = r.duration_secs.unwrap_or(0) as u32;
        let mins = secs / 60;
        let rem = secs % 60;
        Self {
            id: r.id,
            title: r.title,
            version: r.version,
            disc_number: r.disc_number as u32,
            track_number: r.track_number as u32,
            duration_secs: secs,
            duration_display: format!("{mins}:{rem:02}"),
            isrc: r.isrc,
            explicit: r.explicit,
        }
    }
}

pub(crate) async fn load_tracks_for_album(
    pool: &SqlitePool,
    album_id: Uuid,
) -> Result<Vec<TrackInfo>, sqlx::Error> {
    let rows = sqlx::query_as!(
        TrackRow,
        r#"SELECT
            id as "id!: Uuid",
            title, version, disc_number, track_number, duration_secs,
            explicit as "explicit!: bool",
            isrc
         FROM tracks WHERE album_id = $1
         ORDER BY disc_number, track_number"#,
        album_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(TrackInfo::from).collect())
}

pub(crate) async fn upsert_track(
    pool: &SqlitePool,
    track: &TrackInfo,
    album_id: Uuid,
) -> Result<(), sqlx::Error> {
    let disc = track.disc_number as i32;
    let tnum = track.track_number as i32;
    let dur = track.duration_secs as i32;
    sqlx::query!(
        "INSERT INTO tracks (id, album_id, title, version, disc_number, track_number, duration_secs, explicit, isrc)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           version = excluded.version,
           disc_number = excluded.disc_number,
           track_number = excluded.track_number,
           duration_secs = excluded.duration_secs,
           explicit = excluded.explicit,
           isrc = excluded.isrc",
        track.id, album_id, track.title, track.version, disc, tnum, dur, track.explicit, track.isrc,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn find_track_by_provider_link(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT track_id as "track_id!: Uuid"
           FROM track_provider_links WHERE provider = $1 AND external_id = $2"#,
        provider,
        external_id,
    )
    .fetch_optional(pool)
    .await
}

pub(crate) async fn find_track_by_album_isrc(
    pool: &SqlitePool,
    album_id: Uuid,
    isrc: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT id as "id!: Uuid"
           FROM tracks WHERE album_id = $1 AND UPPER(isrc) = UPPER($2) LIMIT 1"#,
        album_id,
        isrc,
    )
    .fetch_optional(pool)
    .await
}

pub(crate) async fn find_track_by_album_position(
    pool: &SqlitePool,
    album_id: Uuid,
    disc_number: u32,
    track_number: u32,
) -> Result<Option<Uuid>, sqlx::Error> {
    let disc = disc_number as i32;
    let tnum = track_number as i32;
    sqlx::query_scalar!(
        r#"SELECT id as "id!: Uuid"
           FROM tracks
           WHERE album_id = $1 AND disc_number = $2 AND track_number = $3
           LIMIT 1"#,
        album_id,
        disc,
        tnum,
    )
    .fetch_optional(pool)
    .await
}
