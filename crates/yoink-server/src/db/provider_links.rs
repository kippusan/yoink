use sqlx::SqlitePool;
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
    sqlx::query!(
        "INSERT INTO artist_provider_links (id, artist_id, provider, external_id, external_url, external_name, image_ref)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           external_url = excluded.external_url,
           external_name = excluded.external_name,
           image_ref = excluded.image_ref",
        link.id, link.artist_id, link.provider, link.external_id, link.external_url, link.external_name, link.image_ref,
    )
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
    sqlx::query!(
        "DELETE FROM artist_provider_links WHERE artist_id = $1 AND provider = $2 AND external_id = $3",
        artist_id, provider, external_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn load_artist_provider_links(
    pool: &SqlitePool,
    artist_id: Uuid,
) -> Result<Vec<ArtistProviderLink>, sqlx::Error> {
    sqlx::query_as!(
        ArtistProviderLink,
        r#"SELECT
            id as "id!: Uuid",
            artist_id as "artist_id!: Uuid",
            provider, external_id, external_url, external_name, image_ref
         FROM artist_provider_links WHERE artist_id = $1"#,
        artist_id,
    )
    .fetch_all(pool)
    .await
}

/// Find a local artist by a provider link's external_id.
pub(crate) async fn find_artist_by_provider_link(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT artist_id as "artist_id!: Uuid"
           FROM artist_provider_links WHERE provider = $1 AND external_id = $2"#,
        provider, external_id,
    )
    .fetch_optional(pool)
    .await
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
    sqlx::query!(
        "INSERT INTO album_provider_links (id, album_id, provider, external_id, external_url, external_title, cover_ref)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           album_id = excluded.album_id,
           external_url = excluded.external_url,
           external_title = excluded.external_title,
           cover_ref = excluded.cover_ref",
        link.id, link.album_id, link.provider, link.external_id, link.external_url, link.external_title, link.cover_ref,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn load_album_provider_links(
    pool: &SqlitePool,
    album_id: Uuid,
) -> Result<Vec<AlbumProviderLink>, sqlx::Error> {
    sqlx::query_as!(
        AlbumProviderLink,
        r#"SELECT
            id as "id!: Uuid",
            album_id as "album_id!: Uuid",
            provider, external_id, external_url, external_title, cover_ref
         FROM album_provider_links WHERE album_id = $1"#,
        album_id,
    )
    .fetch_all(pool)
    .await
}

/// Find a local album by a provider link's external_id.
pub(crate) async fn find_album_by_provider_link(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT album_id as "album_id!: Uuid"
           FROM album_provider_links WHERE provider = $1 AND external_id = $2"#,
        provider, external_id,
    )
    .fetch_optional(pool)
    .await
}

// ── Track provider links ────────────────────────────────────────────

#[allow(dead_code)]
pub(crate) async fn upsert_track_provider_link(
    pool: &SqlitePool,
    track_id: Uuid,
    provider: &str,
    external_id: &str,
) -> Result<(), sqlx::Error> {
    let link_id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO track_provider_links (id, track_id, provider, external_id)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           track_id = excluded.track_id",
        link_id, track_id, provider, external_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}
