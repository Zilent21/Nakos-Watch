//! Saved receiving addresses: lists of addresses the person copied from their own hot or cold
//! wallets (Cake, Sparrow, a Jade…), so they can hand one out without opening that wallet. Nakos
//! never derives these, never sends anything, and talks to no exchange.
//!
//! Each list holds up to 20 unused addresses. "Use next" hands out the first fresh one and marks
//! it given out, so two payers never get the same address. Checking a list asks the Electrum
//! server for each address's history: any history means "used".

use crate::state::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

pub(crate) const MAX_UNUSED: i64 = 20;
const MAX_NAME: usize = 60;
const MAX_LABEL: usize = 100;
/// Transactions fetched per address to total what it received. Hand-out addresses see one or two.
const MAX_TX_PER_ADDRESS: usize = 25;

#[derive(Debug, Serialize, Clone)]
pub struct SavedAddress {
    pub id: String,
    pub address: String,
    pub label: Option<String>,
    pub position: i64,
    pub given_out_at: Option<String>,
    pub used_at: Option<String>,
    pub received_sats: Option<i64>,
    pub device_checked_at: Option<String>,
    pub created_at: String,
    /// fresh | given_out | used
    pub state: &'static str,
}

#[derive(Debug, Serialize)]
pub struct AddressList {
    pub id: String,
    pub name: String,
    pub network: String,
    pub low_at: i64,
    pub last_checked_at: Option<String>,
    pub fresh: i64,
    pub given_out: i64,
    pub used: i64,
    pub low: bool,
    pub addresses: Vec<SavedAddress>,
}

#[derive(Debug, Serialize)]
pub struct Skipped {
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct AddResult {
    pub added: usize,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub checked: usize,
    pub newly_used: usize,
    pub new_alerts: usize,
}

fn db_err(e: rusqlite::Error) -> String {
    e.to_string()
}

fn saved_row(r: &rusqlite::Row) -> rusqlite::Result<SavedAddress> {
    let given: Option<String> = r.get(4)?;
    let used: Option<String> = r.get(5)?;
    let state = if used.is_some() { "used" } else if given.is_some() { "given_out" } else { "fresh" };
    Ok(SavedAddress {
        id: r.get(0)?,
        address: r.get(1)?,
        label: r.get(2)?,
        position: r.get(3)?,
        given_out_at: given,
        used_at: used,
        received_sats: r.get(6)?,
        device_checked_at: r.get(7)?,
        created_at: r.get(8)?,
        state,
    })
}

const SAVED_SQL: &str = "SELECT id, address, label, position, given_out_at, used_at, received_sats, device_checked_at, created_at FROM saved_addresses";

fn saved_one(conn: &Connection, id: &str) -> Result<SavedAddress, String> {
    conn.query_row(&format!("{SAVED_SQL} WHERE id = ?1"), params![id], saved_row).map_err(db_err)
}

pub(crate) fn lists(conn: &Connection) -> Result<Vec<AddressList>, String> {
    let mut stmt = conn.prepare("SELECT id, name, network, low_at, last_checked_at FROM address_lists ORDER BY created_at").map_err(db_err)?;
    let heads = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?)))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    let mut out = Vec::new();
    for (id, name, network, low_at, last_checked_at) in heads {
        let mut s = conn.prepare(&format!("{SAVED_SQL} WHERE list_id = ?1 ORDER BY used_at IS NOT NULL, position")).map_err(db_err)?;
        let addresses = s.query_map(params![id], saved_row).map_err(db_err)?.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        let count = |st: &str| addresses.iter().filter(|a| a.state == st).count() as i64;
        let fresh = count("fresh");
        out.push(AddressList {
            low: !addresses.is_empty() && fresh <= low_at,
            fresh,
            given_out: count("given_out"),
            used: count("used"),
            id,
            name,
            network,
            low_at,
            last_checked_at,
            addresses,
        });
    }
    Ok(out)
}

