//! Syncs a wallet's on-chain state (addresses, UTXOs, transactions) from an
//! Electrum server into the local SQLite database. Uses BDK for
//! descriptor-based wallets and a direct Electrum scripthash lookup for
//! single-address wallets. Nothing here ever touches private keys — every
//! wallet handled is watch-only by construction.

use crate::db::models::Wallet as WalletRow;
use crate::rpc::electrum::ElectrumConfig;
use bdk_electrum::BdkElectrumClient;
use bdk_wallet::chain::ChainPosition;
use bdk_wallet::{KeychainKind, Wallet as BdkWallet};
use bitcoin::{Address, Network};
use rusqlite::{params, Connection};
use thiserror::Error;
use uuid::Uuid;

const STOP_GAP: usize = 30;
const BATCH_SIZE: usize = 20;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("descriptor error: {0}")]
    Descriptor(#[from] bdk_wallet::descriptor::DescriptorError),
    #[error("cannot connect update: {0}")]
    CannotConnect(#[from] bdk_wallet::chain::local_chain::CannotConnectError),
    #[error("electrum error: {0}")]
    Electrum(#[from] crate::rpc::electrum::ElectrumError),
    #[error("electrum client error: {0}")]
    ElectrumClient(#[from] electrum_client::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("wallet is missing a descriptor to sync")]
    NoDescriptor,
    #[error("invalid address: {0}")]
    InvalidAddress(String),
}

pub struct SyncSummary {
    pub new_addresses: usize,
    pub new_utxos: usize,
    pub new_transactions: usize,
    pub balance_sats: u64,
}

pub(crate) fn parse_network(net: &str) -> Network {
    match net {
        "testnet" => Network::Testnet,
        "signet" => Network::Signet,
        "regtest" => Network::Regtest,
        _ => Network::Bitcoin,
    }
}

pub fn sync_wallet_from_electrum(
    conn: &Connection,
    wallet: &WalletRow,
    electrum_cfg: &ElectrumConfig,
) -> Result<SyncSummary, SyncError> {
    let network = parse_network(&wallet.network);

    if wallet.source_type == "address" {
        return sync_single_address(conn, wallet, electrum_cfg, network);
    }

    let external = wallet.descriptor.clone().ok_or(SyncError::NoDescriptor)?;

    let mut bdk_wallet = match &wallet.change_descriptor {
        Some(internal) => BdkWallet::create(external, internal.clone())
            .network(network)
            .create_wallet_no_persist()?,
        None => BdkWallet::create_single(external)
            .network(network)
            .create_wallet_no_persist()?,
    };

    let electrum_client = electrum_cfg.connect()?;
    let client = BdkElectrumClient::new(electrum_client);

    let request = bdk_wallet.start_full_scan().build();
    let response = client.full_scan(request, STOP_GAP, BATCH_SIZE, true)?;
    bdk_wallet.apply_update(response)?;

    write_wallet_scan(conn, wallet, &bdk_wallet, network)
}

/// Write a finished BDK scan into the database. Split from the network scan so the DB side can
/// be tested without a server.
fn write_wallet_scan(
    conn: &Connection,
    wallet: &WalletRow,
    bdk_wallet: &BdkWallet,
    network: Network,
) -> Result<SyncSummary, SyncError> {
    let tip_height = bdk_wallet.latest_checkpoint().height();
    let balance = bdk_wallet.balance();
    let balance_sats = balance.total().to_sat();

    let mut new_addresses = 0usize;
    let mut new_utxos = 0usize;
    let mut new_transactions = 0usize;

    // All DB writes for this scan commit together or not at all. The network
    // scan above has already succeeded, so a failure here rolls back cleanly
    // instead of leaving a half-updated wallet.
    let db_tx = conn.unchecked_transaction()?;
    let conn: &Connection = &db_tx;
    db_tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS scan_keep_utxo (txid TEXT NOT NULL, vout INTEGER NOT NULL);
         CREATE TEMP TABLE IF NOT EXISTS scan_keep_tx (txid TEXT NOT NULL);
         DELETE FROM scan_keep_utxo; DELETE FROM scan_keep_tx;",
    )?;

    // -- transactions ------------------------------------------------
    for tx in bdk_wallet.transactions() {
        let txid = tx.tx_node.txid.to_string();
        conn.execute("INSERT INTO scan_keep_tx (txid) VALUES (?1)", params![txid])?;
        let full_tx = tx.tx_node.tx.clone();
        let (sent, received) = bdk_wallet.sent_and_received(&full_tx);
        let (direction, amount_sats) = if sent.to_sat() > 0 && received.to_sat() > 0 {
            ("self".to_string(), received.to_sat() as i64 - sent.to_sat() as i64)
        } else if sent.to_sat() > 0 {
            ("outgoing".to_string(), -(sent.to_sat() as i64))
        } else {
            ("incoming".to_string(), received.to_sat() as i64)
        };

        let (block_height, block_time, confirmations) = match tx.chain_position {
            ChainPosition::Confirmed { anchor, .. } => (
                Some(anchor.block_id.height as i64),
                Some(anchor.confirmation_time as i64),
                (tip_height as i64 - anchor.block_id.height as i64 + 1).max(0),
            ),
            ChainPosition::Unconfirmed { .. } => (None, None, 0),
        };

        let fee_sats = bdk_wallet.calculate_fee(&full_tx).ok().map(|a| a.to_sat() as i64);
        let block_time_str = block_time.map(|t| {
            chrono::DateTime::from_timestamp(t, 0)
                .unwrap_or_default()
                .to_rfc3339()
        });

        let changed = conn.execute(
            "INSERT INTO transactions (id, wallet_id, txid, fee_sats, block_height, block_time, confirmations, direction, amount_sats)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(wallet_id, txid) DO UPDATE SET
                fee_sats = excluded.fee_sats,
                block_height = excluded.block_height,
                block_time = excluded.block_time,
                confirmations = excluded.confirmations,
                direction = excluded.direction,
                amount_sats = excluded.amount_sats",
            params![
                Uuid::new_v4().to_string(),
                wallet.id,
                txid,
                fee_sats,
                block_height,
                block_time_str,
                confirmations,
                direction,
                amount_sats,
            ],
        )?;
        if changed > 0 {
            new_transactions += 1;
        }
    }

    // -- addresses + utxos --------------------------------------------
    for output in bdk_wallet.list_output() {
        let address = Address::from_script(output.txout.script_pubkey.as_script(), network)
            .map_err(|e| SyncError::InvalidAddress(e.to_string()))?;
        let keychain_str = match output.keychain {
            KeychainKind::External => "external",
            KeychainKind::Internal => "internal",
        };

        let addr_id: String = match conn.query_row(
            "SELECT id FROM addresses WHERE wallet_id = ?1 AND address = ?2",
            params![wallet.id, address.to_string()],
            |r| r.get(0),
        ) {
            Ok(id) => {
                conn.execute(
                    "UPDATE addresses SET is_used = 1, last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
                    params![id],
                )?;
                id
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO addresses (id, wallet_id, address, derivation_path, keychain, address_index, is_used, first_seen_at, last_seen_at)
                     VALUES (?1,?2,?3,?4,?5,?6,1,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                    params![
                        id,
                        wallet.id,
                        address.to_string(),
                        format!("{}/{}", if keychain_str == "external" { 0 } else { 1 }, output.derivation_index),
                        keychain_str,
                        output.derivation_index,
                    ],
                )?;
                new_addresses += 1;
                id
            }
            Err(e) => return Err(e.into()),
        };

        // Spent outputs are not part of the live set. A stored copy is marked spent (never
        // revived by the retire step below); it is only deleted there if it holds no user data.
        if output.is_spent {
            conn.execute(
                "UPDATE utxos SET spent = 1, unseen_since = NULL WHERE wallet_id = ?1 AND txid = ?2 AND vout = ?3",
                params![wallet.id, output.outpoint.txid.to_string(), output.outpoint.vout as i64],
            )?;
            continue;
        }
        conn.execute(
            "INSERT INTO scan_keep_utxo (txid, vout) VALUES (?1, ?2)",
            params![output.outpoint.txid.to_string(), output.outpoint.vout as i64],
        )?;

        let confirmations = match output.chain_position {
            ChainPosition::Confirmed { anchor, .. } => (tip_height as i64 - anchor.block_id.height as i64 + 1).max(0),
            ChainPosition::Unconfirmed { .. } => 0,
        };
        let block_height = match output.chain_position {
            ChainPosition::Confirmed { anchor, .. } => Some(anchor.block_id.height as i64),
            ChainPosition::Unconfirmed { .. } => None,
        };

        let changed = conn.execute(
            "INSERT INTO utxos (id, wallet_id, address_id, txid, vout, amount_sats, confirmations, block_height, is_change, spent)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(wallet_id, txid, vout) DO UPDATE SET
                confirmations = excluded.confirmations,
                block_height = excluded.block_height,
                spent = excluded.spent,
                unseen_since = NULL",
            params![
                Uuid::new_v4().to_string(),
                wallet.id,
                addr_id,
                output.outpoint.txid.to_string(),
                output.outpoint.vout as i64,
                output.txout.value.to_sat() as i64,
                confirmations,
                block_height,
                (output.keychain == KeychainKind::Internal) as i64,
                0i64,
            ],
        )?;
        if changed > 0 {
            new_utxos += 1;
        }
    }

    prune_to_scan(conn, &wallet.id)?;
    db_tx.commit()?;

    Ok(SyncSummary { new_addresses, new_utxos, new_transactions, balance_sats })
}

/// Coin columns the user fills in. A row holding any of them is never deleted by sync.
const USER_DATA: &str = "(is_frozen <> 0 OR label IS NOT NULL OR acquisition_cost_fiat IS NOT NULL
                          OR acquisition_date IS NOT NULL OR acquisition_note IS NOT NULL)";

/// Retire every UTXO row of this wallet that the scan did not report as live. A row with user
/// data (freeze flag, label, cost-basis lot, note) is kept and marked not-live (`spent = 1`,
/// `unseen_since` stamped on the first miss; later misses skip it because it is already
/// `spent = 1`), so a coin a server merely omitted keeps its lot and freeze flag and is excluded
/// from balances and planners until a scan reports it again. A row with no user data is deleted.
/// Rows BDK reported as spent were already marked `spent = 1`.
fn retire_unreported_utxos(conn: &Connection, wallet_id: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        &format!(
            "UPDATE utxos SET spent = 1, unseen_since = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE wallet_id = ?1 AND spent = 0 AND {USER_DATA} AND NOT EXISTS (
                 SELECT 1 FROM scan_keep_utxo k WHERE k.txid = utxos.txid AND k.vout = utxos.vout)"
        ),
        params![wallet_id],
    )?;
    conn.execute(
        &format!(
            "DELETE FROM utxos WHERE wallet_id = ?1 AND NOT {USER_DATA} AND NOT EXISTS (
                 SELECT 1 FROM scan_keep_utxo k WHERE k.txid = utxos.txid AND k.vout = utxos.vout)"
        ),
        params![wallet_id],
    )?;
    Ok(())
}

