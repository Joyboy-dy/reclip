CREATE TABLE jobs (
    id TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    format TEXT NOT NULL,
    format_id TEXT,
    title TEXT,
    state TEXT NOT NULL,
    filename TEXT,
    size INTEGER,
    file_path TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT,
    expires_at TEXT
);

CREATE INDEX idx_jobs_state ON jobs (state);
