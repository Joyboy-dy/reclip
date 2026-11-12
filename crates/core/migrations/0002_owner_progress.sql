ALTER TABLE jobs ADD COLUMN owner TEXT NOT NULL DEFAULT '';
ALTER TABLE jobs ADD COLUMN thumbnail TEXT;

CREATE INDEX idx_jobs_owner_created ON jobs (owner, created_at);
