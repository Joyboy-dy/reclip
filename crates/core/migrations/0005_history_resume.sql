-- Phase 3b: clip, metadata stripping, playlist item, history and thumbnail.
ALTER TABLE jobs ADD COLUMN clip_start REAL;
ALTER TABLE jobs ADD COLUMN clip_end REAL;
ALTER TABLE jobs ADD COLUMN strip_metadata INTEGER NOT NULL DEFAULT 0;
ALTER TABLE jobs ADD COLUMN playlist_item INTEGER;
-- Set when the visitor removes the row from their history.
ALTER TABLE jobs ADD COLUMN hidden_at TEXT;
-- Small local thumbnail kept for the history.
ALTER TABLE jobs ADD COLUMN thumb_path TEXT;
