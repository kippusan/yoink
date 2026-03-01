use sqlx::{Row, SqlitePool};
use uuid::Uuid;

// ── Artist provider links ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ArtistProviderLink {
    pub(crate) id: Uuid,
    pub(crate) artist_id: Uuid,
    pub(crate) provider: String,
    pub(crate) external_id: String,
    pub(crate) external_url: Option<String>,
    pub(crate) external_name: Option<String>,
    pub(crate) image_ref: Option<String>,
}

pub(crate) async fn upsert_artist_provider_link(
    pool: &SqlitePool,
    link: &ArtistProviderLink,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO artist_provider_links (id, artist_id, provider, external_id, external_url, external_name, image_ref)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           external_url = excluded.external_url,
           external_name = excluded.external_name,
           image_ref = excluded.image_ref",
    )
    .bind(link.id.as_bytes().as_slice())
    .bind(link.artist_id.as_bytes().as_slice())
    .bind(&link.provider)
    .bind(&link.external_id)
    .bind(&link.external_url)
    .bind(&link.external_name)
    .bind(&link.image_ref)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_artist_provider_link(
    pool: &SqlitePool,
    artist_id: Uuid,
    provider: &str,
    external_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM artist_provider_links WHERE artist_id = $1 AND provider = $2 AND external_id = $3",
    )
    .bind(artist_id.as_bytes().as_slice())
    .bind(provider)
    .bind(external_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn load_artist_provider_links(
    pool: &SqlitePool,
    artist_id: Uuid,
) -> Result<Vec<ArtistProviderLink>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, artist_id, provider, external_id, external_url, external_name, image_ref
         FROM artist_provider_links WHERE artist_id = $1",
    )
    .bind(artist_id.as_bytes().as_slice())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let artist_id: Vec<u8> = r.get("artist_id");
            ArtistProviderLink {
                id: Uuid::from_slice(&id).unwrap_or_default(),
                artist_id: Uuid::from_slice(&artist_id).unwrap_or_default(),
                provider: r.get("provider"),
                external_id: r.get("external_id"),
                external_url: r.get("external_url"),
                external_name: r.get("external_name"),
                image_ref: r.get("image_ref"),
            }
        })
        .collect())
}

/// Find a local artist by a provider link's external_id.
pub(crate) async fn find_artist_by_provider_link(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT artist_id FROM artist_provider_links WHERE provider = $1 AND external_id = $2",
    )
    .bind(provider)
    .bind(external_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("artist_id");
        Uuid::from_slice(&id).unwrap_or_default()
    }))
}

// ── Album provider links ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct AlbumProviderLink {
    pub(crate) id: Uuid,
    pub(crate) album_id: Uuid,
    pub(crate) provider: String,
    pub(crate) external_id: String,
    pub(crate) external_url: Option<String>,
    pub(crate) external_title: Option<String>,
    pub(crate) cover_ref: Option<String>,
}

pub(crate) async fn upsert_album_provider_link(
    pool: &SqlitePool,
    link: &AlbumProviderLink,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO album_provider_links (id, album_id, provider, external_id, external_url, external_title, cover_ref)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           album_id = excluded.album_id,
           external_url = excluded.external_url,
           external_title = excluded.external_title,
           cover_ref = excluded.cover_ref",
    )
    .bind(link.id.as_bytes().as_slice())
    .bind(link.album_id.as_bytes().as_slice())
    .bind(&link.provider)
    .bind(&link.external_id)
    .bind(&link.external_url)
    .bind(&link.external_title)
    .bind(&link.cover_ref)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn load_album_provider_links(
    pool: &SqlitePool,
    album_id: Uuid,
) -> Result<Vec<AlbumProviderLink>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, album_id, provider, external_id, external_url, external_title, cover_ref
         FROM album_provider_links WHERE album_id = $1",
    )
    .bind(album_id.as_bytes().as_slice())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let album_id: Vec<u8> = r.get("album_id");
            AlbumProviderLink {
                id: Uuid::from_slice(&id).unwrap_or_default(),
                album_id: Uuid::from_slice(&album_id).unwrap_or_default(),
                provider: r.get("provider"),
                external_id: r.get("external_id"),
                external_url: r.get("external_url"),
                external_title: r.get("external_title"),
                cover_ref: r.get("cover_ref"),
            }
        })
        .collect())
}

/// Find a local album by a provider link's external_id.
pub(crate) async fn find_album_by_provider_link(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT album_id FROM album_provider_links WHERE provider = $1 AND external_id = $2",
    )
    .bind(provider)
    .bind(external_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("album_id");
        Uuid::from_slice(&id).unwrap_or_default()
    }))
}

// ── Track provider links ────────────────────────────────────────────

#[allow(dead_code)]
pub(crate) async fn upsert_track_provider_link(
    pool: &SqlitePool,
    track_id: Uuid,
    provider: &str,
    external_id: &str,
) -> Result<(), sqlx::Error> {
    let link_uuid = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO track_provider_links (id, track_id, provider, external_id)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           track_id = excluded.track_id",
    )
    .bind(link_uuid.as_bytes().as_slice())
    .bind(track_id.as_bytes().as_slice())
    .bind(provider)
    .bind(external_id)
    .execute(pool)
    .await?;
    Ok(())
}
