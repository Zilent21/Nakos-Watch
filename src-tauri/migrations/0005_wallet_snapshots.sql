-- Last-known balance carried in an inheritance packet, for wallets imported
-- on a machine that can't sync yet. It is only shown while the wallet has
-- never synced and has no UTXOs of its own; a real sync always wins.
CREATE TABLE wallet_snapshots (
    wallet_id    TEXT PRIMARY KEY REFERENCES wallets(id) ON DELETE CASCADE,
    balance_sats INTEGER NOT NULL,
    utxo_count   INTEGER NOT NULL DEFAULT 0,
    as_of        TEXT NOT NULL
);
