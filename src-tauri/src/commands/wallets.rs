use crate::bitcoin::descriptors;
use crate::db::models::{Cosigner, MultisigConfig, NewWallet, Wallet};
use crate::state::AppState;
use rusqlite::{params, Row};
use uuid::Uuid;

/// Watch-only means no extended private keys on disk. Every wallet write path
/// calls this on the descriptor, change descriptor, xpub and cosigner xpub
/// strings before anything is written. The prefix list lives in
/// `crypto::backup` (the same one that guards backup sealing and restore).
pub(crate) fn refuse_private_key_material<'a>(fields: impl IntoIterator<Item = Option<&'a str>>) -> Result<(), String> {
    for f in fields.into_iter().flatten() {
        if crate::crypto::backup::contains_extended_private_key(f.as_bytes()) {
            return Err("private key material is not stored".into());
        }
        if looks_like_wif(f) {
            return Err("this looks like a single private key (WIF). Nakos never takes one: use the address or a descriptor instead".into());
        }
        if looks_like_seed_phrase(f) {
            return Err("this looks like a seed phrase. Nakos never takes one: export a descriptor or xpub from your wallet instead, and keep the seed offline".into());
        }
    }
    Ok(())
}

/// A private key in Wallet Import Format: base58, 51 characters starting with 5 or 9, or 52
/// starting with K, L or c. No address or extended key has that exact shape.
pub(crate) fn looks_like_wif(text: &str) -> bool {
    let t = text.trim();
    let base58 = t.chars().all(|c| c.is_ascii_alphanumeric() && !"0OIl".contains(c));
    base58 && matches!((t.len(), t.chars().next()), (51, Some('5' | '9')) | (52, Some('K' | 'L' | 'c')))
}

/// 12, 15, 18, 21 or 24 short words of letters only and nothing else: the shape of a BIP-39 seed phrase.
/// Descriptors, xpubs and addresses never look like this, so a match is refused before it is
/// stored or sent anywhere. (No word list is needed: the shape alone is never a public key.)
pub(crate) fn looks_like_seed_phrase(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    matches!(words.len(), 12 | 15 | 18 | 21 | 24)
        && words.iter().all(|w| (3..=8).contains(&w.len()) && w.chars().all(|c| c.is_ascii_alphabetic()))
}