fn clean_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME {
        return Err(format!("give the list a name of 1 to {MAX_NAME} characters, such as the wallet's name"));
    }
    crate::commands::wallets::refuse_private_key_material([Some(n)])?;
    Ok(n.to_string())
}

fn check_network(network: &str) -> Result<(), String> {
    match network {
        "bitcoin" | "testnet" | "signet" | "regtest" => Ok(()),
        _ => Err("unknown network".into()),
    }
}

/// "bitcoin:ADDR?amount=…" → "ADDR"; anything else is returned as it is.
fn strip_uri(token: &str) -> &str {
    let t = token.trim();
    match t.get(..8) {
        Some(s) if s.eq_ignore_ascii_case("bitcoin:") => t[8..].split('?').next().unwrap_or(""),
        _ => t,
    }
}

pub(crate) fn add(conn: &Connection, list_id: &str, text: &str) -> Result<AddResult, String> {
    // A pasted seed phrase or private key is refused whole, before any of it is stored.
    crate::commands::wallets::refuse_private_key_material([Some(text)])?;
    let tokens: Vec<&str> = text.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|t| !t.trim().is_empty()).collect();
    crate::commands::wallets::refuse_private_key_material(tokens.iter().map(|t| Some(*t)))?;
    if tokens.is_empty() {
        return Err("paste one or more receiving addresses, one per line".into());
    }

    let network: String = conn
        .query_row("SELECT network FROM address_lists WHERE id = ?1", params![list_id], |r| r.get(0))
        .optional()
        .map_err(db_err)?
        .ok_or("no such list")?;
    let net = crate::bitcoin::sync::parse_network(&network);
    let mut unused: i64 = conn.query_row("SELECT COUNT(*) FROM saved_addresses WHERE list_id = ?1 AND used_at IS NULL", params![list_id], |r| r.get(0)).map_err(db_err)?;
    let mut position: i64 = conn.query_row("SELECT COALESCE(MAX(position), 0) FROM saved_addresses WHERE list_id = ?1", params![list_id], |r| r.get(0)).map_err(db_err)?;

    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut added = 0;
    let mut skipped = Vec::new();
    for token in tokens {
        let shown: String = token.chars().take(80).collect();
        let address = match crate::bitcoin::descriptors::validate_address(strip_uri(token), net) {
            Ok(a) => a.to_string(),
            Err(_) => {
                skipped.push(Skipped { text: shown, reason: format!("not a {} address", if network == "bitcoin" { "bitcoin" } else { network.as_str() }) });
                continue;
            }
        };
        let elsewhere: Option<String> = tx
            .query_row(
                "SELECT l.name FROM saved_addresses s JOIN address_lists l ON l.id = s.list_id WHERE s.address = ?1 LIMIT 1",
                params![address],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if let Some(name) = elsewhere {
            skipped.push(Skipped { text: shown, reason: format!("already saved in \"{name}\"") });
            continue;
        }
        if unused >= MAX_UNUSED {
            skipped.push(Skipped { text: shown, reason: format!("this list already holds {MAX_UNUSED} unused addresses") });
            continue;
        }
        position += 1;
        tx.execute(
            "INSERT INTO saved_addresses (id, list_id, address, position) VALUES (?1,?2,?3,?4)",
            params![Uuid::new_v4().to_string(), list_id, address, position],
        )
        .map_err(db_err)?;
        unused += 1;
        added += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(AddResult { added, skipped })
}

pub(crate) fn give_out_next(conn: &Connection, list_id: &str) -> Result<SavedAddress, String> {
    let next: Option<(String, String)> = conn
        .query_row(
            "SELECT s.id, l.name FROM saved_addresses s JOIN address_lists l ON l.id = s.list_id
             WHERE s.list_id = ?1 AND s.used_at IS NULL AND s.given_out_at IS NULL ORDER BY s.position LIMIT 1",
            params![list_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_err)?;
    let Some((id, _)) = next else {
        let name: String = conn.query_row("SELECT name FROM address_lists WHERE id = ?1", params![list_id], |r| r.get(0)).optional().map_err(db_err)?.ok_or("no such list")?;
        return Err(format!("No fresh addresses left in \"{name}\". Add more from that wallet the next time you open it."));
    };
    conn.execute("UPDATE saved_addresses SET given_out_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1", params![id]).map_err(db_err)?;
    saved_one(conn, &id)
}

/// What the server said about one address.
pub(crate) struct Lookup {
    pub id: String,
    pub used: bool,
    pub received_sats: u64,
}

/// Write a finished lookup (network-free, testable). Returns how many addresses became used.
pub(crate) fn apply_lookups(conn: &Connection, lookups: &[Lookup]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut newly_used = 0;
    for l in lookups.iter().filter(|l| l.used) {
        let n = tx
            .execute(
                "UPDATE saved_addresses SET used_at = COALESCE(used_at, strftime('%Y-%m-%dT%H:%M:%fZ','now')), received_sats = ?2 WHERE id = ?1 AND used_at IS NULL",
                params![l.id, l.received_sats as i64],
            )
            .map_err(db_err)?;
        newly_used += n;
        tx.execute("UPDATE saved_addresses SET received_sats = ?2 WHERE id = ?1", params![l.id, l.received_sats as i64]).map_err(db_err)?;
    }
    tx.execute("UPDATE address_lists SET last_checked_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')", []).map_err(db_err)?;
    tx.commit().map_err(db_err)?;
    Ok(newly_used)
}

/// Ask the Electrum server about every saved address. Blocking; run off the UI thread.
fn lookup_all(conn: &Connection) -> Result<Vec<Lookup>, String> {
    use electrum_client::ElectrumApi;
    let mut stmt = conn.prepare("SELECT s.id, s.address, l.network FROM saved_addresses s JOIN address_lists l ON l.id = s.list_id").map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let cfg = crate::commands::node::default_electrum_config(conn)?
        .ok_or("No Electrum server set up yet. Add your own, or a public one, on the Node page.")?;
    crate::commands::node::public_server_gate(conn, &cfg)?;
    let client = cfg.connect().map_err(|e| e.to_string())?;

    let mut parsed = Vec::new();
    for (id, addr, network) in rows {
        // A saved address always passed this check; skip quietly if it no longer does.
        if let Ok(a) = crate::bitcoin::descriptors::validate_address(&addr, crate::bitcoin::sync::parse_network(&network)) {
            parsed.push((id, a.script_pubkey()));
        }
    }
    let scripts: Vec<&bitcoin::Script> = parsed.iter().map(|(_, s)| s.as_script()).collect();
    let histories = client.batch_script_get_history(scripts.iter().copied()).map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for ((id, spk), history) in parsed.iter().zip(histories) {
        let txids: Vec<bitcoin::Txid> = history.iter().take(MAX_TX_PER_ADDRESS).map(|h| h.tx_hash).collect();
        let received_sats = if txids.is_empty() {
            0
        } else {
            let txs = client.batch_transaction_get(txids.iter()).map_err(|e| e.to_string())?;
            txs.iter().flat_map(|t| t.output.iter()).filter(|o| &o.script_pubkey == spk).map(|o| o.value.to_sat()).sum()
        };
        out.push(Lookup { id: id.clone(), used: !history.is_empty(), received_sats });
    }
    Ok(out)
}

fn conn_of(state: &AppState) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, String> {
    state.pool()?.get().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_address_lists(state: tauri::State<AppState>) -> Result<Vec<AddressList>, String> {
    lists(&*conn_of(&state)?)
}

#[tauri::command]
pub fn create_address_list(state: tauri::State<AppState>, name: String, network: Option<String>) -> Result<String, String> {
    let conn = conn_of(&state)?;
    let name = clean_name(&name)?;
    let network = network.unwrap_or_else(|| "bitcoin".into());
    check_network(&network)?;
    let id = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO address_lists (id, name, network) VALUES (?1,?2,?3)", params![id, name, network]).map_err(db_err)?;
    Ok(id)
}

#[tauri::command]
pub fn update_address_list(state: tauri::State<AppState>, id: String, name: String, low_at: i64) -> Result<(), String> {
    let conn = conn_of(&state)?;
    let name = clean_name(&name)?;
    if !(0..MAX_UNUSED).contains(&low_at) {
        return Err(format!("\"running low\" must be from 0 to {}", MAX_UNUSED - 1));
    }
    conn.execute("UPDATE address_lists SET name = ?2, low_at = ?3 WHERE id = ?1", params![id, name, low_at]).map_err(db_err)?;
    Ok(())
}

/// Deletes the list and its addresses: Nakos stops watching them.
#[tauri::command]
pub fn delete_address_list(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    conn_of(&state)?.execute("DELETE FROM address_lists WHERE id = ?1", params![id]).map_err(db_err)?;
    Ok(())
}

#[tauri::command]
pub fn add_saved_addresses(state: tauri::State<AppState>, list_id: String, text: String) -> Result<AddResult, String> {
    add(&*conn_of(&state)?, &list_id, &text)
}

/// Stops watching one address. Coins sent to it later still arrive in the wallet it came from;
/// Nakos just won't see them.
#[tauri::command]
pub fn delete_saved_address(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    conn_of(&state)?.execute("DELETE FROM saved_addresses WHERE id = ?1", params![id]).map_err(db_err)?;
    Ok(())
}

#[tauri::command]
pub fn give_out_next_address(state: tauri::State<AppState>, list_id: String) -> Result<SavedAddress, String> {
    give_out_next(&*conn_of(&state)?, &list_id)
}

#[tauri::command]
pub fn update_saved_address(state: tauri::State<AppState>, id: String, label: Option<String>) -> Result<SavedAddress, String> {
    let conn = conn_of(&state)?;
    let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
    if let Some(l) = &label {
        if l.chars().count() > MAX_LABEL {
            return Err(format!("keep the label under {MAX_LABEL} characters"));
        }
        crate::commands::wallets::refuse_private_key_material([Some(l.as_str())])?;
    }
    conn.execute("UPDATE saved_addresses SET label = ?2 WHERE id = ?1", params![id, label]).map_err(db_err)?;
    saved_one(&conn, &id)
}

/// The person compared this address with the one their wallet app or device shows (or undoes that).
#[tauri::command]
pub fn set_saved_checked(state: tauri::State<AppState>, id: String, checked: bool) -> Result<SavedAddress, String> {
    let conn = conn_of(&state)?;
    conn.execute(
        "UPDATE saved_addresses SET device_checked_at = CASE WHEN ?2 THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE NULL END WHERE id = ?1",
        params![id, checked],
    )
    .map_err(db_err)?;
    saved_one(&conn, &id)
}

/// Async: the Electrum round trips take seconds, and must never freeze the window.
#[tauri::command]
pub async fn check_address_lists(state: tauri::State<'_, AppState>) -> Result<CheckResult, String> {
    let pool = state.pool()?;
    let lock = state.sync_lock();
    tokio::task::spawn_blocking(move || {
        let _one_at_a_time = lock.lock();
        let conn = pool.get().map_err(|e| e.to_string())?;
        let lookups = lookup_all(&conn)?;
        let newly_used = apply_lookups(&conn, &lookups)?;
        let new_alerts = crate::commands::alerts::evaluate(&conn)?;
        Ok(CheckResult { checked: lookups.len(), newly_used, new_alerts })
    })
    .await
    .map_err(|e| format!("the check stopped unexpectedly: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const A0: &str = "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu";
    pub(crate) const A1: &str = "bc1qnjg0jd8228aq7egyzacy8cys3knf9xvrerkf9g";
    pub(crate) const A2: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn.execute("INSERT INTO address_lists (id, name) VALUES ('L','Cake wallet')", []).unwrap();
        conn
    }

    #[test]
    fn pasting_addresses_adds_the_good_ones_and_says_why_others_were_skipped() {
        let conn = db();
        let text = format!("{A0}\n  bitcoin:{}?amount=0.01 \nnot-an-address\n{A0}\n{}", A1.to_uppercase(), A2);
        let r = add(&conn, "L", &text).unwrap();
        assert_eq!(r.added, 3);
        assert_eq!(r.skipped.len(), 2);
        assert!(r.skipped[0].reason.contains("not a bitcoin address"));
        assert!(r.skipped[1].reason.contains("already saved"));
        let l = &lists(&conn).unwrap()[0];
        assert_eq!(l.addresses.iter().map(|a| a.address.as_str()).collect::<Vec<_>>(), vec![A0, A1, A2]);
        assert_eq!((l.fresh, l.low), (3, true));
    }

    #[test]
    fn a_seed_phrase_or_private_key_is_refused_whole() {
        let conn = db();
        let seed = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        assert!(add(&conn, "L", seed).unwrap_err().contains("seed phrase"));
        let wif = format!("{A0}\nL1aW4aubDFB7yfras2S1mN3bqg9nwySY8nkoLmJebSLD5BWv3ENZ");
        assert!(add(&conn, "L", &wif).is_err());
        assert!(lists(&conn).unwrap()[0].addresses.is_empty(), "nothing from a refused paste is stored");
        assert!(add(&conn, "L", "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx").unwrap().skipped[0].reason.contains("not a bitcoin"));
    }

    #[test]
    fn a_list_holds_twenty_unused_addresses() {
        let conn = db();
        conn.execute("INSERT INTO address_lists (id, name) VALUES ('M','Other')", []).unwrap();
        for i in 0..20 {
            conn.execute("INSERT INTO saved_addresses (id, list_id, address, position) VALUES (?1,'L',?2,?3)", params![format!("s{i}"), format!("x{i}"), i]).unwrap();
        }
        let r = add(&conn, "L", A0).unwrap();
        assert_eq!(r.added, 0);
        assert!(r.skipped[0].reason.contains("20 unused"));
        conn.execute("UPDATE saved_addresses SET used_at = 'x' WHERE id = 's0'", []).unwrap();
        assert_eq!(add(&conn, "L", A0).unwrap().added, 1, "a used address frees a place");
        assert!(add(&conn, "M", A0).unwrap().skipped[0].reason.contains("Cake wallet"));
    }

    #[test]
    fn use_next_hands_each_address_out_once_in_order() {
        let conn = db();
        add(&conn, "L", &format!("{A0}\n{A1}")).unwrap();
        let a = give_out_next(&conn, "L").unwrap();
        let b = give_out_next(&conn, "L").unwrap();
        assert_eq!((a.address.as_str(), a.state, b.address.as_str()), (A0, "given_out", A1));
        assert!(give_out_next(&conn, "L").unwrap_err().contains("No fresh addresses left in \"Cake wallet\""));
    }

    #[test]
    fn a_lookup_marks_used_addresses_once_and_keeps_the_total_current() {
        let conn = db();
        add(&conn, "L", &format!("{A0}\n{A1}")).unwrap();
        let ids: Vec<String> = lists(&conn).unwrap()[0].addresses.iter().map(|a| a.id.clone()).collect();
        let n = apply_lookups(&conn, &[Lookup { id: ids[0].clone(), used: true, received_sats: 50_000 }, Lookup { id: ids[1].clone(), used: false, received_sats: 0 }]).unwrap();
        assert_eq!(n, 1);
        let again = apply_lookups(&conn, &[Lookup { id: ids[0].clone(), used: true, received_sats: 80_000 }]).unwrap();
        assert_eq!(again, 0, "already used");
        let l = &lists(&conn).unwrap()[0];
        let used = l.addresses.iter().find(|a| a.id == ids[0]).unwrap();
        assert_eq!((used.state, used.received_sats), ("used", Some(80_000)));
        assert_eq!((l.fresh, l.used), (1, 1));
        assert!(l.last_checked_at.is_some());
    }
}
