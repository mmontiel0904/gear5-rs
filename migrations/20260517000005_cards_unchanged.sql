-- Track cards that were seen but skipped because their payload_hash was unchanged.
-- Populated on both the parent run and per-set rows to give full visibility into
-- how much work was skipped vs actually processed.
ALTER TABLE scrape_runs     ADD COLUMN cards_unchanged INT NOT NULL DEFAULT 0;
ALTER TABLE scrape_run_sets ADD COLUMN cards_unchanged INT NOT NULL DEFAULT 0;
