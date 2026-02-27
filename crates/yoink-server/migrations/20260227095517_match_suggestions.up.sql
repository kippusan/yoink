CREATE TABLE match_suggestions (
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

CREATE INDEX idx_match_suggestions_scope ON match_suggestions(scope_type, scope_id);
CREATE INDEX idx_match_suggestions_status ON match_suggestions(status);
