-- Phase-1 `done` rows have no expiry, so the sweep would never remove them.
-- Make them due now: the next sweep expires them and deletes their files.
UPDATE jobs
SET expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE state = 'done' AND expires_at IS NULL;
