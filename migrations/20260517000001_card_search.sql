-- Per-keystroke card search: normalized name column + prefix and trigram indexes.
--
-- The autocomplete endpoint `/cards/search` does two passes:
--   1. Prefix match on `name_norm LIKE $1 || '%'` — backed by btree (text_pattern_ops).
--   2. Trigram similarity fallback `name_norm % $1` — backed by GIN (gin_trgm_ops).
--
-- `name_norm` is a stored generated column so the index expression is trivial and
-- writes pay the cost once per scrape, not per query. The `unaccent_immutable`
-- wrapper exists because `unaccent(text)` is declared STABLE (it depends on a
-- dictionary), and generated columns / expression indexes require IMMUTABLE.

CREATE EXTENSION IF NOT EXISTS unaccent;

CREATE OR REPLACE FUNCTION unaccent_immutable(text)
RETURNS text
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
STRICT
AS $$ SELECT public.unaccent('public.unaccent', $1) $$;

ALTER TABLE cards
    ADD COLUMN name_norm TEXT
    GENERATED ALWAYS AS (lower(unaccent_immutable(name))) STORED;

CREATE INDEX cards_name_norm_prefix
    ON cards (name_norm text_pattern_ops);

CREATE INDEX cards_name_norm_trgm
    ON cards USING gin (name_norm gin_trgm_ops);
