CREATE TABLE admin_accounts (
    email TEXT PRIMARY KEY,
    -- NULL until the first sign-in sets a password.
    password_hash TEXT,
    must_change INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE admin_sessions (
    token_sha256 TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE INDEX idx_admin_sessions_email ON admin_sessions (email);
