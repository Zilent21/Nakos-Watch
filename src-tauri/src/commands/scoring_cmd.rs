use crate::db::models::{HealthScoreSnapshot, SecurityConfig};
use crate::commands::node::{build_core_rpc_config, probe_node_status, row_to_config};
use crate::rpc::core_rpc;
use crate::scoring::health::{self, CompositeInput, LiquidityInput, NodeHealthInput, NodeHealthReport};
use crate::scoring::security::{self, SecurityInput};
use crate::scoring::privacy_metrics::{self, CoinRow, ConsolidationRisk, ReuseSummary, ScriptMixEntry};
use crate::scoring::{privacy, ScoreBreakdown};
use crate::state::AppState;
use rusqlite::{params, Connection};
use uuid::Uuid;

fn build_security_input(conn: &Connection) -> Result<SecurityInput, String> {
    let mut input = SecurityInput::default();

    input.total_active_wallets = conn
        .query_row("SELECT COUNT(*) FROM wallets WHERE archived = 0", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    input.multisig_wallet_count = conn
        .query_row(
            "SELECT COUNT(*) FROM wallets WHERE archived = 0 AND source_type = 'multisig_descriptor'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.singlesig_hardware_wallet_count = conn
        .query_row(
            "SELECT COUNT(*) FROM wallets WHERE archived = 0 AND custody_type = 'cold_storage' AND source_type != 'multisig_descriptor'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.wallets_with_passphrase = conn
        .query_row("SELECT COUNT(*) FROM security_config WHERE passphrase_enabled = 1", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    input.hardware_wallets_total = conn
        .query_row(
            "SELECT COUNT(*) FROM security_config WHERE hardware_wallet_model IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.hardware_wallets_firmware_current = conn
        .query_row(
            "SELECT COUNT(*) FROM security_config WHERE firmware_up_to_date = 1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.max_geographic_locations_reported = conn
        .query_row(
            "SELECT COALESCE(MAX(geographic_redundancy_locations), 0) FROM security_config",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.hardware_wallets_backup_tested = conn
        .query_row("SELECT COUNT(*) FROM security_config WHERE backup_tested = 1", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    input.exchange_wallet_count = conn
        .query_row(
            "SELECT COUNT(*) FROM wallets WHERE archived = 0 AND custody_type = 'exchange'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.exchange_wallets_with_hardware_2fa = conn
        .query_row(
            "SELECT COUNT(*) FROM wallets w JOIN security_config sc ON sc.wallet_id = w.id
             WHERE w.custody_type = 'exchange' AND sc.exchange_2fa_type = 'hardware_key'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.inheritance_test_status = conn
        .query_row(
            "SELECT test_status FROM inheritance_configs ORDER BY updated_at DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "never_tested".to_string());

    Ok(input)
}

fn build_privacy_input(conn: &Connection) -> Result<privacy::PrivacyInput, String> {
    let mut input = privacy::PrivacyInput::default();

    input.total_addresses_used = conn
        .query_row("SELECT COUNT(*) FROM addresses a JOIN wallets w ON w.id = a.wallet_id WHERE a.is_used = 1 AND w.archived = 0", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    // Reused = addresses that have more than one live UTXO recorded against them.
    input.reused_addresses = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT u.address_id FROM utxos u JOIN wallets w ON w.id = u.wallet_id WHERE u.address_id IS NOT NULL AND u.spent = 0 AND w.archived = 0 GROUP BY u.address_id HAVING COUNT(*) > 1)",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.total_utxos = conn
        .query_row("SELECT COUNT(*) FROM utxos u JOIN wallets w ON w.id = u.wallet_id WHERE u.spent = 0 AND w.archived = 0", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    // CIOH exposure proxy: UTXOs belonging to a wallet whose most recent tx had >1 wallet input,
    // i.e. transactions of direction 'self' or 'outgoing' with multiple same-wallet inputs.
    // We approximate using outgoing transactions with negative amount magnitude larger than any
    // single UTXO the wallet held (implies multiple inputs were combined).
    input.utxos_with_cioh_exposure = conn
        .query_row(
            "SELECT COUNT(*) FROM utxos u JOIN wallets w ON w.id = u.wallet_id WHERE u.is_change = 1 AND u.spent = 0 AND w.archived = 0",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    input.total_transactions = conn
        .query_row("SELECT COUNT(*) FROM transactions t JOIN wallets w ON w.id = t.wallet_id WHERE w.archived = 0", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    input.kyc_linked_transactions = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions t JOIN wallets w ON w.id = t.wallet_id WHERE t.category IN ('kyc_withdrawal','kyc_deposit') AND w.archived = 0",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    input.total_outgoing_transactions_with_change = conn
        .query_row("SELECT COUNT(*) FROM transactions t JOIN wallets w ON w.id = t.wallet_id WHERE t.direction = 'outgoing' AND w.archived = 0", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    input.detectable_change_outputs = conn
        .query_row(
            "SELECT COUNT(*) FROM utxos u JOIN wallets w ON w.id = u.wallet_id WHERE u.is_change = 1 AND u.spent = 0 AND w.archived = 0",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    input.merged_coin_events = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions t JOIN wallets w ON w.id = t.wallet_id WHERE t.direction = 'outgoing' AND t.category = 'consolidation' AND w.archived = 0",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    Ok(input)
}

#[tauri::command]
pub fn get_security_score(state: tauri::State<AppState>) -> Result<ScoreBreakdown, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let input = build_security_input(&conn)?;
    Ok(security::score(&input))
}

#[tauri::command]
pub fn get_privacy_score(state: tauri::State<AppState>) -> Result<ScoreBreakdown, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let input = build_privacy_input(&conn)?;
    Ok(privacy::score(&input))
}

/// Node health from the saved Bitcoin Core RPC config (is_default first, else
/// the oldest enabled `bitcoin_core_rpc` row). Score comes from
/// `health::node_health_score`: 80 pts for chain sync (scaled by
/// verification progress, full when headers == blocks and not in IBD),
/// up to 15 pts for peers (8+ = full), 5 pts for Tor reachability.
/// No config → `configured: false`, no score. RPC failure → `error` set,
/// no score (the dimension is dropped from the composite, not zeroed).
fn compute_node_health(state: &AppState, conn: &Connection) -> NodeHealthReport {
    let cfg = conn
        .query_row(
            "SELECT * FROM node_configs WHERE kind = 'bitcoin_core_rpc' AND enabled = 1
             ORDER BY is_default DESC, created_at ASC LIMIT 1",
            [],
            row_to_config,
        )
        .ok();
    let Some(cfg) = cfg else { return NodeHealthReport::default() };
    match build_core_rpc_config(state, conn, &cfg) {
        Ok(rpc_cfg) => probe_node_health(cfg.label, rpc_cfg),
        Err(e) => NodeHealthReport { configured: true, node_label: Some(cfg.label), error: Some(e), ..Default::default() },
    }
}

/// Bounded Core probe (same 3 s connect / 5 s total budget as the status
/// poll), so a dead host cannot stall `get_health_score`.
fn probe_node_health(label: String, rpc_cfg: core_rpc::CoreRpcConfig) -> NodeHealthReport {
    let status = probe_node_status(Some(rpc_cfg), None).core.unwrap_or_else(|| Err("node did not answer in time".to_string()));
    node_health_report(label, status)
}

fn node_health_report(label: String, status: Result<core_rpc::NodeStatus, String>) -> NodeHealthReport {
    let mut report = NodeHealthReport { configured: true, node_label: Some(label), ..Default::default() };
    let st = match status {
        Ok(st) => st,
        Err(e) => {
            report.error = Some(e);
            return report;
        }
    };

    let peers = st.connections_in + st.connections_out;
    report.score = Some(health::node_health_score(&NodeHealthInput {
        is_synced: st.is_synced,
        verification_progress: st.verification_progress,
        peer_count: peers,
        is_reachable_via_tor: st.network_reachable_via_tor,
    }));
    report.reasons.push(format!("{} on {}, height {}", st.subversion, st.chain, st.blocks));
    if st.initial_block_download {
        report.reasons.push(format!("Initial block download ({:.2}% verified)", st.verification_progress * 100.0));
    } else if st.headers != st.blocks {
        report.reasons.push(format!("Behind headers: {} of {} blocks", st.blocks, st.headers));
    } else if st.verification_progress < 0.9999 {
        report.reasons.push(format!("Verification at {:.2}%", st.verification_progress * 100.0));
    } else {
        report.reasons.push("Fully synced".to_string());
    }
    report.reasons.push(if peers == 0 { "0 peers".to_string() } else { format!("{peers} peers") });
    report
}

#[tauri::command]
pub async fn get_health_score(state: tauri::State<'_, AppState>) -> Result<health::CompositeResult, String> {
    health_score_inner(&state)
}

fn health_score_inner(state: &AppState) -> Result<health::CompositeResult, String> {
    Ok(compute_health(state)?.composite)
}

/// Everything a score computation produces. Pure read: callers that want a
/// history row must write it themselves (see `record_health_snapshot`).
struct HealthComputation {
    composite: health::CompositeResult,
    security_score: i64,
    privacy_score: i64,
    liquidity: i64,
    inheritance_score: i64,
    redundancy_score: i64,
    tax_readiness: Option<i64>,
    details: serde_json::Value,
}

fn compute_health(state: &AppState) -> Result<HealthComputation, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    let security_input = build_security_input(&conn)?;
    let security_breakdown = security::score(&security_input);
    let privacy_input = build_privacy_input(&conn)?;
    let privacy_breakdown = privacy::score(&privacy_input);

    let inheritance_score = security_breakdown
        .factors
        .iter()
        .find(|f| f.key == "inheritance_readiness")
        .map(|f| (f.points as f64 / f.max_points.max(1) as f64 * 100.0).round() as i64)
        .unwrap_or(0);
    let redundancy_score = security_breakdown
        .factors
        .iter()
        .find(|f| f.key == "geo_redundancy")
        .map(|f| (f.points as f64 / f.max_points.max(1) as f64 * 100.0).round() as i64)
        .unwrap_or(0);

    let total_utxos: i64 = conn
        .query_row("SELECT COUNT(*) FROM utxos WHERE spent = 0", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let dust_utxos: i64 = conn
        .query_row("SELECT COUNT(*) FROM utxos WHERE spent = 0 AND amount_sats < 10000", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let frozen_utxos: i64 = conn
        .query_row("SELECT COUNT(*) FROM utxos WHERE spent = 0 AND is_frozen = 1", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let liquidity = health::liquidity_score(&LiquidityInput {
        total_utxos,
        dust_utxos,
        frozen_utxos,
        max_single_wallet_share_pct: 0.0,
    });

    // A coin counts once it has a full lot (cost AND acquired date); archived
    // wallets are excluded. With no lots recorded at all there is nothing
    // real to score, so emit no numeric score rather than a fabricated one.
    let (tax_total, utxos_with_cost_basis) = crate::commands::utxos::tax_counts(&conn)?;
    let tax_readiness: Option<i64> = health::tax_readiness_option(tax_total, utxos_with_cost_basis);

    let node = compute_node_health(state, &conn);
    let mut composite = health::composite_score(&CompositeInput {
        security_score: security_breakdown.score,
        privacy_score: privacy_breakdown.score,
        liquidity_score: liquidity,
        inheritance_score,
        redundancy_score,
        node_health_score: node.score,
        lightning_health_score: None,
        tax_readiness_score: tax_readiness,
    });
    composite.node = node.clone();

    let details = serde_json::json!({
        "security": security_breakdown,
        "privacy": privacy_breakdown,
        "liquidity": liquidity,
        "tax_readiness": tax_readiness,
        "node": { "label": node.node_label, "reasons": node.reasons, "error": node.error },
    });

    Ok(HealthComputation {
        composite,
        security_score: security_breakdown.score,
        privacy_score: privacy_breakdown.score,
        liquidity,
        inheritance_score,
        redundancy_score,
        tax_readiness,
        details,
    })
}

/// The text stored with a history row. Snapshot rows can never be edited or deleted, and part of this
/// text is not ours: the node's version string and error come from whatever answers on the node's
/// address (plain HTTP, so even a network attacker), and wallet and node names are user text. If any
/// of it looks like an extended private key it would block encrypted backups for good, so the whole
/// details block is replaced rather than stored.
fn details_for_storage(details: &serde_json::Value) -> String {
    let text = details.to_string();
    if crate::crypto::backup::contains_extended_private_key(text.as_bytes()) {
        return serde_json::json!({ "removed": "details were not stored because they contained text that looks like a private key" }).to_string();
    }
    text
}

/// Explicit history writer: recompute on the backend (the UI never supplies
/// numbers) and append one `health_score_snapshots` row. Only called when the
/// user asks; `get_health_score` stays read-only and nothing runs on a timer.
#[tauri::command]
pub async fn record_health_snapshot(state: tauri::State<'_, AppState>) -> Result<String, String> {
    record_health_snapshot_inner(&state)
}

fn record_health_snapshot_inner(state: &AppState) -> Result<String, String> {
    let h = compute_health(state)?;
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    // `tax_readiness_score` is NOT NULL in this log (the live CompositeResult can
    // omit the dimension). 0 is stored when no cost-basis data exists yet;
    // `details_json.tax_readiness` is the authoritative value (`null` then).
    conn.execute(
        "INSERT INTO health_score_snapshots (
            id, security_score, privacy_score, liquidity_score, inheritance_score,
            redundancy_score, node_health_score, lightning_health_score, tax_readiness_score,
            composite_score, details_json
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id,
            h.security_score,
            h.privacy_score,
            h.liquidity,
            h.inheritance_score,
            h.redundancy_score,
            h.composite.node.score,
            Option::<i64>::None,
            h.tax_readiness.unwrap_or(0),
            h.composite.composite_score,
            details_for_storage(&h.details),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

/// Preference key for the optional daily snapshot. Absent or anything but
/// "true" means off; it is off by default.
const DAILY_SNAPSHOT_PREF: &str = "health_snapshot_daily";

/// Record one snapshot if the user opted in, the vault is unlocked (node
/// credentials are sealed), and no snapshot, manual or automatic, exists yet
/// for today (local date). Returns the new row id, or None when nothing was due.
fn daily_snapshot_if_due(state: &AppState) -> Result<Option<String>, String> {
    let pool = state.pool()?;
    let due = {
        let conn = pool.get().map_err(|e| e.to_string())?;
        if crate::db::pref_get(&conn, DAILY_SNAPSHOT_PREF).map_err(|e| e.to_string())?.as_deref() != Some("true") {
            return Ok(None);
        }
        let taken_today: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM health_score_snapshots WHERE date(taken_at, 'localtime') = date('now', 'localtime'))",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        !taken_today
    };
    if !due || !state.vault()?.lock().master_key.is_some() {
        return Ok(None);
    }
    record_health_snapshot_inner(state).map(Some)
}

static DAILY_SNAPSHOT_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called by the frontend once per unlock. Returns at once; the computation (which
/// can wait on the bounded node probe) runs on its own thread so unlock and the UI
/// are never held up. Does nothing unless the user turned the option on.
#[tauri::command]
pub fn run_daily_health_snapshot(app: tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    // One run at a time: a double call (remount, relock/unlock) cannot write twice.
    if DAILY_SNAPSHOT_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                DAILY_SNAPSHOT_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _reset = Reset;
        let state = app.state::<AppState>();
        let _ = daily_snapshot_if_due(&state);
    });
}

#[tauri::command]
pub fn get_health_history(state: tauri::State<AppState>, limit: Option<i64>) -> Result<Vec<HealthScoreSnapshot>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(90);

    let mut stmt = conn
        .prepare("SELECT * FROM health_score_snapshots ORDER BY taken_at DESC LIMIT ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(HealthScoreSnapshot {
                id: r.get("id")?,
                taken_at: r.get("taken_at")?,
                security_score: r.get("security_score")?,
                privacy_score: r.get("privacy_score")?,
                liquidity_score: r.get("liquidity_score")?,
                inheritance_score: r.get("inheritance_score")?,
                redundancy_score: r.get("redundancy_score")?,
                node_health_score: r.get("node_health_score")?,
                lightning_health_score: r.get("lightning_health_score")?,
                tax_readiness_score: r.get("tax_readiness_score")?,
                composite_score: r.get("composite_score")?,
                details_json: r.get("details_json")?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[derive(Debug, serde::Deserialize)]
pub struct SecurityConfigPatch {
    pub wallet_id: Option<String>,
    pub hardware_wallet_model: Option<String>,
    pub firmware_up_to_date: Option<bool>,
    pub passphrase_enabled: Option<bool>,
    pub geographic_redundancy_locations: Option<i64>,
    pub backup_tested: Option<bool>,
    pub exchange_2fa_type: Option<String>,
}

#[tauri::command]
pub fn update_security_config(state: tauri::State<AppState>, patch: SecurityConfigPatch) -> Result<SecurityConfig, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM security_config WHERE wallet_id IS ?1",
            params![patch.wallet_id],
            |r| r.get(0),
        )
        .ok();

    let id = existing_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    if conn
        .query_row("SELECT 1 FROM security_config WHERE id = ?1", params![id], |_| Ok(()))
        .is_err()
    {
        conn.execute(
            "INSERT INTO security_config (id, wallet_id) VALUES (?1, ?2)",
            params![id, patch.wallet_id],
        )
        .map_err(|e| e.to_string())?;
    }

    macro_rules! apply {
        ($field:ident, $col:literal) => {
            if let Some(v) = &patch.$field {
                conn.execute(
                    concat!("UPDATE security_config SET ", $col, " = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2"),
                    params![v, id],
                )
                .map_err(|e| e.to_string())?;
            }
        };
    }
    apply!(hardware_wallet_model, "hardware_wallet_model");
    apply!(firmware_up_to_date, "firmware_up_to_date");
    apply!(passphrase_enabled, "passphrase_enabled");
    apply!(geographic_redundancy_locations, "geographic_redundancy_locations");
    apply!(backup_tested, "backup_tested");
    apply!(exchange_2fa_type, "exchange_2fa_type");

    if patch.backup_tested == Some(true) {
        conn.execute(
            "UPDATE security_config SET backup_tested_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
    }

    conn.query_row("SELECT * FROM security_config WHERE id = ?1", params![id], |row| {
        Ok(SecurityConfig {
            id: row.get("id")?,
            wallet_id: row.get("wallet_id")?,
            hardware_wallet_model: row.get("hardware_wallet_model")?,
            firmware_up_to_date: row.get::<_, Option<i64>>("firmware_up_to_date")?.map(|v| v != 0),
            passphrase_enabled: row.get::<_, i64>("passphrase_enabled")? != 0,
            is_multisig: row.get::<_, i64>("is_multisig")? != 0,
            geographic_redundancy_locations: row.get("geographic_redundancy_locations")?,
            backup_tested: row.get::<_, i64>("backup_tested")? != 0,
            backup_tested_at: row.get("backup_tested_at")?,
            exchange_2fa_type: row.get("exchange_2fa_type")?,
            updated_at: row.get("updated_at")?,
        })
    })
    .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Serialize)]
pub struct SyncSourceInfo {
    pub configured: bool,
    pub label: Option<String>,
    pub host: Option<String>,
    pub port: Option<i64>,
    pub use_tls: bool,
    /// From the saved row only; no network probe is made here.
    pub last_connected_at: Option<String>,
    pub note: String,
}

#[derive(Debug, serde::Serialize)]
pub struct PrivacyDashboard {
    pub wallet_count: i64,
    pub utxo_count: usize,
    pub total_sats: u64,
    pub script_mix: Vec<ScriptMixEntry>,
    pub reuse: ReuseSummary,
    pub consolidation: ConsolidationRisk,
    pub sync_source: SyncSourceInfo,
}

/// Privacy Dashboard data from local sqlite only (unspent coins, unarchived
/// wallets). Distribution buckets come from the existing `utxo_distribution`
/// command on the frontend; Core status from `get_node_status`.
/// future: optional explorer / LN / cloud privacy providers.
#[tauri::command]
pub fn get_privacy_dashboard(state: tauri::State<AppState>) -> Result<PrivacyDashboard, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    let wallet_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallets WHERE archived = 0", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT a.address, u.amount_sats FROM utxos u
             JOIN wallets w ON w.id = u.wallet_id
             LEFT JOIN addresses a ON a.id = u.address_id
             WHERE u.spent = 0 AND w.archived = 0",
        )
        .map_err(|e| e.to_string())?;
    let coins: Vec<CoinRow> = stmt
        .query_map([], |r| Ok(CoinRow { address: r.get(0)?, amount_sats: r.get::<_, i64>(1)?.max(0) as u64 }))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let amounts: Vec<u64> = coins.iter().map(|c| c.amount_sats).collect();

    let sync_source = conn
        .query_row(
            "SELECT label, host, port, use_tls, last_connected_at FROM node_configs
             WHERE kind = 'electrum' AND enabled = 1 ORDER BY is_default DESC, created_at ASC LIMIT 1",
            [],
            |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?))
            },
        )
        .ok()
        .map(|(label, host, port, tls, last)| SyncSourceInfo {
            configured: true,
            label: Some(label),
            host: Some(host),
            port: Some(port),
            use_tls: tls != 0,
            last_connected_at: last,
            note: "Wallet sync asks this Electrum server about your addresses, so the server operator can see which addresses are yours. On hardware you run yourself, that stays on your network.".into(),
        })
        .unwrap_or(SyncSourceInfo {
            configured: false,
            label: None,
            host: None,
            port: None,
            use_tls: false,
            last_connected_at: None,
            note: "No Electrum server configured, so wallets can't sync. Add one in Node Ops.".into(),
        });

    Ok(PrivacyDashboard {
        wallet_count,
        utxo_count: coins.len(),
        total_sats: amounts.iter().sum(),
        script_mix: privacy_metrics::script_mix(&coins),
        reuse: privacy_metrics::address_reuse(&coins),
        consolidation: privacy_metrics::consolidation_risk(&amounts),
        sync_source,
    })
}

#[cfg(test)]
mod health_probe_tests {
    use super::*;
    use crate::commands::node::STATUS_TOTAL_BUDGET;
    use std::time::{Duration, Instant};

    #[test]
    fn health_node_probe_returns_within_budget_on_a_closed_loopback_port() {
        // Bind then drop to get a loopback port with nothing listening.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let cfg = core_rpc::CoreRpcConfig { host: "127.0.0.1".into(), port, user: Some("u".into()), password: Some("p".into()), cookie_path: None, use_tls: false };
        let start = Instant::now();
        let report = probe_node_health("core".into(), cfg);
        let elapsed = start.elapsed();
        assert!(report.configured && report.score.is_none() && report.error.is_some(), "dead node: no score, error set");
        assert!(elapsed < STATUS_TOTAL_BUDGET + Duration::from_millis(1500), "health probe took {elapsed:?}");
    }

    #[test]
    fn health_node_probe_returns_within_budget_when_the_host_never_answers() {
        // Loopback listener that accepts but never replies: the real read timeout has to fire.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let cfg = core_rpc::CoreRpcConfig { host: "127.0.0.1".into(), port, user: Some("u".into()), password: Some("p".into()), cookie_path: None, use_tls: false };
        let start = Instant::now();
        let report = probe_node_health("core".into(), cfg);
        let elapsed = start.elapsed();
        assert!(report.score.is_none() && report.error.is_some());
        assert!(elapsed < STATUS_TOTAL_BUDGET + Duration::from_millis(1500), "health probe took {elapsed:?}");
        drop(listener);
    }

    #[test]
    fn health_read_does_not_add_a_snapshot_row() {
        let dir = crate::test_dir::TestDir::new("health");
        let pool = crate::db::open_pool(&dir).unwrap();
        let state = AppState::default();
        state.inner.lock().pool = Some(pool.clone());
        let conn = pool.get().unwrap();
        let count = |c: &Connection| -> i64 { c.query_row("SELECT COUNT(*) FROM health_score_snapshots", [], |r| r.get(0)).unwrap() };
        assert_eq!(count(&conn), 0);
        health_score_inner(&state).unwrap();
        health_score_inner(&state).unwrap();
        assert_eq!(count(&conn), 0, "health read must not insert a snapshot row");
        drop(conn);
        drop(pool);
        state.inner.lock().pool = None;
    }

    #[test]
    fn record_health_snapshot_writes_exactly_one_row_per_explicit_call() {
        let dir = crate::test_dir::TestDir::new("health-rec");
        let pool = crate::db::open_pool(&dir).unwrap();
        let state = AppState::default();
        state.inner.lock().pool = Some(pool.clone());
        let conn = pool.get().unwrap();
        let count = |c: &Connection| -> i64 { c.query_row("SELECT COUNT(*) FROM health_score_snapshots", [], |r| r.get(0)).unwrap() };

        health_score_inner(&state).unwrap();
        assert_eq!(count(&conn), 0, "read path stays read-only");

        let id = record_health_snapshot_inner(&state).unwrap();
        assert_eq!(count(&conn), 1);
        let (composite, tax, details): (i64, i64, String) = conn
            .query_row("SELECT composite_score, tax_readiness_score, details_json FROM health_score_snapshots WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!(composite, health_score_inner(&state).unwrap().composite_score, "row matches a fresh read");
        assert_eq!(tax, 0, "no lots: NOT NULL column holds 0");
        let details: serde_json::Value = serde_json::from_str(&details).unwrap();
        assert!(details["tax_readiness"].is_null(), "details keep the authoritative null");

        record_health_snapshot_inner(&state).unwrap();
        assert_eq!(count(&conn), 2, "one row per explicit call");
        health_score_inner(&state).unwrap();
        assert_eq!(count(&conn), 2, "and a later read adds none");
        drop(conn);
        drop(pool);
        state.inner.lock().pool = None;
    }

    fn state_with_vault(dir: &std::path::Path, unlocked: bool) -> AppState {
        let pool = crate::db::open_pool(dir).unwrap();
        let (_, key) = crate::crypto::init_passphrase("correct horse battery staple").unwrap();
        let state = AppState::default();
        let mut inner = state.inner.lock();
        inner.pool = Some(pool);
        inner.vault = Some(std::sync::Arc::new(parking_lot::Mutex::new(crate::state::VaultState { master_key: unlocked.then_some(key) })));
        drop(inner);
        state
    }

    #[test]
    fn daily_snapshot_is_off_by_default_and_needs_an_unlocked_vault() {
        let dir = crate::test_dir::TestDir::new("daily");
        let state = state_with_vault(&dir, true);
        let pool = state.pool().unwrap();
        let conn = pool.get().unwrap();
        let count = |c: &Connection| -> i64 { c.query_row("SELECT COUNT(*) FROM health_score_snapshots", [], |r| r.get(0)).unwrap() };

        assert_eq!(daily_snapshot_if_due(&state).unwrap(), None, "off by default");
        assert_eq!(count(&conn), 0);

        crate::db::pref_set(&conn, DAILY_SNAPSHOT_PREF, "false").unwrap();
        assert_eq!(daily_snapshot_if_due(&state).unwrap(), None, "explicitly off");
        assert_eq!(count(&conn), 0);

        // On, but the vault is locked: skip rather than store a row built without node credentials.
        let locked_dir = crate::test_dir::TestDir::new("daily-locked");
        let locked = state_with_vault(&locked_dir, false);
        let locked_pool = locked.pool().unwrap();
        let locked_conn = locked_pool.get().unwrap();
        crate::db::pref_set(&locked_conn, DAILY_SNAPSHOT_PREF, "true").unwrap();
        assert_eq!(daily_snapshot_if_due(&locked).unwrap(), None, "locked vault");
        assert_eq!(count(&locked_conn), 0);

        drop((conn, locked_conn, pool, locked_pool));
        state.inner.lock().pool = None;
        locked.inner.lock().pool = None;
    }

    #[test]
    fn daily_snapshot_writes_once_per_local_day_when_on() {
        let dir = crate::test_dir::TestDir::new("daily-on");
        let state = state_with_vault(&dir, true);
        let pool = state.pool().unwrap();
        let conn = pool.get().unwrap();
        let count = |c: &Connection| -> i64 { c.query_row("SELECT COUNT(*) FROM health_score_snapshots", [], |r| r.get(0)).unwrap() };
        crate::db::pref_set(&conn, DAILY_SNAPSHOT_PREF, "true").unwrap();

        // A row from two days ago does not satisfy today.
        conn.execute(
            "INSERT INTO health_score_snapshots (id, taken_at, security_score, privacy_score, liquidity_score, inheritance_score, redundancy_score, tax_readiness_score, composite_score)
             VALUES ('old', strftime('%Y-%m-%dT%H:%M:%fZ','now','-2 days'), 1,1,1,1,1,0,1)",
            [],
        )
        .unwrap();
        assert_eq!(count(&conn), 1);

        assert!(daily_snapshot_if_due(&state).unwrap().is_some(), "first call today writes");
        assert_eq!(count(&conn), 2);
        assert_eq!(daily_snapshot_if_due(&state).unwrap(), None, "second call same day is a no-op");
        assert_eq!(count(&conn), 2);

        // An explicit manual snapshot also counts as today's.
        conn.execute("DELETE FROM health_score_snapshots WHERE id != 'old'", []).unwrap();
        record_health_snapshot_inner(&state).unwrap();
        assert_eq!(daily_snapshot_if_due(&state).unwrap(), None, "manual snapshot satisfies the day");
        assert_eq!(count(&conn), 2);

        drop((conn, pool));
        state.inner.lock().pool = None;
    }

    #[test]
    fn stored_snapshot_details_never_carry_key_like_text() {
        // Text a hostile node (or a mistyped name) could put into the details.
        let key_like = "xprvAAAAAAAAAAAAAAAAAAAAAAAA";
        let hostile = serde_json::json!({ "liquidity": 80, "node": { "label": "core", "reasons": [format!("{key_like} on main, height 1")], "error": null } });
        let stored = details_for_storage(&hostile);
        assert!(!crate::crypto::backup::contains_extended_private_key(stored.as_bytes()), "stored: {stored}");
        assert!(stored.contains("removed"), "the row says why the details are missing");

        // Ordinary details are stored unchanged.
        let ordinary = serde_json::json!({ "liquidity": 80, "node": { "label": "core", "reasons": ["Satoshi:27.0.0 on main, height 1"], "error": null } });
        assert_eq!(details_for_storage(&ordinary), ordinary.to_string());
    }

    #[test]
    fn a_snapshot_taken_with_a_key_like_node_label_stores_no_key_like_text() {
        let dir = crate::test_dir::TestDir::new("r2snap");
        let state = state_with_vault(&dir, true);
        let pool = state.pool().unwrap();
        let conn = pool.get().unwrap();
        // An editable node label that looks like a key, on a Core node that refuses connections.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        conn.execute("INSERT INTO node_configs (id, kind, label, host, port) VALUES ('n','bitcoin_core_rpc','xprvAAAAAAAAAAAAAAAAAAAAAAAA','127.0.0.1',?1)", params![port]).unwrap();

        record_health_snapshot_inner(&state).unwrap();
        let details: String = conn.query_row("SELECT details_json FROM health_score_snapshots", [], |r| r.get(0)).unwrap();
        assert!(!crate::crypto::backup::contains_extended_private_key(details.as_bytes()), "details: {details}");

        drop((conn, pool));
        state.inner.lock().pool = None;
    }
}

#[cfg(test)]
mod not_live_coin_tests {
    use super::*;

    /// A coin sync keeps for its user data but marks not-live (`spent = 1`) is not a coin the
    /// wallet holds: it must not count as a UTXO, as an address-reuse hit or as change.
    #[test]
    fn privacy_input_ignores_coins_sync_marked_not_live() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn.execute("INSERT INTO wallets (id,name,custody_type,source_type) VALUES ('w','t','cold_storage','xpub')", []).unwrap();
        conn.execute("INSERT INTO addresses (id,wallet_id,address,keychain,is_used) VALUES ('a','w','addr','external',1)", []).unwrap();
        for (id, vout, spent) in [("live", 0, 0), ("kept", 1, 1)] {
            conn.execute(
                "INSERT INTO utxos (id,wallet_id,address_id,txid,vout,amount_sats,is_change,spent,label)
                 VALUES (?1,'w','a','tx',?2,1000,1,?3,'kept for its label')",
                params![id, vout, spent],
            )
            .unwrap();
        }
        let input = build_privacy_input(&conn).unwrap();
        assert_eq!(input.total_utxos, 1);
        assert_eq!(input.reused_addresses, 0, "one live coin on the address is not reuse");
        assert_eq!(input.utxos_with_cioh_exposure, 1);
        assert_eq!(input.detectable_change_outputs, 1);
    }
}
