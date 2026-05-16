CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TABLE sets (
    id              TEXT PRIMARY KEY,
    source_series   TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    display_label   TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE cards (
    code            TEXT PRIMARY KEY,
    set_id          TEXT NOT NULL REFERENCES sets(id) ON UPDATE CASCADE ON DELETE RESTRICT,
    name            TEXT NOT NULL,
    rarity          TEXT NOT NULL,
    category        TEXT NOT NULL,
    color           TEXT NOT NULL,
    colors          TEXT[] NOT NULL,
    cost            INT,
    life            INT,
    power           INT,
    counter         INT,
    attribute       TEXT,
    block           INT,
    card_type       TEXT,
    features        TEXT[] NOT NULL DEFAULT '{}',
    effect_text     TEXT,
    trigger_text    TEXT,
    notes           TEXT,
    image_path      TEXT NOT NULL,
    image_version   TEXT NOT NULL,
    payload_hash    BYTEA NOT NULL,
    first_seen_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX cards_set_id_idx     ON cards (set_id);
CREATE INDEX cards_category_idx   ON cards (category);
CREATE INDEX cards_rarity_idx     ON cards (rarity);
CREATE INDEX cards_colors_gin     ON cards USING gin (colors);
CREATE INDEX cards_features_gin   ON cards USING gin (features);
CREATE INDEX cards_name_trgm      ON cards USING gin (name gin_trgm_ops);
CREATE INDEX cards_effect_trgm    ON cards USING gin (effect_text gin_trgm_ops);

CREATE TABLE scrape_runs (
    id              BIGSERIAL PRIMARY KEY,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ,
    status          TEXT NOT NULL,
    sets_total      INT,
    sets_ok         INT,
    cards_seen      INT,
    cards_inserted  INT,
    cards_updated   INT,
    error           TEXT
);

CREATE INDEX scrape_runs_finished_idx ON scrape_runs (finished_at DESC);
CREATE INDEX scrape_runs_started_idx  ON scrape_runs (started_at DESC);
