-- Stores the raw artist credits from providers as a JSON array.
-- Each entry: {"name":"…","provider":"tidal","external_id":"123"}
ALTER TABLE albums ADD COLUMN artist_credits TEXT;
