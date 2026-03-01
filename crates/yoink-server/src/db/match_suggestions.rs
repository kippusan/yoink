use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use sqlx::types::Json;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct MatchSuggestion {
    pub(crate) id: Uuid,
    pub(crate) scope_type: String,
    pub(crate) scope_id: Uuid,
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

/// DB row — matches column types exactly for `query_as!`.
struct SuggestionRow {
    id: Uuid,
    scope_type: String,
    scope_id: Uuid,
    left_provider: String,
    left_external_id: String,
    right_provider: String,
    right_external_id: String,
    match_kind: String,
    confidence: i64,
    explanation: Option<String>,
    external_name: Option<String>,
    external_url: Option<String>,
    image_ref: Option<String>,
    disambiguation: Option<String>,
    artist_type: Option<String>,
    country: Option<String>,
    tags_json: Json<Vec<String>>,
    popularity: Option<i64>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<SuggestionRow> for MatchSuggestion {
    fn from(r: SuggestionRow) -> Self {
        Self {
            id: r.id,
            scope_type: r.scope_type,
            scope_id: r.scope_id,
            left_provider: r.left_provider,
            left_external_id: r.left_external_id,
            right_provider: r.right_provider,
            right_external_id: r.right_external_id,
            match_kind: r.match_kind,
            confidence: r.confidence as u8,
            explanation: r.explanation,
            external_name: r.external_name,
            external_url: r.external_url,
            image_ref: r.image_ref,
            disambiguation: r.disambiguation,
            artist_type: r.artist_type,
            country: r.country,
            tags: r.tags_json.0,
            popularity: r.popularity.map(|v| v as u8),
            status: r.status,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub(crate) async fn upsert_match_suggestion(
    pool: &SqlitePool,
    s: &MatchSuggestion,
) -> Result<(), sqlx::Error> {
    let confidence = i32::from(s.confidence);
    let tags_json = Json(&s.tags);
    let popularity = s.popularity.map(i32::from);
    sqlx::query!(
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
        s.id, s.scope_type, s.scope_id,
        s.left_provider, s.left_external_id,
        s.right_provider, s.right_external_id,
        s.match_kind, confidence, s.explanation,
        s.external_name, s.external_url, s.image_ref,
        s.disambiguation, s.artist_type, s.country, tags_json, popularity,
        s.status, s.created_at, s.updated_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn clear_pending_match_suggestions(
    pool: &SqlitePool,
    scope_type: &str,
    scope_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM match_suggestions
         WHERE scope_type = $1 AND scope_id = $2 AND status = 'pending'",
        scope_type, scope_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn load_match_suggestions_for_scope(
    pool: &SqlitePool,
    scope_type: &str,
    scope_id: Uuid,
) -> Result<Vec<MatchSuggestion>, sqlx::Error> {
    let rows = sqlx::query_as!(
        SuggestionRow,
        r#"SELECT
            id as "id!: Uuid",
            scope_type, scope_id as "scope_id!: Uuid",
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind, confidence, explanation,
            external_name, external_url, image_ref,
            disambiguation, artist_type, country,
            COALESCE(tags_json, '[]') as "tags_json!: Json<Vec<String>>",
            popularity,
            status,
            created_at as "created_at!: chrono::DateTime<chrono::Utc>",
            updated_at as "updated_at!: chrono::DateTime<chrono::Utc>"
         FROM match_suggestions
         WHERE scope_type = $1 AND scope_id = $2
         ORDER BY status ASC, confidence DESC, created_at DESC"#,
        scope_type, scope_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(MatchSuggestion::from).collect())
}

pub(crate) async fn load_match_suggestion_by_id(
    pool: &SqlitePool,
    suggestion_id: Uuid,
) -> Result<Option<MatchSuggestion>, sqlx::Error> {
    let row = sqlx::query_as!(
        SuggestionRow,
        r#"SELECT
            id as "id!: Uuid",
            scope_type, scope_id as "scope_id!: Uuid",
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind, confidence, explanation,
            external_name, external_url, image_ref,
            disambiguation, artist_type, country,
            COALESCE(tags_json, '[]') as "tags_json!: Json<Vec<String>>",
            popularity,
            status,
            created_at as "created_at!: chrono::DateTime<chrono::Utc>",
            updated_at as "updated_at!: chrono::DateTime<chrono::Utc>"
         FROM match_suggestions
         WHERE id = $1"#,
        suggestion_id,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(MatchSuggestion::from))
}

pub(crate) async fn set_match_suggestion_status(
    pool: &SqlitePool,
    suggestion_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    sqlx::query!(
        "UPDATE match_suggestions
         SET status = $1, updated_at = $2
         WHERE id = $3",
        status, now, suggestion_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}
