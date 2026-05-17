CREATE TABLE scrape_run_sets (
    run_id        BIGINT      NOT NULL REFERENCES scrape_runs(id) ON DELETE CASCADE,
    source_series TEXT        NOT NULL,
    set_id        TEXT,
    cards_seen    INTEGER     NOT NULL DEFAULT 0,
    status        TEXT        NOT NULL,
    error         TEXT,
    started_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at   TIMESTAMPTZ,
    PRIMARY KEY (run_id, source_series)
);

CREATE INDEX scrape_run_sets_run_idx ON scrape_run_sets(run_id);
