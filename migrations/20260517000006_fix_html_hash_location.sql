-- Move html_hash from sets to scrape_run_sets.
--
-- html_hash tracks whether a *source_series page* has changed since the last
-- scrape, not whether a set entity changed. Storing it on scrape_run_sets
-- (keyed by source_series) is the correct place: it is a property of the
-- fetch/scrape event, not of the set itself. The old location on sets caused
-- incorrect updates when cross-set pages (e.g. Promotion cards) wrote a
-- single hash to multiple set rows with different source_series values.
ALTER TABLE sets             DROP COLUMN IF EXISTS html_hash;
ALTER TABLE scrape_run_sets  ADD  COLUMN IF NOT EXISTS html_hash BYTEA;
