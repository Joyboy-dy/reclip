CREATE TABLE api_keys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    secret_sha256 TEXT NOT NULL,
    -- comma-separated scope names
    scopes TEXT NOT NULL,
    unlimited INTEGER NOT NULL DEFAULT 0,
    probes_per_minute INTEGER,
    jobs_per_hour INTEGER,
    max_active INTEGER,
    created_at TEXT NOT NULL,
    expires_at TEXT,
    revoked_at TEXT,
    last_used_at TEXT,
    request_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

ALTER TABLE jobs ADD COLUMN quota_key TEXT NOT NULL DEFAULT '';

CREATE INDEX idx_jobs_quota_state ON jobs (quota_key, state);

-- Phase-2 follow-up: failed and cancelled rows written before expiry was
-- stamped on them never expire. Make them due now.
UPDATE jobs
SET expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE state IN ('failed', 'cancelled') AND expires_at IS NULL;
