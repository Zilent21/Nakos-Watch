use crate::bitcoin::psbt::{build_unsigned_psbt, PsbtInput, PsbtInputOrigin, PsbtOutput};
use crate::bitcoin::coin_select::{self, script_class, Candidate, Lot, Policy};
use crate::db::models::Utxo;
use crate::state::AppState;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

const DUST_THRESHOLD_SATS: i64 = 10_000;
/// Smallest change output worth creating (P2WPKH dust limit). Distinct from
/// `DUST_THRESHOLD_SATS`, which only buckets "small" UTXOs for display.
const CHANGE_DUST_SATS: u64 = 546;
const SMALL_THRESHOLD_SATS: i64 = 100_000;
const MEDIUM_THRESHOLD_SATS: i64 = 1_000_000;
const LARGE_THRESHOLD_SATS: i64 = 10_000_000;

fn row_to_utxo(row: &Row) -> rusqlite::Result<Utxo> {
    Ok(Utxo {
        id: row.get("id")?,
        wallet_id: row.get("wallet_id")?,
        address_id: row.get("address_id")?,
        txid: row.get("txid")?,
        vout: row.get("vout")?,
        amount_sats: row.get("amount_sats")?,
        confirmations: row.get("confirmations")?,
        block_height: row.get("block_height")?,
        block_time: row.get("block_time")?,
        is_change: row.get::<_, i64>("is_change")? != 0,
        is_frozen: row.get::<_, i64>("is_frozen")? != 0,
        label: row.get("label")?,
        acquisition_cost_fiat: row.get("acquisition_cost_fiat")?,
        acquisition_date: row.get("acquisition_date")?,
        acquisition_note: row.get("acquisition_note")?,
        privacy_risk_score: row.get("privacy_risk_score")?,
        spent: row.get::<_, i64>("spent")? != 0,
        spent_txid: row.get("spent_txid")?,
        spent_at: row.get("spent_at")?,
        unseen_since: row.get("unseen_since")?,
        created_at: row.get("created_at")?,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct UtxoFilter {
    pub wallet_id: Option<String>,
    pub min_amount_sats: Option<i64>,
    pub max_amount_sats: Option<i64>,
    pub is_frozen: Option<bool>,
    pub include_spent: Option<bool>,
    pub label_contains: Option<String>,
    /// When false (the default), UTXOs belonging to an archived wallet are
    /// excluded even when no specific `wallet_id` is given — an archived
    /// wallet should disappear from the "all wallets" view too.
    pub include_archived: Option<bool>,
}

#[tauri::command]
pub fn list_utxos(state: tauri::State<AppState>, filter: Option<UtxoFilter>) -> Result<Vec<Utxo>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let f = filter.unwrap_or_default();

    let mut sql = "SELECT u.* FROM utxos u JOIN wallets w ON w.id = u.wallet_id WHERE 1=1".to_string();
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if !f.include_spent.unwrap_or(false) {
        sql.push_str(" AND u.spent = 0");
    }
    if let Some(w) = &f.wallet_id {
        sql.push_str(" AND u.wallet_id = ?");
        binds.push(Box::new(w.clone()));
    }
    if let Some(m) = f.min_amount_sats {
        sql.push_str(" AND u.amount_sats >= ?");
        binds.push(Box::new(m));
    }
    if let Some(m) = f.max_amount_sats {
        sql.push_str(" AND u.amount_sats <= ?");
        binds.push(Box::new(m));
    }
    if let Some(frozen) = f.is_frozen {
        sql.push_str(" AND u.is_frozen = ?");
        binds.push(Box::new(frozen as i64));
    }
    if let Some(l) = &f.label_contains {
        sql.push_str(" AND u.label LIKE ?");
        binds.push(Box::new(format!("%{l}%")));
    }
    if !f.include_archived.unwrap_or(false) {
        sql.push_str(" AND w.archived = 0");
    }
    sql.push_str(" ORDER BY u.amount_sats DESC");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let utxos = stmt
        .query_map(param_refs.as_slice(), row_to_utxo)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(utxos)
}

#[tauri::command]
pub fn freeze_utxo(state: tauri::State<AppState>, id: String, frozen: bool) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("UPDATE utxos SET is_frozen = ?1 WHERE id = ?2", params![frozen as i64, id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn label_utxo(state: tauri::State<AppState>, id: String, label: Option<String>) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("UPDATE utxos SET label = ?1 WHERE id = ?2", params![label, id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Validate a cost-basis lot. A lot is a date AND a cost together; giving
/// only one is an error, giving neither clears it. Cost is a plain decimal
/// in the user's fiat currency.
pub fn validate_basis(
    cost_fiat: Option<f64>,
    acquired_at: Option<String>,
    note: Option<String>,
) -> Result<(Option<f64>, Option<String>, Option<String>), String> {
    let date = acquired_at.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    let note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    if let Some(n) = &note {
        if n.chars().count() > 500 {
            return Err("note is too long (500 characters max)".into());
        }
    }
    match (cost_fiat, date) {
        (None, None) => Ok((None, None, note)),
        (Some(c), Some(d)) => {
            if !c.is_finite() || c < 0.0 {
                return Err("cost basis must be a number that is zero or more".into());
            }
            chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").map_err(|_| "acquired date must look like YYYY-MM-DD".to_string())?;
            Ok((Some(c), Some(d), note))
        }
        (Some(_), None) => Err("add the date you acquired this coin along with its cost".into()),
        (None, Some(_)) => Err("add the cost basis along with the acquired date".into()),
    }
}

/// Record (or clear, with both empty) the cost-basis lot for one coin. Never
/// deletes the UTXO row.
#[tauri::command]
pub fn set_utxo_basis(
    state: tauri::State<AppState>,
    id: String,
    cost_fiat: Option<f64>,
    acquired_at: Option<String>,
    note: Option<String>,
) -> Result<(), String> {
    let (cost, date, note) = validate_basis(cost_fiat, acquired_at, note)?;
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let changed = conn
        .execute(
            "UPDATE utxos SET acquisition_cost_fiat = ?1, acquisition_date = ?2, acquisition_note = ?3 WHERE id = ?4",
            params![cost, date, note, id],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("that coin is no longer in this wallet (sync may have removed it)".into());
    }
    Ok(())
}

#[derive(Debug, Serialize, PartialEq)]
pub struct TaxReadiness {
    /// True when there is at least one coin and every one has a lot.
    pub ready: bool,
    pub total_utxos: i64,
    pub with_basis: i64,
    pub missing: i64,
}

pub fn tax_readiness_from_counts(total_utxos: i64, with_basis: i64) -> TaxReadiness {
    TaxReadiness { ready: total_utxos > 0 && with_basis >= total_utxos, total_utxos, with_basis, missing: (total_utxos - with_basis).max(0) }
}

/// Counts over unspent coins in unarchived wallets. "With basis" means the
/// coin has both a cost and an acquired date.
pub(crate) fn tax_counts(conn: &rusqlite::Connection) -> Result<(i64, i64), String> {
    conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN u.acquisition_cost_fiat IS NOT NULL AND u.acquisition_date IS NOT NULL THEN 1 ELSE 0 END), 0)
         FROM utxos u JOIN wallets w ON w.id = u.wallet_id
         WHERE u.spent = 0 AND w.archived = 0",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_tax_readiness(state: tauri::State<AppState>) -> Result<TaxReadiness, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let (total, with) = tax_counts(&conn)?;
    Ok(tax_readiness_from_counts(total, with))
}

#[derive(Debug, Serialize)]
pub struct SizeBucket {
    pub bucket: String,
    pub count: i64,
    pub total_sats: i64,
}

#[derive(Debug, Serialize)]
pub struct UtxoDistribution {
    pub by_size: Vec<SizeBucket>,
    pub total_utxos: i64,
    pub total_sats: i64,
    pub dust_count: i64,
    pub average_amount_sats: i64,
}

/// Amounts of live coins (a coin a scan stopped reporting is `spent = 1`), optionally for one wallet.
pub(crate) fn live_amounts(conn: &rusqlite::Connection, wallet_id: Option<&str>, include_archived: bool) -> Result<Vec<i64>, String> {
    let archived_clause = if include_archived { "" } else { " AND w.archived = 0" };
    let sql = match wallet_id {
        Some(_) => format!(
            "SELECT u.amount_sats FROM utxos u JOIN wallets w ON w.id = u.wallet_id \
             WHERE u.spent = 0 AND u.wallet_id = ?1{archived_clause}"
        ),
        None => format!(
            "SELECT u.amount_sats FROM utxos u JOIN wallets w ON w.id = u.wallet_id \
             WHERE u.spent = 0{archived_clause}"
        ),
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_vec: Vec<&dyn rusqlite::ToSql> = match &wallet_id {
        Some(w) => vec![w],
        None => vec![],
    };
    let amounts = stmt
        .query_map(params_vec.as_slice(), |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(amounts)
}

#[tauri::command]
pub fn utxo_distribution(
    state: tauri::State<AppState>,
    wallet_id: Option<String>,
    include_archived: Option<bool>,
) -> Result<UtxoDistribution, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    let amounts = live_amounts(&conn, wallet_id.as_deref(), include_archived.unwrap_or(false))?;

    let mut dust = (0i64, 0i64);
    let mut small = (0i64, 0i64);
    let mut medium = (0i64, 0i64);
    let mut large = (0i64, 0i64);
    let mut whale = (0i64, 0i64);

    for &a in &amounts {
        let bucket = if a < DUST_THRESHOLD_SATS {
            &mut dust
        } else if a < SMALL_THRESHOLD_SATS {
            &mut small
        } else if a < MEDIUM_THRESHOLD_SATS {
            &mut medium
        } else if a < LARGE_THRESHOLD_SATS {
            &mut large
        } else {
            &mut whale
        };
        bucket.0 += 1;
        bucket.1 += a;
    }

    let total_sats: i64 = amounts.iter().sum();
    let total_utxos = amounts.len() as i64;

    Ok(UtxoDistribution {
        by_size: vec![
            SizeBucket { bucket: "dust".into(), count: dust.0, total_sats: dust.1 },
            SizeBucket { bucket: "small".into(), count: small.0, total_sats: small.1 },
            SizeBucket { bucket: "medium".into(), count: medium.0, total_sats: medium.1 },
            SizeBucket { bucket: "large".into(), count: large.0, total_sats: large.1 },
            SizeBucket { bucket: "whale".into(), count: whale.0, total_sats: whale.1 },
        ],
        total_utxos,
        total_sats,
        dust_count: dust.0,
        average_amount_sats: if total_utxos > 0 { total_sats / total_utxos } else { 0 },
    })
}

fn addr_for_utxo(conn: &rusqlite::Connection, utxo: &Utxo) -> Result<String, String> {
    if let Some(addr_id) = &utxo.address_id {
        conn.query_row("SELECT address FROM addresses WHERE id = ?1", params![addr_id], |r| r.get(0))
            .map_err(|e| e.to_string())
    } else {
        Err(format!("utxo {} has no associated address on file", utxo.id))
    }
}

/// Resolve the PSBT signing origin for a UTXO: the wallet's wildcard
/// descriptor (external or internal, matching the address' keychain) plus
/// this address' concrete derivation index — but only when that descriptor
/// actually has a `[fingerprint/path]` origin embedded (see
/// `bitcoin::descriptors::descriptors_from_extended_key` /
/// `descriptors_from_multisig`). Returns `None` — never a guessed value —
/// when origin isn't on file, e.g. for address-only wallets (no key at
/// all) or xpub wallets added without a fingerprint/path.
fn origin_for_utxo(conn: &rusqlite::Connection, utxo: &Utxo) -> Result<Option<PsbtInputOrigin>, String> {
    let Some(addr_id) = &utxo.address_id else { return Ok(None) };

    let addr_row: Option<(String, Option<i64>)> = conn
        .query_row(
            "SELECT keychain, address_index FROM addresses WHERE id = ?1",
            params![addr_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((keychain, Some(index))) = addr_row else { return Ok(None) };

    let (external, internal): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT descriptor, change_descriptor FROM wallets WHERE id = ?1",
            params![utxo.wallet_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    let wildcard_descriptor = match keychain.as_str() {
        "internal" => internal.or(external),
        _ => external,
    };
    let Some(wildcard_descriptor) = wildcard_descriptor else { return Ok(None) };
    if !wildcard_descriptor.contains('[') {
        // No key origin embedded — this wallet was added without a
        // fingerprint/derivation path (or is address-only). Refuse to
        // guess one.
        return Ok(None);
    }

    Ok(Some(PsbtInputOrigin { wildcard_descriptor, derivation_index: index as u32 }))
}

fn wallet_name(conn: &rusqlite::Connection, wallet_id: &str) -> Result<String, String> {
    conn.query_row("SELECT name FROM wallets WHERE id = ?1", params![wallet_id], |r| r.get(0))
        .map_err(|e| e.to_string())
}

/// Build one `PsbtInput` per selected UTXO, including its signing origin
/// when known. Does not itself fail on a missing origin — that decision
/// belongs to `build_unsigned_psbt`, which names the specific input.
/// Coins a spend may use: live (a coin a scan stopped reporting is `spent = 1`) and not frozen.
pub(crate) fn spend_candidates(conn: &rusqlite::Connection, wallet_id: &str) -> Result<Vec<Utxo>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM utxos WHERE wallet_id = ?1 AND spent = 0 AND is_frozen = 0")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![wallet_id], row_to_utxo)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Small, live, unfrozen coins, smallest first.
pub(crate) fn consolidation_candidates(conn: &rusqlite::Connection, wallet_id: &str) -> Result<Vec<Utxo>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT * FROM utxos WHERE wallet_id = ?1 AND spent = 0 AND is_frozen = 0
             AND amount_sats < ?2 ORDER BY amount_sats ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![wallet_id, SMALL_THRESHOLD_SATS], row_to_utxo)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn psbt_inputs_for(conn: &rusqlite::Connection, utxos: &[Utxo]) -> Result<Vec<PsbtInput>, String> {
    utxos
        .iter()
        .map(|u| {
            Ok(PsbtInput {
                txid: u.txid.clone(),
                vout: u.vout as u32,
                amount_sats: u.amount_sats as u64,
                address: addr_for_utxo(conn, u)?,
                origin: origin_for_utxo(conn, u)?,
            })
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct ConsolidationPlan {
    pub wallet_id: String,
    pub input_count: usize,
    pub total_input_sats: u64,
    pub estimated_fee_sats: u64,
    pub estimated_vbytes: f64,
    pub fee_rate_sat_per_vb: f64,
    pub consolidated_output_sats: u64,
    pub destination_address: String,
    pub psbt_base64: String,
}

#[tauri::command]
pub fn plan_consolidation(
    state: tauri::State<AppState>,
    wallet_id: String,
    fee_rate_sat_per_vb: f64,
    max_inputs: Option<usize>,
) -> Result<ConsolidationPlan, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    plan_consolidation_inner(&conn, wallet_id, fee_rate_sat_per_vb, max_inputs)
}

/// Where a consolidation pays: an address of this wallet that has never received coins, with
/// its key origin. Same order as change in `plan_spend`: a stored unused change-chain address,
/// else the next one derived from the change descriptor, else a stored unused receive address.
/// Never an address that already holds or held coins, and never one without an origin (a signer
/// would show the output as an external send).
fn consolidation_destination(conn: &rusqlite::Connection, wallet_id: &str, net: bitcoin::Network) -> Result<ChangeDest, String> {
    let dest = stored_unused_address(conn, wallet_id, "internal")
        .or_else(|| derive_next_change_address(conn, wallet_id, net))
        .or_else(|| stored_unused_address(conn, wallet_id, "external"))
        .ok_or("no unused address to consolidate into: this wallet has no change descriptor and no unused receive address on file. Nakos will not pay a consolidation to an address that already holds coins")?;
    if dest.origin.is_none() {
        return Err("the destination address has no key origin (the wallet's descriptor has no [fingerprint/path]), so a signer would show it as an external send. Add the wallet with its master fingerprint and derivation path".into());
    }
    Ok(dest)
}

fn plan_consolidation_inner(
    conn: &rusqlite::Connection,
    wallet_id: String,
    fee_rate_sat_per_vb: f64,
    max_inputs: Option<usize>,
) -> Result<ConsolidationPlan, String> {
    let network: String = conn
        .query_row("SELECT network FROM wallets WHERE id = ?1", params![wallet_id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let net = match network.as_str() {
        "testnet" => bitcoin::Network::Testnet,
        "signet" => bitcoin::Network::Signet,
        "regtest" => bitcoin::Network::Regtest,
        _ => bitcoin::Network::Bitcoin,
    };

    let mut candidates = consolidation_candidates(conn, &wallet_id)?;

    if let Some(max) = max_inputs {
        candidates.truncate(max);
    }
    if candidates.len() < 2 {
        return Err("fewer than 2 consolidation-eligible UTXOs (dust/small, unfrozen) found in this wallet".into());
    }

    let dest = consolidation_destination(conn, &wallet_id, net)?;
    let destination = dest.address.clone();

    let psbt_inputs = psbt_inputs_for(conn, &candidates)?;
    let wallet_label = wallet_name(conn, &wallet_id)?;

    let total_input: u64 = candidates.iter().map(|u| u.amount_sats as u64).sum();
    let vbytes = crate::bitcoin::psbt::estimate_vbytes(
        &psbt_inputs,
        &[PsbtOutput { address: destination.clone(), amount_sats: 0, origin: None }],
        net,
    )
    .map_err(|e| e.to_string())?;
    let fee = (vbytes * fee_rate_sat_per_vb).ceil() as u64;
    let output_amount = total_input.saturating_sub(fee);
    if output_amount == 0 {
        return Err("estimated fee exceeds total value of selected UTXOs".into());
    }

    let built = build_unsigned_psbt(
        &psbt_inputs,
        &[PsbtOutput { address: destination.clone(), amount_sats: output_amount, origin: dest.origin }],
        net,
        &wallet_label,
    )
    .map_err(|e| e.to_string())?;

    Ok(ConsolidationPlan {
        wallet_id,
        input_count: candidates.len(),
        total_input_sats: total_input,
        estimated_fee_sats: built.fee_sats,
        estimated_vbytes: built.estimated_vbytes,
        fee_rate_sat_per_vb,
        consolidated_output_sats: output_amount,
        destination_address: destination,
        psbt_base64: built.base64,
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum SpendOptimization {
    #[serde(rename = "tax")]
    Tax,
    #[serde(rename = "privacy")]
    Privacy,
    #[serde(rename = "fee")]
    Fee,
    #[serde(rename = "fragmentation")]
    Fragmentation,
}

/// A change destination plus the descriptor/index that produced it (the
/// origin is what lets Sparrow/hardware recognise the output as change).
struct ChangeDest {
    address: String,
    origin: Option<PsbtInputOrigin>,
}

/// Origin for an address of this wallet on `keychain` at `index`, from the
/// stored descriptor for that chain. `None` when that descriptor is missing
/// or has no `[fingerprint/path]` — never a guessed value (same rule as
/// `origin_for_utxo` for inputs).
fn wildcard_origin(conn: &rusqlite::Connection, wallet_id: &str, keychain: &str, index: u32) -> Option<PsbtInputOrigin> {
    let (external, internal): (Option<String>, Option<String>) = conn
        .query_row("SELECT descriptor, change_descriptor FROM wallets WHERE id = ?1", params![wallet_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()?;
    let wildcard_descriptor = if keychain == "internal" { internal? } else { external? };
    wildcard_descriptor
        .contains('[')
        .then_some(PsbtInputOrigin { wildcard_descriptor, derivation_index: index })
}

/// Stored unused address on `keychain`, with its origin.
fn stored_unused_address(conn: &rusqlite::Connection, wallet_id: &str, keychain: &str) -> Option<ChangeDest> {
    let (address, index): (String, Option<i64>) = conn
        .query_row(
            "SELECT address, address_index FROM addresses WHERE wallet_id = ?1 AND keychain = ?2 AND is_used = 0 ORDER BY address_index ASC LIMIT 1",
            params![wallet_id, keychain],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()?;
    let origin = index.and_then(|i| wildcard_origin(conn, wallet_id, keychain, i as u32));
    Some(ChangeDest { address, origin })
}

/// Next change address for a wallet: derived from its stored change
/// descriptor at one past the highest change index we've ever seen an output
/// on (the addresses table only holds addresses that received coins, so
/// "unused" rows never exist). Watch-only: public derivation only. Returns
/// `None` when the wallet has no change descriptor.
fn derive_next_change_address(conn: &rusqlite::Connection, wallet_id: &str, net: bitcoin::Network) -> Option<ChangeDest> {
    let (external, internal): (Option<String>, Option<String>) = conn
        .query_row("SELECT descriptor, change_descriptor FROM wallets WHERE id = ?1", params![wallet_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()?;
    let (external, internal) = (external?, internal?);
    let next_index: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(address_index) + 1, 0) FROM addresses WHERE wallet_id = ?1 AND keychain = 'internal'",
            params![wallet_id],
            |r| r.get::<_, i64>(0),
        )
        .ok()? as u32;
    let wallet = bdk_wallet::Wallet::create(external, internal).network(net).create_wallet_no_persist().ok()?;
    let address = wallet.peek_address(bdk_wallet::KeychainKind::Internal, next_index).address.to_string();
    Some(ChangeDest { address, origin: wildcard_origin(conn, wallet_id, "internal", next_index) })
}

/// Coins in the order the selector chose them (the order basis is consumed in).
fn in_selection_order(candidates: &[Utxo], ids: &[String]) -> Vec<Utxo> {
    ids.iter().filter_map(|id| candidates.iter().find(|u| &u.id == id).cloned()).collect()
}

/// Estimated realized gain for a plan, in fiat. Not tax advice.
///
/// Only `outgoing_sats` (send amount + miner fee) leave the wallet; change
/// comes back to you and is not a sale. Walking the inputs in selection
/// order, each consumes sats toward `outgoing_sats`, and its cost basis is
/// allocated pro rata:
///
///   cost_consumed(input) = input_cost * (sats_consumed / input_sats)
///   gain = outgoing_sats/1e8 * price_per_btc - sum(cost_consumed)
///
/// Returns `None` (never a guess) when there is no price, nothing goes out,
/// or any consumed input has no recorded cost, since a missing cost must not
/// be treated as zero. Cost and price are both in the user's fiat currency.
pub fn realized_gain_fiat(inputs: &[(u64, Option<f64>)], outgoing_sats: u64, price_per_btc: f64) -> Option<f64> {
    let total: u64 = inputs.iter().map(|(sats, _)| sats).sum();
    if outgoing_sats == 0 || outgoing_sats > total || !price_per_btc.is_finite() {
        return None;
    }
    let mut remaining = outgoing_sats;
    let mut cost_consumed = 0.0;
    for (sats, cost) in inputs {
        if remaining == 0 {
            break;
        }
        let used = remaining.min(*sats);
        if used > 0 {
            cost_consumed += (*cost)? * (used as f64 / *sats as f64);
        }
        remaining -= used;
    }
    Some(outgoing_sats as f64 / 100_000_000.0 * price_per_btc - cost_consumed)
}

#[derive(Debug, Serialize)]
pub struct SpendPlan {
    pub selected_utxo_ids: Vec<String>,
    pub input_count: usize,
    pub total_input_sats: u64,
    pub send_amount_sats: u64,
    pub estimated_fee_sats: u64,
    pub change_sats: u64,
    pub estimated_realized_gain_fiat: Option<f64>,
    pub optimization: String,
    pub psbt_base64: String,
    pub change_address: Option<String>,
    /// One-line explanation of what the selected policy did.
    pub selection_note: String,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn plan_spend(
    state: tauri::State<AppState>,
    wallet_id: String,
    destination_address: String,
    amount_sats: u64,
    optimization: SpendOptimization,
    fee_rate_sat_per_vb: f64,
    current_price_fiat_per_btc: Option<f64>,
) -> Result<SpendPlan, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    plan_spend_inner(&conn, wallet_id, destination_address, amount_sats, optimization, fee_rate_sat_per_vb, current_price_fiat_per_btc)
}

fn plan_spend_inner(
    conn: &rusqlite::Connection,
    wallet_id: String,
    destination_address: String,
    amount_sats: u64,
    optimization: SpendOptimization,
    fee_rate_sat_per_vb: f64,
    current_price_fiat_per_btc: Option<f64>,
) -> Result<SpendPlan, String> {
    let network: String = conn
        .query_row("SELECT network FROM wallets WHERE id = ?1", params![wallet_id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let net = match network.as_str() {
        "testnet" => bitcoin::Network::Testnet,
        "signet" => bitcoin::Network::Signet,
        "regtest" => bitcoin::Network::Regtest,
        _ => bitcoin::Network::Bitcoin,
    };

    let candidates = spend_candidates(conn, &wallet_id)?;

    let mut selection_types: Option<(bool, Vec<&'static str>)> = None;
    let total_spendable = candidates.len();
    let selected: Vec<Utxo> = match optimization {
        // Lowest tax impact: HIFO over coins that have a recorded lot. A coin
        // without a lot is never used, so every input in the plan has one.
        SpendOptimization::Tax => {
            let cands: Vec<Candidate> = candidates
                .iter()
                .map(|u| Candidate {
                    id: u.id.clone(),
                    amount_sats: u.amount_sats as u64,
                    script: addr_for_utxo(conn, u).map(|a| script_class(&a)).unwrap_or("unknown"),
                })
                .collect();
            let lots: Vec<Lot> = candidates
                .iter()
                .filter_map(|u| match (u.acquisition_cost_fiat, &u.acquisition_date) {
                    (Some(c), Some(d)) => Some(Lot { id: u.id.clone(), cost_fiat: c, acquired: d.clone() }),
                    _ => None,
                })
                .collect();
            let sel = coin_select::select_hifo(&cands, &lots, amount_sats, fee_rate_sat_per_vb)?;
            selection_types = Some((sel.single_script_type, sel.script_types.clone()));
            in_selection_order(&candidates, &sel.ids)
        }
        // Fee and Privacy use different policies; see bitcoin::coin_select.
        SpendOptimization::Privacy | SpendOptimization::Fee => {
            let policy = if matches!(optimization, SpendOptimization::Fee) { Policy::Fee } else { Policy::Privacy };
            let cands: Vec<Candidate> = candidates
                .iter()
                .map(|u| Candidate {
                    id: u.id.clone(),
                    amount_sats: u.amount_sats as u64,
                    script: addr_for_utxo(conn, u).map(|a| script_class(&a)).unwrap_or("unknown"),
                })
                .collect();
            let sel = coin_select::select(&cands, amount_sats, fee_rate_sat_per_vb, policy)
                .ok_or("insufficient spendable (unfrozen) balance in this wallet to cover amount plus fee")?;
            selection_types = Some((sel.single_script_type, sel.script_types.clone()));
            in_selection_order(&candidates, &sel.ids)
        }
        // Clean up dust: consume smallest UTXOs first, until amount plus fee is covered.
        SpendOptimization::Fragmentation => {
            let cands: Vec<Candidate> = candidates
                .iter()
                .map(|u| Candidate {
                    id: u.id.clone(),
                    amount_sats: u.amount_sats as u64,
                    script: addr_for_utxo(conn, u).map(|a| script_class(&a)).unwrap_or("unknown"),
                })
                .collect();
            let sel = coin_select::select_smallest_first(&cands, amount_sats, fee_rate_sat_per_vb)
                .ok_or("insufficient spendable (unfrozen) balance in this wallet to cover amount plus fee")?;
            in_selection_order(&candidates, &sel.ids)
        }
    };
    let selected_total: u64 = selected.iter().map(|u| u.amount_sats as u64).sum();
    if selected_total < amount_sats {
        return Err("insufficient spendable (unfrozen) balance in this wallet".into());
    }

    let psbt_inputs = psbt_inputs_for(conn, &selected)?;
    let wallet_label = wallet_name(conn, &wallet_id)?;

    // Prefer a change-chain address (stored, else derived from the change
    // descriptor). Only if the wallet has none, fall back to an unused receive
    // address (still ours) rather than dropping change to the miners.
    let internal_dest = stored_unused_address(conn, &wallet_id, "internal")
        .or_else(|| derive_next_change_address(conn, &wallet_id, net));
    let change_from_receive = internal_dest.is_none();
    let change_candidate: Option<ChangeDest> = internal_dest.or_else(|| stored_unused_address(conn, &wallet_id, "external"));

    let mut outputs = vec![PsbtOutput { address: destination_address.clone(), amount_sats, origin: None }];
    if let Some(dest) = &change_candidate {
        outputs.push(PsbtOutput { address: dest.address.clone(), amount_sats: 0, origin: None });
    }

    let vbytes = crate::bitcoin::psbt::estimate_vbytes(&psbt_inputs, &outputs, net).map_err(|e| e.to_string())?;
    let fee = (vbytes * fee_rate_sat_per_vb).ceil() as u64;

    if selected_total < amount_sats + fee {
        return Err("selected inputs can't cover amount plus fee — lower the amount or fee rate".into());
    }
    let mut change_sats = selected_total - amount_sats - fee;
    let mut change_address: Option<String> = None;
    let mut change_has_origin = false;
    let mut final_outputs = vec![PsbtOutput { address: destination_address, amount_sats, origin: None }];
    if change_sats >= CHANGE_DUST_SATS {
        // Never burn real change: no address on file means no plan.
        let dest = change_candidate
            .ok_or("no unused address on file to receive change — sync this wallet first, or spend an amount that leaves no change")?;
        change_has_origin = dest.origin.is_some();
        final_outputs.push(PsbtOutput { address: dest.address.clone(), amount_sats: change_sats, origin: dest.origin });
        change_address = Some(dest.address);
    } else {
        // Sub-dust remainder is added to the fee; report it as such.
        change_sats = 0;
    }

    let built = build_unsigned_psbt(&psbt_inputs, &final_outputs, net, &wallet_label).map_err(|e| e.to_string())?;

    // Only the sats that actually leave the wallet (send + miner fee, which
    // also covers any sub-dust change folded into the fee) are a disposal.
    // The change output is not a sale. See `realized_gain_fiat`.
    let outgoing_sats = amount_sats + built.fee_sats;
    let realized_gain = current_price_fiat_per_btc.and_then(|price| {
        let lots: Vec<(u64, Option<f64>)> = selected.iter().map(|u| (u.amount_sats as u64, u.acquisition_cost_fiat)).collect();
        realized_gain_fiat(&lots, outgoing_sats, price)
    });

    let n = selected.len();
    let selection_note = {
        let inputs = format!("{n} input{}, ~{:.0} vB", if n == 1 { "" } else { "s" }, vbytes);
        let base = match optimization {
            SpendOptimization::Fee => format!("Fee: {inputs} (fewest inputs, largest first; heuristic sizes, no fee API)"),
            SpendOptimization::Privacy => match &selection_types {
                Some((true, t)) => format!("Privacy: {inputs}, same script type ({})", t.join(", ")),
                _ => format!("Privacy: {inputs}, mixed script types (no single type could pay)"),
            },
            SpendOptimization::Fragmentation => format!("Fragmentation: {inputs}, smallest first"),
            SpendOptimization::Tax => format!("Lowest tax impact: {inputs} (HIFO: highest cost per sat first; whole coins only, no holding-period or wash-sale rules; not tax advice)"),
        };
        let base = if change_address.is_some() && change_from_receive {
            format!("{base}; change goes to a receive-chain address (no change-chain address on file)")
        } else {
            base
        };
        let base = if change_address.is_some() && !change_has_origin {
            format!("{base}; change output has no key origin, so Sparrow may show it as an external send")
        } else {
            base
        };
        if total_spendable == 1 && matches!(optimization, SpendOptimization::Fee | SpendOptimization::Privacy) {
            format!("{base} — only one spendable UTXO, so Fee and Privacy give the same plan")
        } else {
            base
        }
    };

    Ok(SpendPlan {
        selected_utxo_ids: selected.iter().map(|u| u.id.clone()).collect(),
        input_count: selected.len(),
        total_input_sats: selected_total,
        send_amount_sats: amount_sats,
        estimated_fee_sats: built.fee_sats,
        change_sats,
        estimated_realized_gain_fiat: realized_gain,
        optimization: match optimization {
            SpendOptimization::Tax => "tax",
            SpendOptimization::Privacy => "privacy",
            SpendOptimization::Fee => "fee",
            SpendOptimization::Fragmentation => "fragmentation",
        }
        .to_string(),
        psbt_base64: built.base64,
        change_address,
        selection_note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> rusqlite::Connection {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn
    }

    fn add_wallet_row(conn: &rusqlite::Connection, id: &str, archived: bool) {
        conn.execute(
            "INSERT INTO wallets (id, name, custody_type, source_type, archived) VALUES (?1, ?1, 'cold_storage', 'xpub', ?2)",
            params![id, archived as i64],
        )
        .unwrap();
    }

    fn add_utxo(conn: &rusqlite::Connection, id: &str, wallet: &str, vout: i64, basis: Option<(f64, &str)>) {
        conn.execute(
            "INSERT INTO utxos (id, wallet_id, txid, vout, amount_sats, spent, acquisition_cost_fiat, acquisition_date)
             VALUES (?1, ?2, ?3, ?4, 10000, 0, ?5, ?6)",
            params![id, wallet, "cd".repeat(32), vout, basis.map(|b| b.0), basis.map(|b| b.1)],
        )
        .unwrap();
    }

    #[test]
    fn basis_needs_a_cost_and_a_date_together() {
        assert!(validate_basis(Some(100.0), Some("2024-01-31".into()), None).is_ok());
        assert_eq!(validate_basis(None, None, None).unwrap(), (None, None, None), "both empty clears the lot");
        assert!(validate_basis(Some(100.0), None, None).unwrap_err().contains("date"));
        assert!(validate_basis(None, Some("2024-01-31".into()), None).unwrap_err().contains("cost"));
        assert!(validate_basis(Some(-1.0), Some("2024-01-31".into()), None).is_err());
        assert!(validate_basis(Some(f64::NAN), Some("2024-01-31".into()), None).is_err());
        assert!(validate_basis(Some(1.0), Some("31/01/2024".into()), None).unwrap_err().contains("YYYY-MM-DD"));
        assert!(validate_basis(Some(1.0), Some("2024-01-31".into()), Some("x".repeat(501))).is_err());
    }

    #[test]
    fn readiness_counts_only_unarchived_unspent_coins_with_a_full_lot() {
        let conn = mem_db();
        add_wallet_row(&conn, "live", false);
        add_wallet_row(&conn, "old", true);
        add_utxo(&conn, "a", "live", 0, Some((50.0, "2024-01-01")));
        add_utxo(&conn, "b", "live", 1, None);
        add_utxo(&conn, "c", "old", 0, None); // archived wallet: ignored
        // A half-filled lot (cost, no date) does not count.
        conn.execute("UPDATE utxos SET acquisition_cost_fiat = 5.0 WHERE id = 'b'", []).unwrap();

        let (total, with) = tax_counts(&conn).unwrap();
        assert_eq!((total, with), (2, 1));
        let r = tax_readiness_from_counts(total, with);
        assert!(!r.ready && r.missing == 1);

        conn.execute("UPDATE utxos SET acquisition_date = '2024-02-02' WHERE id = 'b'", []).unwrap();
        let (total, with) = tax_counts(&conn).unwrap();
        let r = tax_readiness_from_counts(total, with);
        assert!(r.ready && r.missing == 0);
    }

    #[test]
    fn no_coins_is_not_ready_and_score_is_none_until_a_lot_exists() {
        assert!(!tax_readiness_from_counts(0, 0).ready);
        assert_eq!(crate::scoring::health::tax_readiness_option(3, 0), None, "no lots: nothing to score");
        assert_eq!(crate::scoring::health::tax_readiness_option(3, 3), Some(100), "all lots: not None");
        assert_eq!(crate::scoring::health::tax_readiness_option(4, 1), Some(25));
    }

    #[test]
    fn change_is_not_a_sale_cost_is_allocated_to_what_leaves() {
        // One 6010-sat coin costing $10.00; send 5000, fee 141, change 869.
        let cost = 10.0;
        let price = 60_000.0; // $/BTC
        let gain = realized_gain_fiat(&[(6_010, Some(cost))], 5_000 + 141, price).unwrap();
        let cost_consumed = cost * (5_141.0 / 6_010.0);
        let expected = 5_141.0 / 100_000_000.0 * price - cost_consumed;
        assert!((gain - expected).abs() < 1e-9, "gain {gain} vs {expected}");
        // The old behaviour charged the whole coin and valued all 6010 sats.
        let old = 6_010.0 / 100_000_000.0 * price - cost;
        assert!((gain - old).abs() > 1e-6, "must differ from treating the full input as sold");
        assert!(cost_consumed < cost);
    }

    #[test]
    fn no_change_plan_consumes_the_full_input_cost() {
        let gain = realized_gain_fiat(&[(6_010, Some(10.0))], 6_010, 60_000.0).unwrap();
        let expected = 6_010.0 / 100_000_000.0 * 60_000.0 - 10.0;
        assert!((gain - expected).abs() < 1e-9);
    }

    #[test]
    fn multiple_inputs_consume_in_selection_order_and_the_last_is_partial() {
        // A (4000 sats, $8) fully consumed, then B (4000 sats, $20) partially: 1000 of 4000.
        let gain = realized_gain_fiat(&[(4_000, Some(8.0)), (4_000, Some(20.0))], 5_000, 50_000.0).unwrap();
        let cost_consumed = 8.0 + 20.0 * 0.25;
        let expected = 5_000.0 / 100_000_000.0 * 50_000.0 - cost_consumed;
        assert!((gain - expected).abs() < 1e-9);
    }

    #[test]
    fn no_number_without_a_price_or_a_cost_on_every_consumed_input() {
        assert_eq!(realized_gain_fiat(&[(6_010, None)], 5_000, 60_000.0), None, "missing cost is never treated as zero");
        assert_eq!(realized_gain_fiat(&[(4_000, Some(8.0)), (4_000, None)], 3_000, 60_000.0).is_some(), true, "an input that isn't reached doesn't need a cost");
        assert_eq!(realized_gain_fiat(&[(4_000, Some(8.0)), (4_000, None)], 5_000, 60_000.0), None);
        assert_eq!(realized_gain_fiat(&[(6_010, Some(1.0))], 5_000, f64::NAN), None);
        assert_eq!(realized_gain_fiat(&[(6_010, Some(1.0))], 0, 60_000.0), None);
    }

    #[test]
    fn selection_order_is_the_selectors_order_not_database_order() {
        let mk = |id: &str| Utxo {
            id: id.into(),
            wallet_id: "w".into(),
            address_id: None,
            txid: "t".into(),
            vout: 0,
            amount_sats: 1,
            confirmations: 0,
            block_height: None,
            block_time: None,
            is_change: false,
            is_frozen: false,
            label: None,
            acquisition_cost_fiat: None,
            acquisition_date: None,
            acquisition_note: None,
            privacy_risk_score: None,
            spent: false,
            spent_txid: None,
            spent_at: None,
            unseen_since: None,
            created_at: String::new(),
        };
        let db_order = vec![mk("a"), mk("b"), mk("c")];
        let picked = in_selection_order(&db_order, &["c".to_string(), "a".to_string()]);
        assert_eq!(picked.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(), vec!["c", "a"]);
    }

    // ---- R6: consolidation must not pay to an address that already holds coins ----

    // Published BIP-32 test-vector public key (public test data, not a real wallet).
    const XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";

    struct Fixture {
        conn: rusqlite::Connection,
        external: String,
        internal: String,
    }

    fn derived(external: &str, internal: &str, kind: bdk_wallet::KeychainKind, index: u32) -> String {
        let w = bdk_wallet::Wallet::create(external.to_string(), internal.to_string())
            .network(bitcoin::Network::Bitcoin)
            .create_wallet_no_persist()
            .unwrap();
        w.peek_address(kind, index).address.to_string()
    }

    impl Fixture {
        /// A synced-looking wallet: receive addresses 0 and 1 hold a small coin each, change
        /// address 0 was seen before. `with_change` = false stores no change descriptor.
        fn new(with_change: bool) -> Fixture {
            let conn = mem_db();
            let pair = crate::bitcoin::descriptors::descriptors_from_extended_key(XPUB, bitcoin::Network::Bitcoin, Some(("aabbccdd", "84'/0'/0'"))).unwrap();
            conn.execute(
                "INSERT INTO wallets (id, name, custody_type, source_type, network, descriptor, change_descriptor)
                 VALUES ('w', 'test wallet', 'cold_storage', 'xpub', 'bitcoin', ?1, ?2)",
                params![pair.external, with_change.then_some(pair.internal.clone())],
            )
            .unwrap();
            let f = Fixture { conn, external: pair.external, internal: pair.internal };
            for (i, sats) in [(0u32, 20_000i64), (1, 30_000)] {
                f.add_address("external", i, true);
                f.conn
                    .execute(
                        "INSERT INTO utxos (id, wallet_id, address_id, txid, vout, amount_sats, spent) VALUES (?1,'w',?2,?3,0,?4,0)",
                        params![format!("u{i}"), format!("a-external-{i}"), format!("{:02x}", 0x20 + i).repeat(32), sats],
                    )
                    .unwrap();
            }
            f.add_address("internal", 0, true);
            f
        }

        fn add_address(&self, keychain: &str, index: u32, used: bool) -> String {
            let kind = if keychain == "internal" { bdk_wallet::KeychainKind::Internal } else { bdk_wallet::KeychainKind::External };
            let address = derived(&self.external, &self.internal, kind, index);
            self.conn
                .execute(
                    "INSERT INTO addresses (id, wallet_id, address, keychain, address_index, is_used) VALUES (?1,'w',?2,?3,?4,?5)",
                    params![format!("a-{keychain}-{index}"), address, keychain, index, used as i64],
                )
                .unwrap();
            address
        }

        fn used_addresses(&self) -> Vec<String> {
            let mut stmt = self.conn.prepare("SELECT address FROM addresses WHERE is_used = 1").unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
        }

        fn plan(&self) -> Result<ConsolidationPlan, String> {
            plan_consolidation_inner(&self.conn, "w".into(), 2.0, None)
        }
    }

    /// (fingerprint, path) pairs on the PSBT's first output.
    fn output_origins(plan: &ConsolidationPlan) -> Vec<(String, String)> {
        use bitcoin::base64::{engine::general_purpose::STANDARD, Engine};
        let psbt = bitcoin::psbt::Psbt::deserialize(&STANDARD.decode(&plan.psbt_base64).unwrap()).unwrap();
        psbt.outputs[0].bip32_derivation.values().map(|(fp, path)| (fp.to_string(), path.to_string())).collect()
    }

    #[test]
    fn consolidation_pays_to_a_fresh_change_address_and_labels_it_as_ours() {
        let f = Fixture::new(true);
        let plan = f.plan().unwrap();
        assert!(!f.used_addresses().contains(&plan.destination_address), "paid to an address that already received coins");
        assert_eq!(plan.destination_address, derived(&f.external, &f.internal, bdk_wallet::KeychainKind::Internal, 1), "next change address after the one seen");
        assert_eq!(output_origins(&plan), vec![("aabbccdd".to_string(), "84'/0'/0'/1/1".to_string())], "the output carries its key origin, so a signer shows it as change");
        assert_eq!(plan.input_count, 2);
    }

    #[test]
    fn a_stored_unused_change_address_is_preferred() {
        let f = Fixture::new(true);
        let stored = f.add_address("internal", 7, false);
        let plan = f.plan().unwrap();
        assert_eq!(plan.destination_address, stored);
        assert_eq!(output_origins(&plan)[0].1, "84'/0'/0'/1/7");
    }

    #[test]
    fn with_no_change_descriptor_only_an_unused_receive_address_is_used() {
        let f = Fixture::new(false);
        let stored = f.add_address("external", 2, false);
        let plan = f.plan().unwrap();
        assert_eq!(plan.destination_address, stored);
        assert_eq!(output_origins(&plan)[0].1, "84'/0'/0'/0/2");
    }

    #[test]
    fn it_refuses_instead_of_falling_back_to_a_used_address() {
        // The reported case: nothing unused is derivable, and the old code paid to the most
        // recently seen address (one that already holds coins).
        let f = Fixture::new(false);
        let err = f.plan().unwrap_err();
        assert!(err.contains("no unused address to consolidate into"), "{err}");
    }

    #[test]
    fn it_refuses_a_destination_without_a_key_origin() {
        let f = Fixture::new(true);
        // Change descriptor with its [fingerprint/path] removed: the address derives, but a signer
        // would not know it is ours.
        let bare = f.internal.replace("[aabbccdd/84'/0'/0']", "");
        assert_ne!(bare, f.internal);
        let bare = bare.split('#').next().unwrap().to_string();
        f.conn.execute("UPDATE wallets SET change_descriptor = ?1", params![bare]).unwrap();
        let err = f.plan().unwrap_err();
        assert!(err.contains("no key origin"), "{err}");
    }

    // ---- R10: Fragmentation must count the fee when it picks coins ----

    #[test]
    fn fragmentation_adds_a_coin_when_the_smallest_coins_only_equal_the_amount() {
        let f = Fixture::new(true); // coins of 20_000 and 30_000 sats
        f.add_address("external", 2, true);
        f.conn
            .execute(
                "INSERT INTO utxos (id, wallet_id, address_id, txid, vout, amount_sats, spent) VALUES ('u2','w','a-external-2',?1,0,100000,0)",
                params!["22".repeat(32)],
            )
            .unwrap();
        let dest = derived(&f.external, &f.internal, bdk_wallet::KeychainKind::External, 9);
        let plan = |rate: f64| plan_spend_inner(&f.conn, "w".into(), dest.clone(), 49_500, SpendOptimization::Fragmentation, rate, None);

        // 20_000 + 30_000 = 50_000 covers 49_500 on its own, but not the ~2_000 sat fee at 10 sat/vB:
        // the old code picked those two and then failed with "can't cover amount plus fee".
        let p = plan(10.0).expect("a third coin must be added instead of failing");
        assert_eq!(p.input_count, 3);
        assert!(p.estimated_fee_sats >= 2_000, "{}", p.estimated_fee_sats);
        assert_eq!(p.total_input_sats, 150_000);
        assert!(p.change_sats > 90_000);

        // At 1 sat/vB the two smallest coins do cover it, and nothing more is taken.
        let p = plan(1.0).unwrap();
        assert_eq!(p.input_count, 2);
        assert_eq!(p.total_input_sats, 50_000);
    }

    #[test]
    fn fragmentation_says_so_when_every_coin_together_cannot_pay_the_fee() {
        let f = Fixture::new(true); // 50_000 sats in total
        let dest = derived(&f.external, &f.internal, bdk_wallet::KeychainKind::External, 9);
        let err = plan_spend_inner(&f.conn, "w".into(), dest, 49_900, SpendOptimization::Fragmentation, 10.0, None).unwrap_err();
        assert!(err.contains("to cover amount plus fee"), "{err}");
    }
}
