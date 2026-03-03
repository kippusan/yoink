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
    track_artist: Option<String>,
    file_path: Option<String>,
    monitored: bool,
    acquired: bool,
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
            track_artist: r.track_artist,
            file_path: r.file_path,
            monitored: r.monitored,
            acquired: r.acquired,
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
            isrc, track_artist, file_path,
            monitored as "monitored!: bool",
            acquired as "acquired!: bool"
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
        "INSERT INTO tracks (id, album_id, title, version, disc_number, track_number, duration_secs, explicit, isrc, track_artist, file_path, monitored, acquired)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           version = excluded.version,
           disc_number = excluded.disc_number,
           track_number = excluded.track_number,
           duration_secs = excluded.duration_secs,
           explicit = excluded.explicit,
           isrc = excluded.isrc,
           track_artist = COALESCE(excluded.track_artist, tracks.track_artist),
           file_path = COALESCE(excluded.file_path, tracks.file_path),
           monitored = excluded.monitored,
           acquired = excluded.acquired",
        track.id, album_id, track.title, track.version, disc, tnum, dur, track.explicit, track.isrc,
        track.track_artist, track.file_path, track.monitored, track.acquired,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Update only the monitored and acquired flags for a track.
pub(crate) async fn update_track_flags(
    pool: &SqlitePool,
    track_id: Uuid,
    monitored: bool,
    acquired: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE tracks SET monitored = $1, acquired = $2 WHERE id = $3",
        monitored,
        acquired,
        track_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Load all monitored tracks for an album (tracks individually selected for download).
pub(crate) async fn load_monitored_tracks_for_album(
    pool: &SqlitePool,
    album_id: Uuid,
) -> Result<Vec<TrackInfo>, sqlx::Error> {
    let rows = sqlx::query_as!(
        TrackRow,
        r#"SELECT
            id as "id!: Uuid",
            title, version, disc_number, track_number, duration_secs,
            explicit as "explicit!: bool",
            isrc, track_artist, file_path,
            monitored as "monitored!: bool",
            acquired as "acquired!: bool"
         FROM tracks WHERE album_id = $1 AND monitored = 1
         ORDER BY disc_number, track_number"#,
        album_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(TrackInfo::from).collect())
}

/// Check if an album has any individually monitored tracks that are not yet acquired.
pub(crate) async fn has_wanted_tracks(
    pool: &SqlitePool,
    album_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64"
           FROM tracks WHERE album_id = $1 AND monitored = 1 AND acquired = 0"#,
        album_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
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

/// Load all tracks across all albums, joined with album title and primary artist name.
/// Used for the library-wide Tracks tab.
pub(crate) async fn load_all_tracks(
    pool: &SqlitePool,
) -> Result<Vec<(TrackInfo, Uuid, String, Uuid, String)>, sqlx::Error> {
    struct JoinedRow {
        id: Uuid,
        title: String,
        version: Option<String>,
        disc_number: i64,
        track_number: i64,
        duration_secs: Option<i64>,
        explicit: bool,
        isrc: Option<String>,
        track_artist: Option<String>,
        file_path: Option<String>,
        monitored: bool,
        acquired: bool,
        album_id: Uuid,
        album_title: String,
        artist_id: Uuid,
        artist_name: String,
    }

    let rows = sqlx::query_as!(
        JoinedRow,
        r#"SELECT
            t.id as "id!: Uuid",
            t.title, t.version, t.disc_number, t.track_number, t.duration_secs,
            t.explicit as "explicit!: bool",
            t.isrc, t.track_artist, t.file_path,
            t.monitored as "monitored!: bool",
            t.acquired as "acquired!: bool",
            a.id as "album_id!: Uuid",
            a.title as album_title,
            ar.id as "artist_id!: Uuid",
            ar.name as artist_name
         FROM tracks t
         JOIN albums a ON t.album_id = a.id
         JOIN artists ar ON a.artist_id = ar.id
         ORDER BY ar.name, a.title, t.disc_number, t.track_number"#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let secs = r.duration_secs.unwrap_or(0) as u32;
            let mins = secs / 60;
            let rem = secs % 60;
            let track = TrackInfo {
                id: r.id,
                title: r.title,
                version: r.version,
                disc_number: r.disc_number as u32,
                track_number: r.track_number as u32,
                duration_secs: secs,
                duration_display: format!("{mins}:{rem:02}"),
                isrc: r.isrc,
                explicit: r.explicit,
                track_artist: r.track_artist,
                file_path: r.file_path,
                monitored: r.monitored,
                acquired: r.acquired,
            };
            (track, r.album_id, r.album_title, r.artist_id, r.artist_name)
        })
        .collect())
}

/// Check whether ALL monitored tracks for an album have been acquired.
/// Returns `true` when there are no monitored-but-unacquired tracks,
/// including the degenerate case where no tracks are monitored at all.
pub(crate) async fn all_monitored_tracks_acquired(
    pool: &SqlitePool,
    album_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64"
           FROM tracks WHERE album_id = $1 AND monitored = 1 AND acquired = 0"#,
        album_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(count == 0)
}
