-- 0.6.0: receiving and alerts. Watch-only: every row is a public address or a note about one.

-- Addresses handed out by Receive, derived from a wallet's own descriptor (external chain).
-- An index is handed out once and never again.
CREATE TABLE receive_addresses (
    id                  TEXT PRIMARY KEY,
    wallet_id           TEXT NOT NULL REFERENCES wallets(id) ON DELETE CASCADE,
    address_index       INTEGER NOT NULL CHECK (address_index >= 0),
    address             TEXT NOT NULL,
    label               TEXT,
    amount_sats         INTEGER CHECK (amount_sats IS NULL OR amount_sats > 0),
    device_checked_at   TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(wallet_id, address_index),
    UNIQUE(wallet_id, address)
);

-- Lists of receiving addresses the person copied from their own wallets (Cake, Sparrow, a Jade...).
-- Nakos never derives these. low_alerted remembers that the "running low" alert already fired.
CREATE TABLE address_lists (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    network         TEXT NOT NULL DEFAULT 'bitcoin' CHECK (network IN ('bitcoin','testnet','signet','regtest')),
    low_at          INTEGER NOT NULL DEFAULT 3 CHECK (low_at BETWEEN 0 AND 19),
    low_alerted     INTEGER NOT NULL DEFAULT 0,
    last_checked_at TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE saved_addresses (
    id                  TEXT PRIMARY KEY,
    list_id             TEXT NOT NULL REFERENCES address_lists(id) ON DELETE CASCADE,
    address             TEXT NOT NULL,
    label               TEXT,
    position            INTEGER NOT NULL,
    given_out_at        TEXT,
    used_at             TEXT,
    received_sats       INTEGER,
    device_checked_at   TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(list_id, address)
);

CREATE INDEX idx_saved_addresses_list ON saved_addresses(list_id);

-- "Tell me when this wallet goes over N sats". is_above is the last state seen, so the alert
-- fires once per upward crossing, not on every sync.
CREATE TABLE balance_alerts (
    wallet_id       TEXT PRIMARY KEY REFERENCES wallets(id) ON DELETE CASCADE,
    threshold_sats  INTEGER NOT NULL CHECK (threshold_sats > 0),
    is_above        INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- What the person is shown. ref_id ties an event to the row it is about, so the same payment never
-- makes two alerts (NULL for balance crossings, which can rightly repeat).
CREATE TABLE alert_events (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('balance_over','receive_paid','saved_used','list_low')),
    wallet_id   TEXT REFERENCES wallets(id) ON DELETE CASCADE,
    list_id     TEXT REFERENCES address_lists(id) ON DELETE CASCADE,
    ref_id      TEXT,
    message     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    read_at     TEXT,
    UNIQUE(kind, ref_id)
);

CREATE INDEX idx_alert_events_unread ON alert_events(read_at);
