CREATE TABLE api_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    prefix          TEXT NOT NULL UNIQUE,
    hash            TEXT NOT NULL,
    scopes          TEXT[] NOT NULL DEFAULT ARRAY['read']::TEXT[],
    rate_limit_rpm  INT NOT NULL DEFAULT 120,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at    TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ
);

CREATE INDEX api_keys_active_prefix_idx
    ON api_keys (prefix)
    WHERE revoked_at IS NULL;