/// Retire UTXO rows the scan did not report (see `retire_unreported_utxos`) and drop transaction
/// rows it did not report. This is what lets a fully spent wallet fall to zero. Transactions hold
/// no user-entered data today, so they are still deleted outright.
fn prune_to_scan(conn: &Connection, wallet_id: &str) -> Result<(), rusqlite::Error> {
    retire_unreported_utxos(conn, wallet_id)?;
    conn.execute(
        "DELETE FROM transactions WHERE wallet_id = ?1 AND txid NOT IN (SELECT txid FROM scan_keep_tx)",
        params![wallet_id],
    )?;
    Ok(())
}

fn sync_single_address(
    conn: &Connection,
    wallet: &WalletRow,
    electrum_cfg: &ElectrumConfig,
    network: Network,
) -> Result<SyncSummary, SyncError> {
    let raw = wallet.descriptor.clone().ok_or(SyncError::NoDescriptor)?;
    let address = crate::bitcoin::descriptors::validate_address(&raw, network)
        .map_err(|e| SyncError::InvalidAddress(e.to_string()))?;

    let client = electrum_cfg.connect()?;
    let utxos = crate::rpc::electrum::list_address_utxos(&client, &address)?;

    write_address_scan(conn, wallet, &address, &utxos)
}

/// Write a finished single-address lookup into the database (network-free, testable).
fn write_address_scan(
    conn: &Connection,
    wallet: &WalletRow,
    address: &Address,
    utxos: &[crate::rpc::electrum::ElectrumUtxo],
) -> Result<SyncSummary, SyncError> {
    let db_tx = conn.unchecked_transaction()?;
    let conn: &Connection = &db_tx;
    db_tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS scan_keep_utxo (txid TEXT NOT NULL, vout INTEGER NOT NULL);
         CREATE TEMP TABLE IF NOT EXISTS scan_keep_tx (txid TEXT NOT NULL);
         DELETE FROM scan_keep_utxo; DELETE FROM scan_keep_tx;",
    )?;

    let addr_id: String = match conn.query_row(
        "SELECT id FROM addresses WHERE wallet_id = ?1 AND address = ?2",
        params![wallet.id, address.to_string()],
        |r| r.get(0),
    ) {
        Ok(id) => id,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO addresses (id, wallet_id, address, keychain, is_used) VALUES (?1,?2,?3,'n/a',0)",
                params![id, wallet.id, address.to_string()],
            )?;
            id
        }
        Err(e) => return Err(e.into()),
    };

    let mut new_utxos = 0usize;
    let mut balance_sats = 0u64;
    for u in utxos {
        balance_sats += u.amount_sats;
        conn.execute("INSERT INTO scan_keep_utxo (txid, vout) VALUES (?1, ?2)", params![u.txid, u.vout as i64])?;
        let changed = conn.execute(
            "INSERT INTO utxos (id, wallet_id, address_id, txid, vout, amount_sats, confirmations, block_height, spent)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0)
             ON CONFLICT(wallet_id, txid, vout) DO UPDATE SET
                confirmations = excluded.confirmations,
                spent = 0,
                unseen_since = NULL",
            params![
                Uuid::new_v4().to_string(),
                wallet.id,
                addr_id,
                u.txid,
                u.vout as i64,
                u.amount_sats as i64,
                if u.height > 0 { 1 } else { 0 },
                if u.height > 0 { Some(u.height) } else { None },
            ],
        )?;
        if changed > 0 {
            new_utxos += 1;
        }
    }

    if !utxos.is_empty() {
        conn.execute(
            "UPDATE addresses SET is_used = 1, last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
            params![addr_id],
        )?;
    }

    // The address history isn't fetched on this path, so only UTXOs are
    // retired; the (empty) tx keep-list must not wipe stored transactions.
    retire_unreported_utxos(conn, &wallet.id)?;
    db_tx.commit()?;

    Ok(SyncSummary { new_addresses: 0, new_utxos, new_transactions: 0, balance_sats })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db_with_wallet() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO wallets (id,name,custody_type,source_type,network) VALUES ('w','t','cold_storage','zpub','bitcoin')",
            [],
        )
        .unwrap();
        for (i, sats) in [(0, 1_000), (1, 2_000), (2, 3_000)] {
            conn.execute(
                "INSERT INTO utxos (id,wallet_id,txid,vout,amount_sats) VALUES (?1,'w',?2,0,?3)",
                params![format!("u{i}"), format!("tx{i}"), sats],
            )
            .unwrap();
        }
        conn.execute("INSERT INTO transactions (id,wallet_id,txid,direction,amount_sats) VALUES ('t0','w','tx0','incoming',1000)", []).unwrap();
        conn.execute_batch(
            "CREATE TEMP TABLE scan_keep_utxo (txid TEXT NOT NULL, vout INTEGER NOT NULL);
             CREATE TEMP TABLE scan_keep_tx (txid TEXT NOT NULL);",
        )
        .unwrap();
        conn
    }

    fn balance(conn: &Connection) -> i64 {
        conn.query_row("SELECT COALESCE(SUM(amount_sats),0) FROM utxos WHERE wallet_id='w' AND spent=0", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn scan_that_reports_one_live_utxo_drops_the_other_two() {
        let conn = db_with_wallet();
        assert_eq!(balance(&conn), 6_000);
        conn.execute("INSERT INTO scan_keep_utxo VALUES ('tx1', 0)", []).unwrap();
        prune_to_scan(&conn, "w").unwrap();
        assert_eq!(balance(&conn), 2_000);
    }

    #[test]
    fn drained_wallet_goes_to_zero() {
        let conn = db_with_wallet();
        // Scan finds only spent outputs => nothing in the keep list.
        prune_to_scan(&conn, "w").unwrap();
        assert_eq!(balance(&conn), 0);
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM utxos WHERE wallet_id='w'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn prune_is_rolled_back_with_its_transaction() {
        let conn = db_with_wallet();
        {
            let tx = conn.unchecked_transaction().unwrap();
            prune_to_scan(&tx, "w").unwrap();
            // dropped without commit => rollback
        }
        assert_eq!(balance(&conn), 6_000);
    }
}

/// R3: a scan that does not report a coin must not destroy the user's data on it.
#[cfg(test)]
mod omitted_coin_tests {
    use super::*;
    use crate::rpc::electrum::ElectrumUtxo;
    use bitcoin::absolute::LockTime;
    use bitcoin::hashes::Hash;
    use std::str::FromStr;
    use bitcoin::transaction::Version;
    use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness};

    const TEST_XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
    const ADDRESS: &str = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";

    fn mem_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO wallets (id,name,custody_type,source_type,network,last_synced_at)
             VALUES ('w','t','cold_storage','xpub','bitcoin','2026-09-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn
    }

    fn wallet_row() -> WalletRow {
        WalletRow {
            id: "w".into(),
            name: "t".into(),
            custody_type: "cold_storage".into(),
            risk_level: "low".into(),
            source_type: "xpub".into(),
            descriptor: None,
            change_descriptor: None,
            xpub: None,
            master_fingerprint: None,
            derivation_path: None,
            network: "bitcoin".into(),
            gap_limit: 20,
            is_watch_only: true,
            manual_balance_sats: None,
            manual_cost_basis_fiat: None,
            color: None,
            icon: None,
            notes: None,
            archived: false,
            last_synced_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// Live check, not part of the normal run (`cargo test live_sync -- --ignored`): a full xpub
    /// scan and a single-address lookup through Nakos's own sync code against a public Electrum
    /// server over TLS. Uses only public test data (the BIP32 test-vector xpub and a well-known
    /// address); it shows what that server sees when someone uses a public server.
    #[test]
    #[ignore = "network: talks to electrum.blockstream.info:50002"]
    fn live_sync_against_a_public_electrum_server() {
        let cfg = ElectrumConfig { host: "electrum.blockstream.info".into(), port: 50002, use_tls: true };
        let conn = mem_db();
        let mut xpub_wallet = wallet_row();
        xpub_wallet.xpub = Some(TEST_XPUB.into());
        let d = crate::bitcoin::descriptors::descriptors_from_extended_key(TEST_XPUB, Network::Bitcoin, None).unwrap();
        xpub_wallet.descriptor = Some(d.external.to_string());
        xpub_wallet.change_descriptor = Some(d.internal.to_string());
        let started = std::time::Instant::now();
        let s = sync_wallet_from_electrum(&conn, &xpub_wallet, &cfg).expect("xpub sync over TLS");
        println!("xpub: {} addresses, {} utxos, {} txs, {} sats in {:?}", s.new_addresses, s.new_utxos, s.new_transactions, s.balance_sats, started.elapsed());
        conn.execute("INSERT INTO wallets (id,name,custody_type,source_type,network) VALUES ('a','a','cold_storage','address','bitcoin')", []).unwrap();
        let mut addr_wallet = wallet_row();
        addr_wallet.id = "a".into();
        addr_wallet.source_type = "address".into();
        addr_wallet.descriptor = Some(ADDRESS.into());
        let s = sync_wallet_from_electrum(&conn, &addr_wallet, &cfg).expect("address sync over TLS");
        println!("address: {} utxos, {} sats", s.new_utxos, s.balance_sats);
    }

    fn bdk_wallet() -> BdkWallet {
        let d = crate::bitcoin::descriptors::descriptors_from_extended_key(TEST_XPUB, Network::Bitcoin, None).unwrap();
        BdkWallet::create(d.external, d.internal).network(Network::Bitcoin).create_wallet_no_persist().unwrap()
    }

    fn tx_paying(input: OutPoint, script_pubkey: ScriptBuf, sats: u64) -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn { previous_output: input, script_sig: ScriptBuf::new(), sequence: Sequence::MAX, witness: Witness::new() }],
            output: vec![TxOut { value: Amount::from_sat(sats), script_pubkey }],
        }
    }

    /// Credit the wallet's address `index` with `sats` from an unrelated outpoint tagged `tag`.
    fn fund(w: &mut BdkWallet, index: u32, sats: u64, tag: u8) -> OutPoint {
        let prev = OutPoint { txid: Txid::from_slice(&[tag; 32]).unwrap(), vout: 0 };
        let spk = w.peek_address(KeychainKind::External, index).address.script_pubkey();
        let tx = tx_paying(prev, spk, sats);
        let outpoint = OutPoint { txid: tx.compute_txid(), vout: 0 };
        w.apply_unconfirmed_txs([(tx, 1_700_000_000u64)]);
        outpoint
    }

    fn spend(w: &mut BdkWallet, coin: OutPoint) {
        let elsewhere = Address::from_str(ADDRESS).unwrap().assume_checked().script_pubkey();
        let tx = tx_paying(coin, elsewhere, 1_000);
        w.apply_unconfirmed_txs([(tx, 1_700_000_100u64)]);
    }

    fn scan(conn: &Connection, w: &BdkWallet) {
        write_wallet_scan(conn, &wallet_row(), w, Network::Bitcoin).unwrap();
    }

    /// Everything the user can type into a coin.
    fn give_all_user_data(conn: &Connection, op: OutPoint) {
        conn.execute(
            "UPDATE utxos SET is_frozen = 1, label = 'cold stash', acquisition_cost_fiat = 123.45,
                    acquisition_date = '2024-01-02', acquisition_note = 'gift from A'
             WHERE txid = ?1 AND vout = ?2",
            params![op.txid.to_string(), op.vout as i64],
        )
        .unwrap();
    }

    fn give_label_only(conn: &Connection, op: OutPoint) {
        conn.execute(
            "UPDATE utxos SET label = 'tip jar' WHERE txid = ?1 AND vout = ?2",
            params![op.txid.to_string(), op.vout as i64],
        )
        .unwrap();
    }

    #[derive(Debug, PartialEq)]
    struct Row {
        id: String,
        spent: i64,
        unseen: bool,
        frozen: i64,
        label: Option<String>,
        cost: Option<f64>,
        date: Option<String>,
        note: Option<String>,
    }

    fn row(conn: &Connection, op: OutPoint) -> Option<Row> {
        conn.query_row(
            "SELECT id, spent, unseen_since IS NOT NULL, is_frozen, label, acquisition_cost_fiat, acquisition_date, acquisition_note
             FROM utxos WHERE wallet_id = 'w' AND txid = ?1 AND vout = ?2",
            params![op.txid.to_string(), op.vout as i64],
            |r| {
                Ok(Row {
                    id: r.get(0)?,
                    spent: r.get(1)?,
                    unseen: r.get::<_, i64>(2)? != 0,
                    frozen: r.get(3)?,
                    label: r.get(4)?,
                    cost: r.get(5)?,
                    date: r.get(6)?,
                    note: r.get(7)?,
                })
            },
        )
        .ok()
    }

    fn row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM utxos WHERE wallet_id = 'w'", [], |r| r.get(0)).unwrap()
    }

    /// What every reader of live coins reports for wallet `w`: (balance, tax total, distribution
    /// count, spend candidates, consolidation candidates).
    fn live_view(conn: &Connection) -> (i64, i64, usize, usize, usize) {
        let balance = crate::commands::balance_rows(conn, false).unwrap().iter().find(|r| r.wallet_id == "w").unwrap().balance_sats;
        let (tax_total, _) = crate::commands::tax_counts(conn).unwrap();
        let dist = crate::commands::live_amounts(conn, Some("w"), false).unwrap().len();
        let spend = crate::commands::spend_candidates(conn, "w").unwrap().len();
        let consolidate = crate::commands::consolidation_candidates(conn, "w").unwrap().len();
        (balance, tax_total, dist, spend, consolidate)
    }

    #[test]
    fn omitted_coin_keeps_its_data_stops_counting_and_comes_back_intact() {
        let conn = mem_db();
        let mut full = bdk_wallet();
        let frozen_coin = fund(&mut full, 0, 40_000, 1); // lot + label + note + frozen
        let label_coin = fund(&mut full, 1, 30_000, 2); // label only: not frozen, so only `spent` keeps it out of planners
        scan(&conn, &full);
        give_all_user_data(&conn, frozen_coin);
        give_label_only(&conn, label_coin);
        let before = row(&conn, frozen_coin).unwrap();
        assert_eq!((before.spent, before.unseen), (0, false));
        assert_eq!(live_view(&conn), (70_000, 2, 2, 1, 1), "frozen coin is counted but never spendable");

        // A scan that succeeds but reports nothing.
        scan(&conn, &bdk_wallet());

        let kept = row(&conn, frozen_coin).expect("row with user data must not be deleted");
        assert_eq!(kept.id, before.id, "same row, not re-created");
        assert_eq!((kept.frozen, kept.label.as_deref(), kept.cost, kept.date.as_deref(), kept.note.as_deref()), (1, Some("cold stash"), Some(123.45), Some("2024-01-02"), Some("gift from A")));
        assert_eq!((kept.spent, kept.unseen), (1, true), "flagged not-live and marked as unseen");
        assert!(row(&conn, label_coin).is_some());
        assert_eq!(live_view(&conn), (0, 0, 0, 0, 0), "not in balance, tax counts, distribution or either planner");

        // The server reports both coins again.
        scan(&conn, &full);

        let back = row(&conn, frozen_coin).unwrap();
        assert_eq!(back, Row { spent: 0, unseen: false, ..before }, "live again, lot/label/note/frozen untouched");
        assert_eq!(live_view(&conn), (70_000, 2, 2, 1, 1), "frozen coin is still frozen after reappearing");
    }

    #[test]
    fn a_second_miss_does_not_move_the_first_unseen_time() {
        let conn = mem_db();
        let mut full = bdk_wallet();
        let coin = fund(&mut full, 0, 40_000, 1);
        scan(&conn, &full);
        give_label_only(&conn, coin);
        scan(&conn, &bdk_wallet());
        conn.execute("UPDATE utxos SET unseen_since = '2020-01-01T00:00:00.000Z'", []).unwrap();
        scan(&conn, &bdk_wallet());
        let first: String = conn.query_row("SELECT unseen_since FROM utxos", [], |r| r.get(0)).unwrap();
        assert_eq!(first, "2020-01-01T00:00:00.000Z");
    }

    #[test]
    fn a_coin_with_no_user_data_that_is_omitted_is_still_removed() {
        let conn = mem_db();
        let mut full = bdk_wallet();
        let coin = fund(&mut full, 0, 40_000, 1);
        scan(&conn, &full);
        assert!(row(&conn, coin).is_some());
        scan(&conn, &bdk_wallet());
        assert!(row(&conn, coin).is_none());
        assert_eq!(live_view(&conn), (0, 0, 0, 0, 0));
    }

    #[test]
    fn spent_coins_stop_counting_and_a_fully_spent_wallet_falls_to_zero() {
        let conn = mem_db();
        let mut w = bdk_wallet();
        let plain = fund(&mut w, 0, 40_000, 1);
        let kept = fund(&mut w, 1, 30_000, 2);
        scan(&conn, &w);
        give_all_user_data(&conn, kept);
        assert_eq!(live_view(&conn).0, 70_000);

        spend(&mut w, plain);
        spend(&mut w, kept);
        scan(&conn, &w);

        assert_eq!(live_view(&conn), (0, 0, 0, 0, 0), "fully spent: zero in every reader");
        assert!(row(&conn, plain).is_none(), "spent coin without user data is deleted as before");
        let k = row(&conn, kept).expect("spent coin with a lot keeps its row");
        assert_eq!((k.spent, k.unseen, k.cost), (1, false, Some(123.45)), "known spent, not 'unseen'");
    }

    #[test]
    fn omitted_single_address_coin_keeps_its_data_and_returns_intact() {
        let conn = mem_db();
        conn.execute("UPDATE wallets SET source_type = 'address'", []).unwrap();
        let address = Address::from_str(ADDRESS).unwrap().assume_checked();
        let wallet = wallet_row();
        let coin = |tag: &str, vout: u32, sats: u64| ElectrumUtxo { txid: tag.repeat(64), vout, amount_sats: sats, height: 800_000 };
        let both = vec![coin("a", 0, 40_000), coin("b", 0, 30_000)];
        let op = |u: &ElectrumUtxo| OutPoint { txid: u.txid.parse().unwrap(), vout: u.vout };
        let (frozen_coin, plain_coin) = (op(&both[0]), op(&both[1]));

        write_address_scan(&conn, &wallet, &address, &both).unwrap();
        give_all_user_data(&conn, frozen_coin);
        conn.execute(
            "INSERT INTO transactions (id,wallet_id,txid,direction,amount_sats) VALUES ('t0','w','tx0','incoming',1000)",
            [],
        )
        .unwrap();
        let before = row(&conn, frozen_coin).unwrap();

        write_address_scan(&conn, &wallet, &address, &[]).unwrap();

        let kept = row(&conn, frozen_coin).expect("row with user data must not be deleted");
        assert_eq!((kept.spent, kept.unseen, kept.frozen, kept.cost, kept.note.as_deref()), (1, true, 1, Some(123.45), Some("gift from A")));
        assert!(row(&conn, plain_coin).is_none(), "no user data: removed as before");
        assert_eq!(live_view(&conn), (0, 0, 0, 0, 0));
        let txs: i64 = conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0)).unwrap();
        assert_eq!(txs, 1, "the address path never prunes transactions");

        write_address_scan(&conn, &wallet, &address, &both[..1]).unwrap();

        assert_eq!(row(&conn, frozen_coin).unwrap(), Row { spent: 0, unseen: false, ..before });
        assert_eq!(live_view(&conn).0, 40_000);
        assert_eq!(live_view(&conn).3, 0, "still frozen, so still not spendable");
    }

    /// Each user-entered column alone is enough to keep a row.
    #[test]
    fn every_user_data_column_protects_an_omitted_coin() {
        for (i, set) in [
            "is_frozen = 1",
            "label = 'x'",
            "acquisition_cost_fiat = 1.0",
            "acquisition_date = '2024-01-01'",
            "acquisition_note = 'n'",
        ]
        .iter()
        .enumerate()
        {
            let conn = mem_db();
            let mut full = bdk_wallet();
            let coin = fund(&mut full, 0, 40_000, 1);
            scan(&conn, &full);
            conn.execute(&format!("UPDATE utxos SET {set}"), []).unwrap();
            scan(&conn, &bdk_wallet());
            assert_eq!(row_count(&conn), 1, "case {i} ({set}): row deleted");
            assert_eq!(row(&conn, coin).unwrap().spent, 1, "case {i} ({set})");
        }
    }
}
