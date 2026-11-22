-- Who uses ReClip, from where, and who is blocked. Activity rows are purged
-- after RECLIP_HISTORY_DAYS, like the download history.

-- One row per identity: a web device (`web:<cookie>`), a Telegram account
-- (`tg:<id>`) or an API key (`key:<id>`).
CREATE TABLE clients (
    owner TEXT PRIMARY KEY NOT NULL,
    -- Summary of the last User-Agent: "Chrome 131 · Windows · desktop".
    device TEXT,
    -- Telegram accounts only, as the bot last saw them.
    tg_username TEXT,
    tg_name TEXT,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    last_ip TEXT
);

CREATE INDEX idx_clients_last_seen ON clients (last_seen);
CREATE INDEX idx_clients_first_seen ON clients (first_seen);

-- Addresses each identity came from (an IPv6 address counts as its /64).
CREATE TABLE client_ips (
    owner TEXT NOT NULL,
    ip TEXT NOT NULL,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    hits INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (owner, ip)
);

CREATE INDEX idx_client_ips_ip ON client_ips (ip, last_seen);
CREATE INDEX idx_client_ips_last_seen ON client_ips (last_seen);

-- Where each job came from and how long it ran.
ALTER TABLE jobs ADD COLUMN ip TEXT;
ALTER TABLE jobs ADD COLUMN device TEXT;
ALTER TABLE jobs ADD COLUMN duration_ms INTEGER;

-- Kept until an admin lifts them.
CREATE TABLE blocks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- `owner`: a web device, Telegram account or API key; `ip`: an address
    -- or a range in CIDR notation.
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    created_by TEXT,
    UNIQUE (kind, target)
);