pub(crate) fn row_to_wallet(row: &Row) -> rusqlite::Result<Wallet> {
    Ok(Wallet {
        id: row.get("id")?,
        name: row.get("name")?,
        custody_type: row.get("custody_type")?,
        risk_level: row.get("risk_level")?,
        source_type: row.get("source_type")?,
        descriptor: row.get("descriptor")?,
        change_descriptor: row.get("change_descriptor")?,
        xpub: row.get("xpub")?,
        master_fingerprint: row.get("master_fingerprint")?,
        derivation_path: row.get("derivation_path")?,
        network: row.get("network")?,
        gap_limit: row.get("gap_limit")?,
        is_watch_only: row.get::<_, i64>("is_watch_only")? != 0,
        manual_balance_sats: row.get("manual_balance_sats")?,
        manual_cost_basis_fiat: row.get("manual_cost_basis_fiat")?,
        color: row.get("color")?,
        icon: row.get("icon")?,
        notes: row.get("notes")?,
        archived: row.get::<_, i64>("archived")? != 0,
        last_synced_at: row.get("last_synced_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[tauri::command]
pub fn detect_source_type(input: String) -> String {
    descriptors::sniff_source_type(&input).to_string()
}

#[tauri::command]
pub fn list_wallets(state: tauri::State<AppState>, include_archived: Option<bool>) -> Result<Vec<Wallet>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let include_archived = include_archived.unwrap_or(false);
    let sql = if include_archived {
        "SELECT * FROM wallets ORDER BY created_at ASC"
    } else {
        "SELECT * FROM wallets WHERE archived = 0 ORDER BY created_at ASC"
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let wallets = stmt
        .query_map([], row_to_wallet)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(wallets)
}

fn parse_network(net: &str) -> bitcoin::Network {
    match net {
        "testnet" => bitcoin::Network::Testnet,
        "signet" => bitcoin::Network::Signet,
        "regtest" => bitcoin::Network::Regtest,
        _ => bitcoin::Network::Bitcoin,
    }
}

#[tauri::command]
pub fn add_wallet(state: tauri::State<AppState>, input: NewWallet) -> Result<Wallet, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    insert_wallet(&conn, input)
}

/// The Add Wallet path, shared with inheritance import. Watch-only: takes
/// public material only.
pub(crate) fn insert_wallet(conn: &rusqlite::Connection, input: NewWallet) -> Result<Wallet, String> {
    refuse_private_key_material([
        input.descriptor.as_deref(),
        input.change_descriptor.as_deref(),
        input.xpub.as_deref(),
        // No edit path exists for these two, and only the xpub branch validates them, so a key-like
        // string here would block encrypted backups for good.
        input.master_fingerprint.as_deref(),
        input.derivation_path.as_deref(),
    ])?;
    let network = parse_network(&input.network);

    // Master fingerprint + derivation path are optional (a portfolio-
    // tracking-only wallet doesn't need them), but if supplied they must
    // come as a pair — Nakos never invents one half of an origin. Only
    // meaningful for the xpub/ypub/zpub path; persisted as-is either way.
    let fp = input.master_fingerprint.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let path = input.derivation_path.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let origin = match (fp, path) {
        (Some(fp), Some(path)) => Some((fp, path)),
        (None, None) => None,
        _ => return Err("master fingerprint and derivation path must be provided together, or not at all".into()),
    };

    let (descriptor, change_descriptor) = match input.source_type.as_str() {
        "xpub" | "ypub" | "zpub" => {
            let raw = input.xpub.clone().ok_or("xpub value required")?;
            let pair = descriptors::descriptors_from_extended_key(&raw, network, origin).map_err(|e| e.to_string())?;
            (Some(pair.external), Some(pair.internal))
        }
        "descriptor" => {
            // Stored exactly as pasted — it already carries its own
            // [fingerprint/path], so it is never wrapped again.
            let d = input.descriptor.clone().ok_or("descriptor value required")?;
            // A `<0;1>` descriptor (Sparrow's export) holds both branches: store them apart.
            match descriptors::split_multipath(&d).map_err(|e| e.to_string())? {
                Some((receive, change)) => (Some(receive), Some(change)),
                None => {
                    let change = input.change_descriptor.clone().or_else(|| descriptors::change_from_receive(&d));
                    (Some(d), change)
                }
            }
        }
        "address" => {
            let addr = input.descriptor.clone().ok_or("address value required")?;
            descriptors::validate_address(&addr, network).map_err(|e| e.to_string())?;
            (Some(addr), None)
        }
        "manual" => (None, None),
        other => return Err(format!("unsupported source_type: {other}")),
    };

    let id = Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO wallets (
            id, name, custody_type, risk_level, source_type, descriptor,
            change_descriptor, xpub, master_fingerprint, derivation_path, network, manual_balance_sats,
            manual_cost_basis_fiat, color, notes
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            id,
            input.name,
            input.custody_type,
            input.risk_level,
            input.source_type,
            descriptor,
            change_descriptor,
            input.xpub,
            fp,
            path,
            input.network,
            input.manual_balance_sats,
            input.manual_cost_basis_fiat,
            input.color,
            input.notes,
        ],
    )
    .map_err(|e| e.to_string())?;

    // Auto-create a blank security_config row for this wallet.
    conn.execute(
        "INSERT INTO security_config (id, wallet_id, is_multisig) VALUES (?1, ?2, 0)",
        params![Uuid::new_v4().to_string(), id],
    )
    .map_err(|e| e.to_string())?;

    if input.source_type == "address" {
        if let Some(addr) = &descriptor {
            conn.execute(
                "INSERT INTO addresses (id, wallet_id, address, keychain, is_used) VALUES (?1,?2,?3,'n/a',0)",
                params![Uuid::new_v4().to_string(), id, addr],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    get_wallet(conn, &id)
}

#[tauri::command]
pub fn add_multisig_wallet(
    state: tauri::State<AppState>,
    name: String,
    custody_type: String,
    risk_level: String,
    network: String,
    threshold_m: u8,
    cosigners: Vec<Cosigner>,
    script_type: String,
) -> Result<Wallet, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    insert_multisig_wallet(&conn, name, custody_type, risk_level, network, threshold_m, &cosigners, &script_type)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_multisig_wallet(
    conn: &rusqlite::Connection,
    name: String,
    custody_type: String,
    risk_level: String,
    network: String,
    threshold_m: u8,
    cosigners: &[Cosigner],
    script_type: &str,
) -> Result<Wallet, String> {
    // Cosigner rows can never be edited or deleted afterwards, so every text field is checked, not
    // only the key: a key-like label would block encrypted backups for good.
    refuse_private_key_material(cosigners.iter().flat_map(|c| {
        [Some(c.xpub.as_str()), Some(c.label.as_str()), c.hardware_model.as_deref(), c.fingerprint.as_deref(), c.derivation_path.as_deref()]
    }))?;
    let net = parse_network(&network);
    // Validates (and rejects, per-cosigner) that every cosigner has a
    // master fingerprint + derivation path before anything is written —
    // a multisig wallet whose PSBTs can never be signed isn't saved.
    let pair = descriptors::descriptors_from_multisig(threshold_m, cosigners, net, script_type)
        .map_err(|e| e.to_string())?;

    let wallet_id = Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO wallets (id, name, custody_type, risk_level, source_type, descriptor, change_descriptor, network)
         VALUES (?1,?2,?3,?4,'multisig_descriptor',?5,?6,?7)",
        params![wallet_id, name, custody_type, risk_level, pair.external, pair.internal, network],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO security_config (id, wallet_id, is_multisig) VALUES (?1, ?2, 1)",
        params![Uuid::new_v4().to_string(), wallet_id],
    )
    .map_err(|e| e.to_string())?;

    let multisig_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO multisig_configs (id, wallet_id, threshold_m, total_n, script_type) VALUES (?1,?2,?3,?4,?5)",
        params![multisig_id, wallet_id, threshold_m as i64, cosigners.len() as i64, script_type],
    )
    .map_err(|e| e.to_string())?;

    for (i, c) in cosigners.iter().enumerate() {
        conn.execute(
            "INSERT INTO multisig_cosigners (id, multisig_id, label, xpub, fingerprint, derivation_path, is_mine, hardware_model, sort_order)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                Uuid::new_v4().to_string(),
                multisig_id,
                c.label,
                c.xpub,
                c.fingerprint,
                c.derivation_path,
                c.is_mine as i64,
                c.hardware_model,
                i as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    get_wallet(conn, &wallet_id)
}

#[tauri::command]
pub fn get_multisig_config(state: tauri::State<AppState>, wallet_id: String) -> Result<Option<MultisigConfig>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT id, wallet_id, threshold_m, total_n, script_type FROM multisig_configs WHERE wallet_id = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params![wallet_id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let multisig_id: String = row.get(0).map_err(|e| e.to_string())?;
    let mut config = MultisigConfig {
        id: multisig_id.clone(),
        wallet_id: row.get(1).map_err(|e| e.to_string())?,
        threshold_m: row.get(2).map_err(|e| e.to_string())?,
        total_n: row.get(3).map_err(|e| e.to_string())?,
        script_type: row.get(4).map_err(|e| e.to_string())?,
        cosigners: Vec::new(),
    };

    let mut cstmt = conn
        .prepare("SELECT id, label, xpub, fingerprint, derivation_path, is_mine, hardware_model FROM multisig_cosigners WHERE multisig_id = ?1 ORDER BY sort_order ASC")
        .map_err(|e| e.to_string())?;
    let cosigners = cstmt
        .query_map(params![multisig_id], |r| {
            Ok(Cosigner {
                id: r.get(0)?,
                label: r.get(1)?,
                xpub: r.get(2)?,
                fingerprint: r.get(3)?,
                derivation_path: r.get(4)?,
                is_mine: r.get::<_, i64>(5)? != 0,
                hardware_model: r.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    config.cosigners = cosigners;

    Ok(Some(config))
}

pub(crate) fn get_wallet(conn: &rusqlite::Connection, id: &str) -> Result<Wallet, String> {
    conn.query_row("SELECT * FROM wallets WHERE id = ?1", params![id], row_to_wallet)
        .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Deserialize)]
pub struct WalletPatch {
    pub name: Option<String>,
    pub custody_type: Option<String>,
    pub risk_level: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub manual_balance_sats: Option<i64>,
    pub manual_cost_basis_fiat: Option<f64>,
}

#[tauri::command]
pub fn update_wallet(state: tauri::State<AppState>, id: String, patch: WalletPatch) -> Result<Wallet, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    macro_rules! apply {
        ($field:ident, $col:literal) => {
            if let Some(v) = &patch.$field {
                conn.execute(
                    concat!("UPDATE wallets SET ", $col, " = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2"),
                    params![v, id],
                )
                .map_err(|e| e.to_string())?;
            }
        };
    }
    apply!(name, "name");
    apply!(custody_type, "custody_type");
    apply!(risk_level, "risk_level");
    apply!(color, "color");
    apply!(notes, "notes");
    apply!(manual_balance_sats, "manual_balance_sats");
    apply!(manual_cost_basis_fiat, "manual_cost_basis_fiat");

    get_wallet(&conn, &id)
}

#[tauri::command]
pub fn archive_wallet(state: tauri::State<AppState>, id: String, archived: bool) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE wallets SET archived = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        params![archived as i64, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct SyncResult {
    pub wallet_id: String,
    pub new_addresses: usize,
    pub new_utxos: usize,
    pub new_transactions: usize,
    pub balance_sats: u64,
}

/// Async so a sync, which waits on the Electrum server for seconds, never freezes the window:
/// the work runs on a blocking-pool thread, one sync at a time.
#[tauri::command]
pub async fn sync_wallet(state: tauri::State<'_, AppState>, id: String) -> Result<SyncResult, String> {
    let pool = state.pool()?;
    sync_wallet_in_background(pool, state.sync_lock(), id).await
}

async fn sync_wallet_in_background(pool: crate::db::DbPool, lock: std::sync::Arc<parking_lot::Mutex<()>>, id: String) -> Result<SyncResult, String> {
    tokio::task::spawn_blocking(move || {
        let _one_at_a_time = lock.lock();
        sync_wallet_on(&pool, id)
    })
    .await
    .map_err(|e| format!("the sync stopped unexpectedly: {e}"))?
}

fn sync_wallet_on(pool: &crate::db::DbPool, id: String) -> Result<SyncResult, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let wallet = get_wallet(&conn, &id)?;

    let node_cfg = crate::commands::node::default_electrum_config(&conn)?;
    let Some(electrum_cfg) = node_cfg else {
        return Err("No Electrum server set up yet. Add your own, or a public one, on the Node page.".to_string());
    };
    // Someone else's server sees every address it is asked about: only after the warning.
    crate::commands::node::public_server_gate(&conn, &electrum_cfg)?;

    let result = crate::bitcoin::sync::sync_wallet_from_electrum(&conn, &wallet, &electrum_cfg).map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE wallets SET last_synced_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    // A failed alert check never fails the sync that just succeeded.
    if let Err(e) = crate::commands::alerts::evaluate(&conn) {
        tracing::warn!("alert check after sync failed: {e}");
    }

    Ok(SyncResult {
        wallet_id: id,
        new_addresses: result.new_addresses,
        new_utxos: result.new_utxos,
        new_transactions: result.new_transactions,
        balance_sats: result.balance_sats,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct CustodyBreakdown {
    pub custody_type: String,
    pub total_sats: i64,
    pub wallet_count: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct RiskBreakdown {
    pub risk_level: String,
    pub total_sats: i64,
    pub wallet_count: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct PortfolioSummary {
    pub total_sats: i64,
    pub wallet_count: i64,
    pub by_custody_type: Vec<CustodyBreakdown>,
    pub by_risk_level: Vec<RiskBreakdown>,
}

/// One wallet's balance in sats. Derived UTXOs (unspent) for descriptor /
/// address / multisig wallets; `manual_balance_sats` for manual-entry wallets
/// (exchange / ETF / retirement). A wallet never synced here with no coins of
/// its own falls back to the last-known balance from an imported inheritance
/// packet; a real sync always wins. Returns `(sats, is_snapshot)`.
/// The most sats that can exist: 21 million BTC. A balance read from a file or the database that
/// falls outside `0..=MAX_SATS` is not a balance. It is also below 2^53, so it survives the
/// round trip through JavaScript numbers, and a few thousand of them cannot overflow an `i64` sum.
pub(crate) const MAX_SATS: i64 = 2_100_000_000_000_000;

pub(crate) fn plausible_sats(sats: i64) -> bool {
    (0..=MAX_SATS).contains(&sats)
}

pub(crate) fn wallet_balance_sats(
    source_type: &str,
    manual_balance_sats: Option<i64>,
    utxo_sats: i64,
    last_synced_at: Option<&str>,
    snapshot_sats: Option<i64>,
) -> (i64, bool) {
    if source_type == "manual" {
        (manual_balance_sats.unwrap_or(0), false)
    } else if utxo_sats == 0 && last_synced_at.is_none() {
        match snapshot_sats {
            // An implausible snapshot (an older import of a bad packet) is not shown or summed.
            Some(s) if plausible_sats(s) => (s, true),
            _ => (0, false),
        }
    } else {
        (utxo_sats, false)
    }
}

pub(crate) struct BalanceRow {
    pub wallet_id: String,
    pub custody_type: String,
    pub risk_level: String,
    pub balance_sats: i64,
    pub is_snapshot: bool,
}

pub(crate) fn balance_rows(conn: &rusqlite::Connection, include_archived: bool) -> Result<Vec<BalanceRow>, String> {
    let sql = format!(
        "SELECT w.id, w.custody_type, w.risk_level, w.source_type, w.manual_balance_sats,
                COALESCE((SELECT SUM(amount_sats) FROM utxos u WHERE u.wallet_id = w.id AND u.spent = 0), 0) AS utxo_sats,
                w.last_synced_at,
                (SELECT balance_sats FROM wallet_snapshots s WHERE s.wallet_id = w.id) AS snapshot_sats
         FROM wallets w {} ORDER BY w.created_at ASC",
        if include_archived { "" } else { "WHERE w.archived = 0" }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let source_type: String = r.get(3)?;
            let last_synced: Option<String> = r.get(6)?;
            let (balance_sats, is_snapshot) =
                wallet_balance_sats(&source_type, r.get(4)?, r.get(5)?, last_synced.as_deref(), r.get(7)?);
            Ok(BalanceRow { wallet_id: r.get(0)?, custody_type: r.get(1)?, risk_level: r.get(2)?, balance_sats, is_snapshot })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[derive(Debug, serde::Serialize)]
pub struct WalletBalance {
    pub wallet_id: String,
    pub balance_sats: i64,
    /// True when the figure is a last-known balance from an imported packet.
    pub is_snapshot: bool,
}

/// Per-wallet balances for the Portfolio table. Includes archived wallets
/// only when asked; they never count toward the summary total.
#[tauri::command]
pub fn list_wallet_balances(state: tauri::State<AppState>, include_archived: Option<bool>) -> Result<Vec<WalletBalance>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    Ok(balance_rows(&conn, include_archived.unwrap_or(false))?
        .into_iter()
        .map(|r| WalletBalance { wallet_id: r.wallet_id, balance_sats: r.balance_sats, is_snapshot: r.is_snapshot })
        .collect())
}

#[tauri::command]
pub fn portfolio_summary(state: tauri::State<AppState>) -> Result<PortfolioSummary, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    // The headline total and the per-wallet column both come from
    // `balance_rows`, so they can't disagree. Archived wallets are excluded.
    let rows = balance_rows(&conn, false)?;
    portfolio_from_rows(&rows)
}

/// Sums are checked: a total that does not fit is an error, never a panic (debug) or a negative
/// number (release, which does not check overflow).
fn portfolio_from_rows(rows: &[BalanceRow]) -> Result<PortfolioSummary, String> {
    const TOO_BIG: &str = "wallet balances are too large to add up";
    let mut total = 0i64;
    let mut by_custody: std::collections::BTreeMap<String, (i64, i64)> = Default::default();
    let mut by_risk: std::collections::BTreeMap<String, (i64, i64)> = Default::default();

    for row in rows {
        let sats = row.balance_sats;
        total = total.checked_add(sats).ok_or(TOO_BIG)?;
        let c = by_custody.entry(row.custody_type.clone()).or_insert((0, 0));
        c.0 = c.0.checked_add(sats).ok_or(TOO_BIG)?;
        c.1 += 1;
        let r = by_risk.entry(row.risk_level.clone()).or_insert((0, 0));
        r.0 = r.0.checked_add(sats).ok_or(TOO_BIG)?;
        r.1 += 1;
    }

    Ok(PortfolioSummary {
        total_sats: total,
        wallet_count: rows.len() as i64,
        by_custody_type: by_custody
            .into_iter()
            .map(|(k, (sats, count))| CustodyBreakdown { custody_type: k, total_sats: sats, wallet_count: count })
            .collect(),
        by_risk_level: by_risk
            .into_iter()
            .map(|(k, (sats, count))| RiskBreakdown { risk_level: k, total_sats: sats, wallet_count: count })
            .collect(),
    })
}

#[derive(Debug, serde::Serialize)]
pub struct SnapshotWallet {
    pub wallet_id: String,
    pub name: String,
    pub balance_sats: i64,
    pub as_of: String,
}

/// Unarchived wallets whose balance is currently a packet snapshot (never
/// synced here, no coins of their own). Lets the UI say so honestly.
#[tauri::command]
pub fn snapshot_wallets(state: tauri::State<AppState>) -> Result<Vec<SnapshotWallet>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    snapshot_wallet_rows(&conn)
}

/// Same rule as `wallet_balance_sats`: a snapshot outside `0..=MAX_SATS` is not listed.
fn snapshot_wallet_rows(conn: &rusqlite::Connection) -> Result<Vec<SnapshotWallet>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT w.id, w.name, s.balance_sats, s.as_of FROM wallets w
             JOIN wallet_snapshots s ON s.wallet_id = w.id
             WHERE w.archived = 0 AND w.last_synced_at IS NULL
               AND s.balance_sats BETWEEN 0 AND ?1
               AND NOT EXISTS (SELECT 1 FROM utxos u WHERE u.wallet_id = w.id AND u.spent = 0)
             ORDER BY w.created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![MAX_SATS], |r| Ok(SnapshotWallet { wallet_id: r.get(0)?, name: r.get(1)?, balance_sats: r.get(2)?, as_of: r.get(3)? }))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[cfg(test)]
mod balance_tests {
    use super::*;

    fn mem_db() -> rusqlite::Connection {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn
    }

    fn wallet(conn: &rusqlite::Connection, id: &str, source: &str, archived: bool, manual: Option<i64>, synced: bool) {
        conn.execute(
            "INSERT INTO wallets (id, name, custody_type, source_type, archived, manual_balance_sats, last_synced_at)
             VALUES (?1, ?1, 'cold_storage', ?2, ?3, ?4, ?5)",
            params![id, source, archived as i64, manual, synced.then_some("2026-09-01T00:00:00Z")],
        )
        .unwrap();
    }

    fn utxo(conn: &rusqlite::Connection, id: &str, wallet: &str, vout: i64, sats: i64, spent: bool) {
        conn.execute(
            "INSERT INTO utxos (id, wallet_id, txid, vout, amount_sats, spent) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, wallet, "ef".repeat(32), vout, sats, spent as i64],
        )
        .unwrap();
    }

    fn sats_of(rows: &[BalanceRow], id: &str) -> i64 {
        rows.iter().find(|r| r.wallet_id == id).unwrap().balance_sats
    }

    fn snapshot(conn: &rusqlite::Connection, wallet: &str, sats: i64) {
        conn.execute(
            "INSERT INTO wallet_snapshots (wallet_id, balance_sats, utxo_count, as_of) VALUES (?1, ?2, 1, '2026-09-01T00:00:00Z')",
            params![wallet, sats],
        )
        .unwrap();
    }

    /// R7: a packet's snapshot comes from a file anyone can edit. Rows that are not a possible
    /// amount (from an older import) are not shown and not summed; a possible one still is.
    #[test]
    fn implausible_snapshot_rows_are_ignored_by_balances_totals_and_the_snapshot_list() {
        let conn = mem_db();
        for (id, sats) in [("huge1", i64::MAX), ("huge2", i64::MAX), ("negative", -5), ("over", MAX_SATS + 1), ("edge", MAX_SATS), ("normal", 777_000)] {
            wallet(&conn, id, "xpub", false, None, false);
            snapshot(&conn, id, sats);
        }
        let rows = balance_rows(&conn, false).unwrap();
        for id in ["huge1", "huge2", "negative", "over"] {
            assert_eq!(sats_of(&rows, id), 0, "{id}");
            assert!(!rows.iter().find(|r| r.wallet_id == id).unwrap().is_snapshot, "{id}");
        }
        assert_eq!(sats_of(&rows, "edge"), MAX_SATS);
        assert_eq!(sats_of(&rows, "normal"), 777_000);
        let summary = portfolio_from_rows(&rows).unwrap();
        assert_eq!(summary.total_sats, MAX_SATS + 777_000);
        let listed: Vec<String> = snapshot_wallet_rows(&conn).unwrap().into_iter().map(|w| w.wallet_id).collect();
        assert_eq!(listed.len(), 2);
        assert!(listed.contains(&"edge".to_string()) && listed.contains(&"normal".to_string()), "{listed:?}");
    }

    /// Whatever the source of the numbers, a sum that does not fit is an error, not a panic
    /// (debug) and not a negative total (release does not check overflow).
    #[test]
    fn a_portfolio_sum_that_does_not_fit_is_an_error_not_a_panic() {
        let row = |id: &str, custody: &str, risk: &str, sats: i64| BalanceRow {
            wallet_id: id.into(),
            custody_type: custody.into(),
            risk_level: risk.into(),
            balance_sats: sats,
            is_snapshot: false,
        };
        // Overflows the headline total.
        let err = portfolio_from_rows(&[row("a", "cold", "low", i64::MAX), row("b", "hot", "high", 1)]).err().unwrap();
        assert!(err.contains("too large"), "{err}");
        // Overflows only inside one custody group and one risk group (the total stays in range).
        let err = portfolio_from_rows(&[row("b", "hot", "high", -100), row("a", "cold", "low", i64::MAX), row("c", "cold", "low", 50)]).err().unwrap();
        assert!(err.contains("too large"), "{err}");
        // Overflows only inside one risk group: custody groups and the total all fit.
        let err = portfolio_from_rows(&[row("c", "warm", "high", -100), row("a", "cold", "low", i64::MAX), row("b", "hot", "low", 1)]).err().unwrap();
        assert!(err.contains("too large"), "{err}");
        // Ordinary numbers are untouched.
        let ok = portfolio_from_rows(&[row("a", "cold", "low", 5), row("b", "hot", "low", 7)]).unwrap();
        assert_eq!((ok.total_sats, ok.wallet_count), (12, 2));
    }

    #[test]
    fn per_wallet_balances_sum_to_the_unarchived_total() {
        let conn = mem_db();
        wallet(&conn, "test5", "xpub", false, None, true);
        utxo(&conn, "a", "test5", 0, 6_010, false);
        utxo(&conn, "b", "test5", 1, 8_406, false);
        utxo(&conn, "spent", "test5", 2, 99_999, true); // spent coins never count
        wallet(&conn, "test4", "xpub", false, None, true);
        utxo(&conn, "c", "test4", 0, 5_000, false);
        wallet(&conn, "empty", "xpub", false, None, true); // no coins: 0
        wallet(&conn, "old", "xpub", true, None, true);
        utxo(&conn, "d", "old", 0, 123_456, false);
        wallet(&conn, "exch", "manual", false, Some(2_000_000), false);

        let live = balance_rows(&conn, false).unwrap();
        assert_eq!(sats_of(&live, "test5"), 14_416);
        assert_eq!(sats_of(&live, "test4"), 5_000);
        assert_eq!(sats_of(&live, "empty"), 0);
        assert_eq!(sats_of(&live, "exch"), 2_000_000);
        assert!(live.iter().all(|r| r.wallet_id != "old"), "archived wallets are not in the default list or the total");
        let total: i64 = live.iter().map(|r| r.balance_sats).sum();
        assert_eq!(total, 14_416 + 5_000 + 2_000_000, "the headline total is exactly the sum of the rows");

        // With archived shown, the row exists (with its own balance) but the total basis is unchanged.
        let all = balance_rows(&conn, true).unwrap();
        assert_eq!(sats_of(&all, "old"), 123_456);
        assert_eq!(all.iter().filter(|r| r.wallet_id != "old").map(|r| r.balance_sats).sum::<i64>(), total);
    }

    #[test]
    fn a_never_synced_wallet_shows_its_packet_snapshot_until_a_real_sync() {
        assert_eq!(wallet_balance_sats("xpub", None, 0, None, Some(777)), (777, true));
        assert_eq!(wallet_balance_sats("xpub", None, 0, Some("2026-09-01"), Some(777)), (0, false), "synced: the snapshot no longer applies");
        assert_eq!(wallet_balance_sats("xpub", None, 500, None, Some(777)), (500, false), "real coins win");
        assert_eq!(wallet_balance_sats("xpub", None, 0, None, None), (0, false));
        assert_eq!(wallet_balance_sats("manual", Some(42), 0, None, Some(777)), (42, false));
    }
}

#[cfg(test)]
mod private_key_tests {
    use super::*;

    /// A vault with one single-address wallet whose Electrum server is a loopback listener that
    /// reads the request, waits `delay_ms` and hangs up (so the sync ends in an error after that).
    fn wallet_with_slow_electrum(tag: &str, delay_ms: u64) -> (crate::test_dir::TestDir, crate::db::DbPool) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut s = stream.unwrap();
                std::thread::spawn(move || {
                    use std::io::Read;
                    let mut buf = [0u8; 1024];
                    let _ = s.read(&mut buf);
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                });
            }
        });
        let dir = crate::test_dir::TestDir::new(tag);
        let pool = crate::db::open_pool(&dir).unwrap();
        let conn = pool.get().unwrap();
        conn.execute("INSERT INTO wallets (id, name, custody_type, source_type, descriptor) VALUES ('w', 'Spending', 'hot_wallet', 'address', 'bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq')", []).unwrap();
        conn.execute("INSERT INTO node_configs (id, kind, label, host, port, use_tls) VALUES ('e', 'electrum', 'mine', '127.0.0.1', ?1, 0)", params![port]).unwrap();
        (dir, pool)
    }

    /// A slow Electrum server must not hold the async runtime: on a runtime with ONE thread, a
    /// task that needs ~50 ms still finishes long before the sync does.
    #[test]
    fn a_slow_sync_does_not_hold_the_async_runtime() {
        let (_dir, pool) = wallet_with_slow_electrum("sync-async", 1500);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (result, quick_after, total) = rt.block_on(async {
            let start = std::time::Instant::now();
            let quick = async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                start.elapsed()
            };
            let (r, q) = tokio::join!(sync_wallet_in_background(pool, Default::default(), "w".into()), quick);
            (r, q, start.elapsed())
        });
        assert!(result.is_err(), "the server hangs up, so the sync reports an error");
        assert!(total >= std::time::Duration::from_millis(1400), "the server was supposed to be slow: {total:?}");
        assert!(quick_after < std::time::Duration::from_millis(600), "the runtime thread was held for {quick_after:?}");
    }

    /// Syncs run one at a time: while another sync holds the lock, a new one waits (in the
    /// background) and goes on as soon as the lock is free.
    #[test]
    fn syncs_run_one_at_a_time() {
        // No Electrum server: an unblocked sync ends at once with "No Electrum server set up yet".
        let dir = crate::test_dir::TestDir::new("sync-lock");
        let pool = crate::db::open_pool(&dir).unwrap();
        pool.get().unwrap().execute("INSERT INTO wallets (id, name, custody_type, source_type, descriptor) VALUES ('w', 'Spending', 'hot_wallet', 'address', 'bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq')", []).unwrap();
        let lock: std::sync::Arc<parking_lot::Mutex<()>> = Default::default();
        let held = lock.lock();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let task = rt.spawn(sync_wallet_in_background(pool, lock.clone(), "w".into()));
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(!task.is_finished(), "a second sync waits while one is running");
        drop(held);
        let started = std::time::Instant::now();
        let result = rt.block_on(task).unwrap();
        assert!(result.unwrap_err().contains("No Electrum server set up yet"), "it then ran");
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "and went on as soon as the lock was free");
        // Every sync command gets the same lock from the app state.
        let state = AppState::default();
        assert!(std::sync::Arc::ptr_eq(&state.sync_lock(), &state.sync_lock()));
    }

    /// Sync asks someone else's Electrum server nothing until the privacy warning was accepted
    /// for it, and goes straight on to a server on the person's own network.
    #[test]
    fn sync_waits_for_the_privacy_warning_before_asking_someone_elses_server() {
        let dir = crate::test_dir::TestDir::new("sync-gate");
        let pool = crate::db::open_pool(&dir).unwrap();
        let conn = pool.get().unwrap();
        conn.execute("INSERT INTO wallets (id, name, custody_type, source_type, descriptor) VALUES ('w', 'Spending', 'hot_wallet', 'address', 'bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq')", []).unwrap();

        let none = sync_wallet_on(&pool, "w".into()).unwrap_err();
        assert!(none.contains("No Electrum server set up yet"), "{none}");

        // 203.0.113.0/24 is a documentation range: nothing answers, so reaching it would hang.
        conn.execute("INSERT INTO node_configs (id, kind, label, host, port, use_tls) VALUES ('e', 'electrum', 'public', '203.0.113.7', 50002, 1)", []).unwrap();
        let started = std::time::Instant::now();
        let gated = sync_wallet_on(&pool, "w".into()).unwrap_err();
        assert_eq!(gated, "PUBLIC_SERVER_NOT_ACKNOWLEDGED|203.0.113.7|50002");
        assert!(started.elapsed() < std::time::Duration::from_secs(1), "refused before any connection attempt");

        // Own network: no warning; the sync goes on and fails only because nothing listens.
        conn.execute("UPDATE node_configs SET host = '127.0.0.1', port = 1, use_tls = 0 WHERE id = 'e'", []).unwrap();
        let own = sync_wallet_on(&pool, "w".into()).unwrap_err();
        assert!(!own.starts_with("PUBLIC_SERVER_NOT_ACKNOWLEDGED"), "{own}");
    }

    // Published BIP-32 test vector 1 (public test data, not a real wallet).
    const XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
    const XPRV: &str = "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi";
    const REFUSAL: &str = "private key material is not stored";

    fn mem_db() -> rusqlite::Connection {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn
    }

    fn count(conn: &rusqlite::Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    fn new_wallet(source_type: &str) -> NewWallet {
        NewWallet {
            name: "Test".into(),
            custody_type: "cold_storage".into(),
            risk_level: "low".into(),
            source_type: source_type.into(),
            descriptor: None,
            change_descriptor: None,
            xpub: None,
            master_fingerprint: None,
            derivation_path: None,
            network: "bitcoin".into(),
            manual_balance_sats: None,
            manual_cost_basis_fiat: None,
            color: None,
            notes: None,
        }
    }

    // Sparrow-style <0;1> descriptor; BIP84 test vector account 0 ("abandon … about"), public test data.
    const SPARROW: &str = "wpkh([73c5da0a/84h/0h/0h]xpub6CatWdiZiodmUeTDp8LT5or8nmbKNcuyvz7WyksVFkKB4RHwCD3XyuvPEbvqAQY3rAPshWcMLoP2fMFMKHPJ4ZeZXYVUhLv1VMrjPC7PW6V/<0;1>/*)";

    #[test]
    fn a_multipath_descriptor_is_stored_as_receive_and_change() {
        let conn = mem_db();
        let mut w = new_wallet("descriptor");
        w.descriptor = Some(SPARROW.into());
        let saved = insert_wallet(&conn, w).unwrap();
        let (recv, change) = (saved.descriptor.unwrap(), saved.change_descriptor.unwrap());
        assert!(!recv.contains('<') && recv.contains("/0/*)") && change.contains("/1/*)"), "{recv} {change}");
    }

    #[test]
    fn a_multipath_descriptor_saved_before_the_fix_is_split_when_the_vault_opens() {
        let conn = mem_db();
        conn.execute(
            "INSERT INTO wallets (id, name, custody_type, risk_level, source_type, descriptor, network)
             VALUES ('old','Old','cold_storage','low','descriptor',?1,'bitcoin')",
            [SPARROW],
        )
        .unwrap();
        crate::db::split_saved_multipath_descriptors(&conn).unwrap();
        crate::db::split_saved_multipath_descriptors(&conn).unwrap(); // a second open changes nothing
        let (recv, change): (String, String) = conn
            .query_row("SELECT descriptor, change_descriptor FROM wallets WHERE id = 'old'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert!(recv.contains("/0/*)") && change.contains("/1/*)"), "{recv} {change}");
    }

    #[test]
    fn a_seed_phrase_or_wif_key_is_refused_in_every_field_and_public_material_is_not_mistaken_for_one() {
        let seed12 = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let seed24 = ["zoo"; 23].join(" ") + " wrong";
        for s in [seed12, seed24.as_str(), "Abandon Abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon About", "  abandon abandon abandon abandon abandon abandon\nabandon abandon abandon abandon abandon about "] {
            assert!(looks_like_seed_phrase(s), "{s}");
            let err = refuse_private_key_material([None, Some(s)]).unwrap_err();
            assert!(err.contains("seed phrase"), "{err}");
        }
        for wif in ["5HueCGU8rMjxEXxiPuD5BDku4MkFqeZyd4dZ1jvhTVqvbTLvyTJ", "KwdMAjGmerYanjeui5SHS7JkmpZvVipYvB2LJGU1ZxJwYvP98617"] {
            assert!(refuse_private_key_material([Some(wif)]).unwrap_err().contains("WIF"), "{wif}");
        }
        assert!(!looks_like_wif("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2"));
        assert!(!looks_like_wif("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdqbc1qar0srrr7"));
        for fine in [
            XPUB,
            "wpkh([d34db33f/84h/0h/0h]xpub6CUGRUonZSQ4TWtTMmzXdrXDtypWKiKrhko4egpiMZbpiaQL2jkwSB1icqYh2cfDfVxdx4df189oLKnC5fSwqPfgyP3hooxujYzAu3fDVmz/0/*)",
            "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
            "my cold storage wallet",
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon",
        ] {
            assert!(!looks_like_seed_phrase(fine), "{fine}");
        }
    }

    #[test]
    fn descriptor_containing_xprv_is_refused_and_no_row_is_added() {
        let conn = mem_db();
        let mut w = new_wallet("descriptor");
        w.descriptor = Some(format!("wpkh({XPRV}/84'/0'/0'/0/*)"));
        assert_eq!(insert_wallet(&conn, w).unwrap_err(), REFUSAL);
        assert_eq!(count(&conn, "wallets"), 0);

        // The change descriptor is checked too, even when the receive one is clean.
        let mut w = new_wallet("descriptor");
        w.descriptor = Some(format!("wpkh([aabbccdd/84'/0'/0']{XPUB}/0/*)"));
        w.change_descriptor = Some(format!("wpkh({XPRV}/84'/0'/0'/1/*)"));
        assert_eq!(insert_wallet(&conn, w).unwrap_err(), REFUSAL);
        assert_eq!(count(&conn, "wallets"), 0);
        assert_eq!(count(&conn, "security_config"), 0);
    }

    #[test]
    fn xpub_field_containing_xprv_is_refused_for_every_source_type() {
        let conn = mem_db();
        for source in ["xpub", "descriptor", "manual"] {
            let mut w = new_wallet(source);
            w.xpub = Some(XPRV.into());
            if source == "descriptor" {
                w.descriptor = Some(format!("wpkh([aabbccdd/84'/0'/0']{XPUB}/0/*)"));
            }
            assert_eq!(insert_wallet(&conn, w).unwrap_err(), REFUSAL, "source_type {source}");
        }
        assert_eq!(count(&conn, "wallets"), 0);
    }

    #[test]
    fn every_extended_private_prefix_is_refused() {
        for prefix in ["xprv", "yprv", "zprv", "Yprv", "Zprv", "tprv", "uprv", "vprv", "Uprv", "Vprv"] {
            let mut w = new_wallet("manual");
            w.xpub = Some(format!("{prefix}{}", &XPRV[4..]));
            assert_eq!(insert_wallet(&mem_db(), w).unwrap_err(), REFUSAL, "{prefix}");
        }
    }

    /// The reported case: a multisig descriptor carrying a capitalized (SLIP-132 multisig) private
    /// key, built from a real key re-encoded with each of the four capitalized versions.
    #[test]
    fn multisig_descriptor_with_a_capitalized_private_key_is_refused_and_nothing_is_stored() {
        let conn = mem_db();
        for (name, version) in [
            ("Yprv", [0x02, 0x95, 0xB0, 0x05]),
            ("Zprv", [0x02, 0xAA, 0x7A, 0x99]),
            ("Uprv", [0x02, 0x42, 0x85, 0xB5]),
            ("Vprv", [0x02, 0x57, 0x50, 0x48]),
        ] {
            let mut data = bitcoin::base58::decode_check(XPRV).unwrap();
            data[..4].copy_from_slice(&version);
            let key = bitcoin::base58::encode_check(&data);
            assert!(key.starts_with(name));
            let mut w = new_wallet("descriptor");
            w.descriptor = Some(format!("wsh(sortedmulti(2,[aabbccdd/48'/0'/0'/2']{key}/0/*,[11223344/48'/0'/0'/2']{XPUB}/0/*))"));
            assert_eq!(insert_wallet(&conn, w).unwrap_err(), REFUSAL, "{name}");
        }
        assert_eq!(count(&conn, "wallets"), 0);
    }

    #[test]
    fn descriptor_with_only_an_xpub_still_inserts() {
        let conn = mem_db();
        let mut w = new_wallet("descriptor");
        w.descriptor = Some(format!("wpkh([aabbccdd/84'/0'/0']{XPUB}/0/*)"));
        insert_wallet(&conn, w).unwrap();
        assert_eq!(count(&conn, "wallets"), 1);

        let mut w = new_wallet("xpub");
        w.name = "Plain xpub".into();
        w.xpub = Some(XPUB.into());
        insert_wallet(&conn, w).unwrap();
        assert_eq!(count(&conn, "wallets"), 2);
    }

    #[test]
    fn multisig_cosigner_xprv_is_refused_before_anything_is_written() {
        let conn = mem_db();
        let cosigner = |xpub: &str| Cosigner {
            id: Uuid::new_v4().to_string(),
            label: "key".into(),
            xpub: xpub.into(),
            fingerprint: Some("aabbccdd".into()),
            derivation_path: Some("m/48'/0'/0'/2'".into()),
            is_mine: false,
            hardware_model: None,
        };
        let cosigners = [cosigner(XPUB), cosigner(XPRV)];
        let err = insert_multisig_wallet(&conn, "Vault".into(), "multisig".into(), "low".into(), "bitcoin".into(), 2, &cosigners, "p2wsh").unwrap_err();
        assert_eq!(err, REFUSAL);
        for table in ["wallets", "multisig_configs", "multisig_cosigners", "security_config"] {
            assert_eq!(count(&conn, table), 0, "{table}");
        }
    }

    /// Rows the app can never edit or delete must not take key-like text: it would block encrypted
    /// backups for good. Editable fields (names, notes) are left as the owner decided.
    #[test]
    fn text_in_rows_that_can_never_be_edited_is_checked_too() {
        const KEY_LIKE: &str = "xprvAAAAAAAAAAAAAAAAAAAAAAAA";
        const XPUB_2: &str = "xpub661MyMwAqRbcFW31YEwpkMuc5THy2PSt5bDMsktWQcFF8syAmRUapSCGu8ED9W6oDMSgv6Zz8idoc4a6mr8BDzTJY47LJhkJ8UB7WEGuduB";
        let conn = mem_db();

        // Origin text on a wallet type that does not validate it.
        let mut w = new_wallet("manual");
        w.master_fingerprint = Some("aabbccdd".into());
        w.derivation_path = Some(KEY_LIKE.into());
        assert_eq!(insert_wallet(&conn, w).unwrap_err(), REFUSAL);

        // Cosigner label and hardware model.
        let cosigner = |xpub: &str, label: &str, model: Option<&str>| Cosigner {
            id: Uuid::new_v4().to_string(),
            label: label.into(),
            xpub: xpub.into(),
            fingerprint: Some("aabbccdd".into()),
            derivation_path: Some("m/48'/0'/0'/2'".into()),
            is_mine: false,
            hardware_model: model.map(String::from),
        };
        for cosigners in [
            [cosigner(XPUB, KEY_LIKE, None), cosigner(XPUB_2, "Key 2", None)],
            [cosigner(XPUB, "Key 1", Some(KEY_LIKE)), cosigner(XPUB_2, "Key 2", None)],
        ] {
            let err = insert_multisig_wallet(&conn, "Vault".into(), "multisig_vault".into(), "low".into(), "bitcoin".into(), 2, &cosigners, "p2wsh").unwrap_err();
            assert_eq!(err, REFUSAL);
        }
        for table in ["wallets", "multisig_configs", "multisig_cosigners", "security_config"] {
            assert_eq!(count(&conn, table), 0, "{table}");
        }

        // Ordinary text still saves.
        let ok = [cosigner(XPUB, "Key 1", Some("Trezor")), cosigner(XPUB_2, "Key 2", None)];
        insert_multisig_wallet(&conn, "Vault".into(), "multisig_vault".into(), "low".into(), "bitcoin".into(), 2, &ok, "p2wsh").unwrap();
        assert_eq!(count(&conn, "multisig_cosigners"), 2);
    }
}
