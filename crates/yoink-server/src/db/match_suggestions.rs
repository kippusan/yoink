use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use super::{new_uuid, parse_dt, parse_uuid};

#[derive(Debug, Clone)]
pub(crate) struct MatchSuggestion {
    pub(crate) id: String,
    pub(crate) scope_type: String,
    pub(crate) scope_id: String,
    pub(crate) left_provider: String,
    pub(crate) left_external_id: String,
    pub(crate) right_provider: String,
    pub(crate) right_external_id: String,
    pub(crate) match_kind: String,
    pub(crate) confidence: u8,
    pub(crate) explanation: Option<String>,
    pub(crate) external_name: Option<String>,
    pub(crate) external_url: Option<String>,
    pub(crate) image_ref: Option<String>,
    pub(crate) disambiguation: Option<String>,
    pub(crate) artist_type: Option<String>,
    pub(crate) country: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) popularity: Option<u8>,
    pub(crate) status: String,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

pub(crate) async fn upsert_match_suggestion(
    pool: &SqlitePool,
    suggestion: &MatchSuggestion,
) -> Result<(), sqlx::Error> {
    let id_uuid = parse_uuid(&suggestion.id).unwrap_or_else(|_| new_uuid());
    let scope_uuid = parse_uuid(&suggestion.scope_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO match_suggestions (
            id, scope_type, scope_id,
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind, confidence, explanation,
            external_name, external_url, image_ref,
            disambiguation, artist_type, country, tags_json, popularity,
            status, created_at, updated_at
         ) VALUES (
            $1, $2, $3,
            $4, $5,
            $6, $7,
            $8, $9, $10,
            $11, $12, $13,
            $14, $15, $16, $17, $18,
            $19, $20, $21
         )
         ON CONFLICT(
            scope_type, scope_id,
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind
         ) DO UPDATE SET
            confidence = excluded.confidence,
            explanation = excluded.explanation,
            external_name = excluded.external_name,
            external_url = excluded.external_url,
            image_ref = excluded.image_ref,
            disambiguation = excluded.disambiguation,
            artist_type = excluded.artist_type,
            country = excluded.country,
            tags_json = excluded.tags_json,
            popularity = excluded.popularity,
            updated_at = excluded.updated_at
         WHERE match_suggestions.status != 'dismissed' AND match_suggestions.status != 'accepted'",
    )
    .bind(id_uuid.as_bytes().as_slice())
    .bind(&suggestion.scope_type)
    .bind(scope_uuid.as_bytes().as_slice())
    .bind(&suggestion.left_provider)
    .bind(&suggestion.left_external_id)
    .bind(&suggestion.right_provider)
    .bind(&suggestion.right_external_id)
    .bind(&suggestion.match_kind)
    .bind(i32::from(suggestion.confidence))
    .bind(&suggestion.explanation)
    .bind(&suggestion.external_name)
    .bind(&suggestion.external_url)
    .bind(&suggestion.image_ref)
    .bind(&suggestion.disambiguation)
    .bind(&suggestion.artist_type)
    .bind(&suggestion.country)
    .bind(json!(suggestion.tags).to_string())
    .bind(suggestion.popularity.map(i32::from))
    .bind(&suggestion.status)
    .bind(suggestion.created_at.to_rfc3339())
    .bind(suggestion.updated_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn clear_pending_match_suggestions(
    pool: &SqlitePool,
    scope_type: &str,
    scope_id: &str,
) -> Result<u64, sqlx::Error> {
    let scope_uuid = parse_uuid(scope_id).unwrap_or_default();
    let result = sqlx::query(
        "DELETE FROM match_suggestions
         WHERE scope_type = $1 AND scope_id = $2 AND status = 'pending'",
    )
    .bind(scope_type)
    .bind(scope_uuid.as_bytes().as_slice())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn load_match_suggestions_for_scope(
    pool: &SqlitePool,
    scope_type: &str,
    scope_id: &str,
) -> Result<Vec<MatchSuggestion>, sqlx::Error> {
    let scope_uuid = parse_uuid(scope_id).unwrap_or_default();
    let rows = sqlx::query(
        "SELECT
            id, scope_type, scope_id,
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind, confidence, explanation,
            external_name, external_url, image_ref,
            disambiguation, artist_type, country, tags_json, popularity,
            status,
            created_at, updated_at
         FROM match_suggestions
         WHERE scope_type = $1 AND scope_id = $2
         ORDER BY status ASC, confidence DESC, created_at DESC",
    )
    .bind(scope_type)
    .bind(scope_uuid.as_bytes().as_slice())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_suggestion).collect())
}

pub(crate) async fn load_match_suggestion_by_id(
    pool: &SqlitePool,
    suggestion_id: &str,
) -> Result<Option<MatchSuggestion>, sqlx::Error> {
    let suggestion_uuid = parse_uuid(suggestion_id).unwrap_or_default();
    let row = sqlx::query(
        "SELECT
            id, scope_type, scope_id,
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind, confidence, explanation,
            external_name, external_url, image_ref,
            disambiguation, artist_type, country, tags_json, popularity,
            status,
            created_at, updated_at
         FROM match_suggestions
         WHERE id = $1",
    )
    .bind(suggestion_uuid.as_bytes().as_slice())
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_suggestion))
}

pub(crate) async fn set_match_suggestion_status(
    pool: &SqlitePool,
    suggestion_id: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    let suggestion_uuid = parse_uuid(suggestion_id).unwrap_or_default();
    sqlx::query(
        "UPDATE match_suggestions
         SET status = $1, updated_at = $2
         WHERE id = $3",
    )
    .bind(status)
    .bind(Utc::now().to_rfc3339())
    .bind(suggestion_uuid.as_bytes().as_slice())
    .execute(pool)
    .await?;
    Ok(())
}

// ── Helper ──────────────────────────────────────────────────────────

fn row_to_suggestion(r: sqlx::sqlite::SqliteRow) -> MatchSuggestion {
    let id: Vec<u8> = r.get("id");
    let scope_id: Vec<u8> = r.get("scope_id");
    MatchSuggestion {
        id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
        scope_type: r.get("scope_type"),
        scope_id: Uuid::from_slice(&scope_id).unwrap_or_default().to_string(),
        left_provider: r.get("left_provider"),
        left_external_id: r.get("left_external_id"),
        right_provider: r.get("right_provider"),
        right_external_id: r.get("right_external_id"),
        match_kind: r.get("match_kind"),
        confidence: r.get::<i32, _>("confidence") as u8,
        explanation: r.get("explanation"),
        external_name: r.get("external_name"),
        external_url: r.get("external_url"),
        image_ref: r.get("image_ref"),
        disambiguation: r.get("disambiguation"),
        artist_type: r.get("artist_type"),
        country: r.get("country"),
        tags: r
            .get::<Option<String>, _>("tags_json")
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default(),
        popularity: r.get::<Option<i32>, _>("popularity").map(|v| v as u8),
        status: r.get("status"),
        created_at: parse_dt(r.get::<String, _>("created_at")),
        updated_at: parse_dt(r.get::<String, _>("updated_at")),
    }
}
