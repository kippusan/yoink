DROP INDEX IF EXISTS idx_match_suggestions_status;
DROP INDEX IF EXISTS idx_match_suggestions_scope;

CREATE TABLE match_suggestions_tmp (
    id                BLOB PRIMARY KEY,
    scope_type        TEXT NOT NULL,
    scope_id          BLOB NOT NULL,
    left_provider     TEXT NOT NULL,
    left_external_id  TEXT NOT NULL,
    right_provider    TEXT NOT NULL,
    right_external_id TEXT NOT NULL,
    match_kind        TEXT NOT NULL,
    confidence        INTEGER NOT NULL,
    explanation       TEXT,
    status            TEXT NOT NULL DEFAULT 'pending',
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    UNIQUE(
        scope_type,
        scope_id,
        left_provider,
        left_external_id,
        right_provider,
        right_external_id,
        match_kind
    )
);

INSERT INTO match_suggestions_tmp (
    id,
    scope_type,
    scope_id,
    left_provider,
    left_external_id,
    right_provider,
    right_external_id,
    match_kind,
    confidence,
    explanation,
    status,
    created_at,
    updated_at
)
SELECT
    id,
    scope_type,
    scope_id,
    left_provider,
    left_external_id,
    right_provider,
    right_external_id,
    match_kind,
    confidence,
    explanation,
    status,
    created_at,
    updated_at
FROM match_suggestions;

DROP TABLE match_suggestions;
ALTER TABLE match_suggestions_tmp RENAME TO match_suggestions;

CREATE INDEX idx_match_suggestions_scope ON match_suggestions(scope_type, scope_id);
CREATE INDEX idx_match_suggestions_status ON match_suggestions(status);
