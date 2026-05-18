-- Store a SHA-256 hash of the raw HTML fetched for each set.
-- Used by the scraper to detect unchanged pages and skip redundant card upserts.
ALTER TABLE sets ADD COLUMN html_hash BYTEA;
