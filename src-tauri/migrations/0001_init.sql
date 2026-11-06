-- Nakos local database schema (v1)
-- All data is local-only. No network sync of any kind.
-- Sensitive blobs (inheritance instructions, rpc credentials, contact notes)
-- are AES-256-GCM encrypted at rest using a key derived from the user's
-- passphrase via Argon2id (see src-tauri/src/crypto).

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------
-- Wallets: every source of Bitcoin the user tracks, watch-only.
-- ---------------------------------------------------------------------
CREATE TABLE wallets (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    custody_type        TEXT NOT NULL CHECK (custody_type IN (
                            'cold_storage', 'multisig_vault', 'hot_wallet',
                            'lightning', 'mining', 'exchange', 'etf',
                            'retirement', 'other'
                        )),
    risk_level          TEXT NOT NULL DEFAULT 'medium' CHECK (risk_level IN ('low','medium','high')),
    source_type         TEXT NOT NULL CHECK (source_type IN (
                            'descriptor', 'xpub', 'ypub', 'zpub', 'address',
                            'multisig_descriptor', 'manual'
                        )),
    descriptor          TEXT,
    change_descriptor   TEXT,
    xpub                TEXT,
    network             TEXT NOT NULL DEFAULT 'bitcoin' CHECK (network IN ('bitcoin','testnet','signet','regtest')),
    gap_limit           INTEGER NOT NULL DEFAULT 20,
    is_watch_only       INTEGER NOT NULL DEFAULT 1,
    manual_balance_sats INTEGER,
    manual_cost_basis_fiat REAL,
    color               TEXT DEFAULT '#F7931A',
    icon                TEXT,
    notes               TEXT,
    archived            INTEGER NOT NULL DEFAULT 0,
    last_synced_at      TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX idx_wallets_custody_type ON wallets(custody_type);

-- ---------------------------------------------------------------------
-- Multisig cosigner configuration
-- ---------------------------------------------------------------------
CREATE TABLE multisig_configs (
    id              TEXT PRIMARY KEY,
    wallet_id       TEXT NOT NULL REFERENCES wallets(id) ON DELETE CASCADE,
    threshold_m     INTEGER NOT NULL,
    total_n         INTEGER NOT NULL,
    script_type     TEXT NOT NULL DEFAULT 'p2wsh' CHECK (script_type IN ('p2sh','p2sh-p2wsh','p2wsh','p2tr')),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE multisig_cosigners (
    id              TEXT PRIMARY KEY,
    multisig_id     TEXT NOT NULL REFERENCES multisig_configs(id) ON DELETE CASCADE,
    label           TEXT NOT NULL,
    xpub            TEXT NOT NULL,
    fingerprint     TEXT,
    derivation_path TEXT,
    is_mine         INTEGER NOT NULL DEFAULT 0,
    hardware_model  TEXT,
    sort_order      INTEGER NOT NULL DEFAULT 0
);

-- ---------------------------------------------------------------------
-- Addresses derived/imported for a wallet
-- ---------------------------------------------------------------------
CREATE TABLE addresses (
    id              TEXT PRIMARY KEY,
    wallet_id       TEXT NOT NULL REFERENCES wallets(id) ON DELETE CASCADE,
    address         TEXT NOT NULL,
    derivation_path TEXT,
    keychain        TEXT NOT NULL DEFAULT 'external' CHECK (keychain IN ('external','internal','n/a')),
    address_index   INTEGER,
    is_used         INTEGER NOT NULL DEFAULT 0,
    label           TEXT,
    first_seen_at   TEXT,
    last_seen_at    TEXT,
    UNIQUE(wallet_id, address)
);

CREATE INDEX idx_addresses_wallet ON addresses(wallet_id);
CREATE INDEX idx_addresses_used ON addresses(is_used);

-- ---------------------------------------------------------------------
-- UTXOs
-- ---------------------------------------------------------------------
CREATE TABLE utxos (
    id                      TEXT PRIMARY KEY,
    wallet_id               TEXT NOT NULL REFERENCES wallets(id) ON DELETE CASCADE,
    address_id              TEXT REFERENCES addresses(id) ON DELETE SET NULL,
    txid                    TEXT NOT NULL,
    vout                    INTEGER NOT NULL,
    amount_sats             INTEGER NOT NULL,
    confirmations           INTEGER NOT NULL DEFAULT 0,
    block_height            INTEGER,
    block_time              TEXT,
    is_change               INTEGER NOT NULL DEFAULT 0,
    is_frozen               INTEGER NOT NULL DEFAULT 0,
    label                   TEXT,
    acquisition_cost_fiat   REAL,
    acquisition_date        TEXT,
    privacy_risk_score      INTEGER,
    spent                   INTEGER NOT NULL DEFAULT 0,
    spent_txid              TEXT,
    spent_at                TEXT,
    created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(wallet_id, txid, vout)
);

CREATE INDEX idx_utxos_wallet ON utxos(wallet_id);
CREATE INDEX idx_utxos_spent ON utxos(spent);
CREATE INDEX idx_utxos_frozen ON utxos(is_frozen);

-- ---------------------------------------------------------------------
-- Transactions
-- ---------------------------------------------------------------------
CREATE TABLE transactions (
    id              TEXT PRIMARY KEY,
    wallet_id       TEXT NOT NULL REFERENCES wallets(id) ON DELETE CASCADE,
    txid            TEXT NOT NULL,
    fee_sats        INTEGER,
    block_height    INTEGER,
    block_time      TEXT,
    confirmations   INTEGER NOT NULL DEFAULT 0,
    direction       TEXT NOT NULL CHECK (direction IN ('incoming','outgoing','self')),
    amount_sats     INTEGER NOT NULL,
    label           TEXT,
    category        TEXT CHECK (category IN (
                        'kyc_withdrawal','kyc_deposit','internal_transfer',
                        'purchase','consolidation','payment','mining_reward',
                        'lightning_swap','other'
                    )),
    counterparty    TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(wallet_id, txid)
);

CREATE INDEX idx_transactions_wallet ON transactions(wallet_id);
CREATE INDEX idx_transactions_category ON transactions(category);

-- ---------------------------------------------------------------------
-- Free-form labels/tags (generic entity tagging, separate from the
-- single `label` convenience column on utxos/addresses/transactions)
-- ---------------------------------------------------------------------
CREATE TABLE labels (
    id          TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL CHECK (entity_type IN ('utxo','address','transaction','wallet')),
    entity_id   TEXT NOT NULL,
    label       TEXT NOT NULL,
    color       TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX idx_labels_entity ON labels(entity_type, entity_id);

-- ---------------------------------------------------------------------
-- Security configuration (per-wallet and global rows; wallet_id NULL = global)
-- ---------------------------------------------------------------------
CREATE TABLE security_config (
    id                              TEXT PRIMARY KEY,
    wallet_id                       TEXT REFERENCES wallets(id) ON DELETE CASCADE,
    hardware_wallet_model           TEXT,
    firmware_up_to_date             INTEGER,
    passphrase_enabled              INTEGER NOT NULL DEFAULT 0,
    is_multisig                     INTEGER NOT NULL DEFAULT 0,
    geographic_redundancy_locations INTEGER NOT NULL DEFAULT 0,
    backup_tested                   INTEGER NOT NULL DEFAULT 0,
    backup_tested_at                TEXT,
    exchange_2fa_type               TEXT CHECK (exchange_2fa_type IN ('none','sms','totp','hardware_key')),
    updated_at                      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE UNIQUE INDEX idx_security_config_wallet ON security_config(wallet_id);

-- ---------------------------------------------------------------------
-- Inheritance & recovery
-- ---------------------------------------------------------------------
CREATE TABLE recovery_contacts (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    relationship        TEXT,
    contact_method      TEXT,
    has_key_share       INTEGER NOT NULL DEFAULT 0,
    notes_encrypted     BLOB,
    notes_nonce         BLOB,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE inheritance_configs (
    id                      TEXT PRIMARY KEY,
    wallet_id               TEXT REFERENCES wallets(id) ON DELETE CASCADE,
    instructions_encrypted  BLOB,
    instructions_nonce      BLOB,
    last_tested_at          TEXT,
    test_status             TEXT NOT NULL DEFAULT 'never_tested' CHECK (test_status IN ('never_tested','tested_pass','tested_fail')),
    updated_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE UNIQUE INDEX idx_inheritance_wallet ON inheritance_configs(wallet_id);

CREATE TABLE inheritance_contact_links (
    id              TEXT PRIMARY KEY,
    inheritance_id  TEXT NOT NULL REFERENCES inheritance_configs(id) ON DELETE CASCADE,
    contact_id      TEXT NOT NULL REFERENCES recovery_contacts(id) ON DELETE CASCADE,
    role            TEXT DEFAULT 'beneficiary'
);

-- ---------------------------------------------------------------------
-- Health score history (recomputed continuously, snapshotted for trend)
-- ---------------------------------------------------------------------
CREATE TABLE health_score_snapshots (
    id                      TEXT PRIMARY KEY,
    taken_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    security_score          INTEGER NOT NULL,
    privacy_score           INTEGER NOT NULL,
    liquidity_score         INTEGER NOT NULL,
    inheritance_score       INTEGER NOT NULL,
    redundancy_score        INTEGER NOT NULL,
    node_health_score       INTEGER,
    lightning_health_score  INTEGER,
    tax_readiness_score     INTEGER NOT NULL,
    composite_score         INTEGER NOT NULL,
    details_json            TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_health_snapshots_taken_at ON health_score_snapshots(taken_at);

-- ---------------------------------------------------------------------
-- Node configuration (Bitcoin Core RPC / Electrum)
-- ---------------------------------------------------------------------
CREATE TABLE node_configs (
    id                      TEXT PRIMARY KEY,
    kind                    TEXT NOT NULL CHECK (kind IN ('bitcoin_core_rpc','electrum')),
    label                   TEXT NOT NULL,
    host                    TEXT NOT NULL,
    port                    INTEGER NOT NULL,
    rpc_user                TEXT,
    rpc_password_encrypted  BLOB,
    rpc_password_nonce      BLOB,
    use_tls                 INTEGER NOT NULL DEFAULT 0,
    use_tor                 INTEGER NOT NULL DEFAULT 0,
    enabled                 INTEGER NOT NULL DEFAULT 1,
    is_default              INTEGER NOT NULL DEFAULT 0,
    last_connected_at       TEXT,
    created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- ---------------------------------------------------------------------
-- Lightning channels (populated via LND/CLN detection or manual entry)
-- ---------------------------------------------------------------------
CREATE TABLE lightning_channels (
    id                  TEXT PRIMARY KEY,
    wallet_id           TEXT REFERENCES wallets(id) ON DELETE CASCADE,
    channel_id          TEXT,
    remote_alias        TEXT,
    remote_pubkey       TEXT,
    capacity_sats       INTEGER NOT NULL,
    local_balance_sats  INTEGER NOT NULL,
    remote_balance_sats INTEGER NOT NULL,
    is_active           INTEGER NOT NULL DEFAULT 1,
    is_private           INTEGER NOT NULL DEFAULT 0,
    uptime_pct          REAL,
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- ---------------------------------------------------------------------
-- User preferences (key/value)
-- ---------------------------------------------------------------------
CREATE TABLE preferences (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- ---------------------------------------------------------------------
-- App/database metadata: encryption salt + KDF params for the
-- passphrase-derived master key. Never stores the passphrase itself.
-- ---------------------------------------------------------------------
CREATE TABLE db_meta (
    key     TEXT PRIMARY KEY,
    value   TEXT NOT NULL
);

-- ---------------------------------------------------------------------
-- Local encrypted backups log
-- ---------------------------------------------------------------------
CREATE TABLE backup_log (
    id          TEXT PRIMARY KEY,
    file_path   TEXT NOT NULL,
    size_bytes  INTEGER NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('automatic','manual','export')),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
