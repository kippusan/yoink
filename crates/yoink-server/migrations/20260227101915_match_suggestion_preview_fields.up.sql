ALTER TABLE match_suggestions ADD COLUMN external_name TEXT;
ALTER TABLE match_suggestions ADD COLUMN external_url TEXT;
ALTER TABLE match_suggestions ADD COLUMN image_ref TEXT;
ALTER TABLE match_suggestions ADD COLUMN disambiguation TEXT;
ALTER TABLE match_suggestions ADD COLUMN artist_type TEXT;
ALTER TABLE match_suggestions ADD COLUMN country TEXT;
ALTER TABLE match_suggestions ADD COLUMN tags_json TEXT;
ALTER TABLE match_suggestions ADD COLUMN popularity INTEGER;
