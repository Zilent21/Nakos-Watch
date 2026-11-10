//! Alerts: things the person asked to be told about, worked out from data Nakos already has after
//! a sync or an address-list check. Nothing here sends coins; a balance alert only says "this
//! wallet is over N sats", and moving them is up to the person.
//!
//! - balance_over: a wallet's balance crossed up past the person's threshold (once per crossing);
//! - receive_paid: an address handed out by Receive got coins;
//! - saved_used: a saved address got used;
//! - list_low: a saved-address list is down to its "running low" number of fresh addresses.

use crate::state::AppState;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct AlertEvent {
    pub id: String,
    pub kind: String,
    pub wallet_id: Option<String>,
    pub list_id: Option<String>,
    pub message: String,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BalanceAlert {
    pub wallet_id: String,
    pub threshold_sats: i64,
    pub is_above: bool,
}

fn db_err(e: rusqlite::Error) -> String {
    e.to_string()
}

/// 1234567 → "1,234,567".
pub(crate) fn sats(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn short(addr: &str) -> String {
    if addr.len() <= 16 { addr.to_string() } else { format!("{}…{}", &addr[..8], &addr[addr.len() - 6..]) }
}

fn insert(conn: &Connection, kind: &str, wallet_id: Option<&str>, list_id: Option<&str>, ref_id: Option<&str>, message: &str) -> Result<usize, String> {
    conn.execute(
        "INSERT OR IGNORE INTO alert_events (id, kind, wallet_id, list_id, ref_id, message) VALUES (?1,?2,?3,?4,?5,?6)",
        params![Uuid::new_v4().to_string(), kind, wallet_id, list_id, ref_id, message],
    )
    .map_err(db_err)
}

fn balances(conn: &Connection) -> Result<HashMap<String, i64>, String> {
    Ok(crate::commands::wallets::balance_rows(conn, true)?.into_iter().map(|r| (r.wallet_id, r.balance_sats)).collect())
}

/// Works out new alerts from the database as it is now. Returns how many were added.
pub(crate) fn evaluate(conn: &Connection) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let conn: &Connection = &tx;
    let mut added = 0;

    // Balance crossings.
    let bal = balances(conn)?;
    let mut stmt = conn
        .prepare("SELECT b.wallet_id, b.threshold_sats, b.is_above, w.name FROM balance_alerts b JOIN wallets w ON w.id = b.wallet_id WHERE w.archived = 0")
        .map_err(db_err)?;
    let rules = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?)))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    drop(stmt);
    for (wallet_id, threshold, was_above, name) in rules {
        let now = bal.get(&wallet_id).copied().unwrap_or(0);
        let above = now >= threshold;
        if above && !was_above {
            added += insert(conn, "balance_over", Some(&wallet_id), None, None, &format!("{name} is over {} sats: it holds {} sats now.", sats(threshold), sats(now)))?;
        }
        if above != was_above {
            conn.execute("UPDATE balance_alerts SET is_above = ?2 WHERE wallet_id = ?1", params![wallet_id, above]).map_err(db_err)?;
        }
    }

    // Payments to addresses handed out by Receive.
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.wallet_id, w.name, r.address_index, r.label,
                    COALESCE((SELECT SUM(u.amount_sats) FROM utxos u WHERE u.address_id = a.id), 0)
             FROM receive_addresses r JOIN wallets w ON w.id = r.wallet_id
             JOIN addresses a ON a.wallet_id = r.wallet_id AND a.address = r.address AND a.is_used = 1
             WHERE NOT EXISTS (SELECT 1 FROM alert_events e WHERE e.kind = 'receive_paid' AND e.ref_id = r.id)",
        )
        .map_err(db_err)?;
    let paid = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, i64>(5)?)))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    drop(stmt);
    for (id, wallet_id, name, index, label, received) in paid {
        let label = label.map(|l| format!(" ({l})")).unwrap_or_default();
        let amount = if received > 0 { format!("{} sats", sats(received)) } else { "coins".into() };
        added += insert(conn, "receive_paid", Some(&wallet_id), None, Some(&id), &format!("{name}: address #{index}{label} received {amount}."))?;
    }

    // Saved addresses that got used.
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.list_id, l.name, s.address, s.label, s.received_sats FROM saved_addresses s JOIN address_lists l ON l.id = s.list_id
             WHERE s.used_at IS NOT NULL AND NOT EXISTS (SELECT 1 FROM alert_events e WHERE e.kind = 'saved_used' AND e.ref_id = s.id)",
        )
        .map_err(db_err)?;
    let used = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<i64>>(5)?)))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    drop(stmt);
    for (id, list_id, name, address, label, received) in used {
        let label = label.map(|l| format!(" ({l})")).unwrap_or_default();
        let amount = received.filter(|r| *r > 0).map(|r| format!(", {} sats received", sats(r))).unwrap_or_default();
        added += insert(conn, "saved_used", None, Some(&list_id), Some(&id), &format!("{name}: {}{label} was used{amount}.", short(&address)))?;
    }

    // Lists running low. low_alerted is cleared once the list is topped up again.
    let mut stmt = conn
        .prepare(
            "SELECT l.id, l.name, l.low_at, l.low_alerted,
                    (SELECT COUNT(*) FROM saved_addresses s WHERE s.list_id = l.id AND s.used_at IS NULL AND s.given_out_at IS NULL),
                    (SELECT COUNT(*) FROM saved_addresses s WHERE s.list_id = l.id)
             FROM address_lists l",
        )
        .map_err(db_err)?;
    let lists = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, bool>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?)))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    drop(stmt);
    for (id, name, low_at, alerted, fresh, total) in lists {
        let low = total > 0 && fresh <= low_at;
        if low && !alerted {
            let what = match fresh {
                0 => "no fresh addresses left".to_string(),
                1 => "only 1 fresh address left".to_string(),
                n => format!("only {n} fresh addresses left"),
            };
            // A fresh id each time: running low again after a top-up is a new alert.
            let ref_id = Uuid::new_v4().to_string();
            added += insert(conn, "list_low", None, Some(&id), Some(&ref_id), &format!("{name}: {what}. Add more from that wallet the next time you open it."))?;
        }
        if low != alerted {
            conn.execute("UPDATE address_lists SET low_alerted = ?2 WHERE id = ?1", params![id, low]).map_err(db_err)?;
        }
    }

    tx.commit().map_err(db_err)?;
    Ok(added)
}

fn conn_of(state: &AppState) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, String> {
    state.pool()?.get().map_err(|e| e.to_string())
}

pub(crate) fn events(conn: &Connection, include_read: bool) -> Result<Vec<AlertEvent>, String> {
    let sql = format!(
        "SELECT id, kind, wallet_id, list_id, message, created_at, read_at FROM alert_events {} ORDER BY created_at DESC LIMIT 100",
        if include_read { "" } else { "WHERE read_at IS NULL" }
    );
    let mut stmt = conn.prepare(&sql).map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| Ok(AlertEvent { id: r.get(0)?, kind: r.get(1)?, wallet_id: r.get(2)?, list_id: r.get(3)?, message: r.get(4)?, created_at: r.get(5)?, read_at: r.get(6)? }))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err);
    rows
}

#[tauri::command]
pub fn list_alerts(state: tauri::State<AppState>, include_read: Option<bool>) -> Result<Vec<AlertEvent>, String> {
    events(&*conn_of(&state)?, include_read.unwrap_or(false))
}

/// Marks the given alerts read, or every unread one when `ids` is None.
#[tauri::command]
pub fn mark_alerts_read(state: tauri::State<AppState>, ids: Option<Vec<String>>) -> Result<(), String> {
    let conn = conn_of(&state)?;
    match ids {
        None => conn.execute("UPDATE alert_events SET read_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE read_at IS NULL", []).map(|_| ()),
        Some(ids) => ids.iter().try_for_each(|id| {
            conn.execute("UPDATE alert_events SET read_at = COALESCE(read_at, strftime('%Y-%m-%dT%H:%M:%fZ','now')) WHERE id = ?1", params![id]).map(|_| ())
        }),
    }
    .map_err(db_err)
}

#[tauri::command]
pub fn list_balance_alerts(state: tauri::State<AppState>) -> Result<Vec<BalanceAlert>, String> {
    let conn = conn_of(&state)?;
    let mut stmt = conn.prepare("SELECT wallet_id, threshold_sats, is_above FROM balance_alerts").map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| Ok(BalanceAlert { wallet_id: r.get(0)?, threshold_sats: r.get(1)?, is_above: r.get(2)? }))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err);
    rows
}

pub(crate) fn set_balance(conn: &Connection, wallet_id: &str, threshold_sats: Option<i64>) -> Result<(), String> {
    match threshold_sats {
        None => {
            conn.execute("DELETE FROM balance_alerts WHERE wallet_id = ?1", params![wallet_id]).map_err(db_err)?;
        }
        Some(t) if t <= 0 || !crate::commands::wallets::plausible_sats(t) => return Err("the amount must be more than 0 and at most 21 million BTC".into()),
        Some(t) => {
            // Starts from the balance as it is now, so setting a line below it does not fire at once.
            let now = balances(conn)?.get(wallet_id).copied().unwrap_or(0);
            conn.execute(
                "INSERT INTO balance_alerts (wallet_id, threshold_sats, is_above) VALUES (?1,?2,?3)
                 ON CONFLICT(wallet_id) DO UPDATE SET threshold_sats = excluded.threshold_sats, is_above = excluded.is_above",
                params![wallet_id, t, now >= t],
            )
            .map_err(db_err)?;
        }
    }
    Ok(())
}

/// `threshold_sats` None removes the alert.
#[tauri::command]
pub fn set_balance_alert(state: tauri::State<AppState>, wallet_id: String, threshold_sats: Option<i64>) -> Result<(), String> {
    set_balance(&*conn_of(&state)?, &wallet_id, threshold_sats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn.execute("INSERT INTO wallets (id,name,custody_type,source_type,last_synced_at) VALUES ('w','Hot','hot_wallet','zpub','x')", []).unwrap();
        conn
    }

    fn set_coins(conn: &Connection, total: i64) {
        conn.execute("DELETE FROM utxos", []).unwrap();
        conn.execute("INSERT INTO utxos (id,wallet_id,txid,vout,amount_sats) VALUES ('u','w','tx',0,?1)", params![total]).unwrap();
    }

    fn kinds(conn: &Connection) -> Vec<String> {
        events(conn, true).unwrap().into_iter().map(|e| e.kind).collect()
    }

    #[test]
    fn sats_have_thousands_separators() {
        assert_eq!(sats(0), "0");
        assert_eq!(sats(999), "999");
        assert_eq!(sats(1_000), "1,000");
        assert_eq!(sats(2_100_000_000_000_000), "2,100,000,000,000,000");
        assert_eq!(sats(-21_000), "-21,000");
    }

    #[test]
    fn a_balance_alert_fires_once_per_upward_crossing() {
        let conn = db();
        set_coins(&conn, 500_000);
        set_balance(&conn, "w", Some(1_000_000)).unwrap();
        assert_eq!(evaluate(&conn).unwrap(), 0);
        set_coins(&conn, 1_200_000);
        assert_eq!(evaluate(&conn).unwrap(), 1);
        assert_eq!(evaluate(&conn).unwrap(), 0, "still above: no repeat");
        set_coins(&conn, 100_000);
        evaluate(&conn).unwrap();
        set_coins(&conn, 1_000_000);
        assert_eq!(evaluate(&conn).unwrap(), 1, "crossed up again");
        let e = &events(&conn, false).unwrap()[0];
        assert_eq!(e.message, "Hot is over 1,000,000 sats: it holds 1,000,000 sats now.");
    }

    #[test]
    fn setting_a_line_below_the_balance_does_not_fire_at_once() {
        let conn = db();
        set_coins(&conn, 2_000_000);
        set_balance(&conn, "w", Some(1_000_000)).unwrap();
        assert_eq!(evaluate(&conn).unwrap(), 0);
        set_balance(&conn, "w", None).unwrap();
        assert!(set_balance(&conn, "w", Some(0)).is_err());
    }

    #[test]
    fn a_paid_receive_address_alerts_once() {
        let conn = db();
        conn.execute("INSERT INTO receive_addresses (id, wallet_id, address_index, address, label) VALUES ('r','w',7,'bc1qx','from Kraken')", []).unwrap();
        assert_eq!(evaluate(&conn).unwrap(), 0);
        conn.execute("INSERT INTO addresses (id, wallet_id, address, keychain, address_index, is_used) VALUES ('a','w','bc1qx','external',7,1)", []).unwrap();
        conn.execute("INSERT INTO utxos (id,wallet_id,address_id,txid,vout,amount_sats) VALUES ('u','w','a','tx',0,250000)", []).unwrap();
        assert_eq!(evaluate(&conn).unwrap(), 1);
        assert_eq!(evaluate(&conn).unwrap(), 0);
        assert_eq!(events(&conn, false).unwrap()[0].message, "Hot: address #7 (from Kraken) received 250,000 sats.");
    }

    #[test]
    fn saved_addresses_alert_when_used_and_lists_when_low() {
        let conn = db();
        conn.execute("INSERT INTO address_lists (id, name, low_at) VALUES ('L','Cake',1)", []).unwrap();
        for i in 0..3 {
            conn.execute("INSERT INTO saved_addresses (id, list_id, address, position) VALUES (?1,'L',?2,?3)", params![format!("s{i}"), format!("bc1qaddress{i}xxxxxxxxxx"), i]).unwrap();
        }
        assert_eq!(evaluate(&conn).unwrap(), 0);
        conn.execute("UPDATE saved_addresses SET given_out_at = 'x' WHERE id IN ('s0','s1')", []).unwrap();
        conn.execute("UPDATE saved_addresses SET used_at = 'x', received_sats = 40000 WHERE id = 's0'", []).unwrap();
        assert_eq!(evaluate(&conn).unwrap(), 2);
        assert_eq!(kinds(&conn).iter().filter(|k| *k == "list_low").count(), 1);
        assert_eq!(evaluate(&conn).unwrap(), 0, "already told");
        // Topped up, then low again: a new alert.
        conn.execute("INSERT INTO saved_addresses (id, list_id, address, position) VALUES ('s9','L','bc1qnew',9)", []).unwrap();
        evaluate(&conn).unwrap();
        conn.execute("UPDATE saved_addresses SET given_out_at = 'x' WHERE id IN ('s2','s9')", []).unwrap();
        assert_eq!(evaluate(&conn).unwrap(), 1);
        let msgs: Vec<String> = events(&conn, true).unwrap().into_iter().map(|e| e.message).collect();
        assert!(msgs.iter().any(|m| m == "Cake: bc1qaddr…xxxxxx was used, 40,000 sats received."), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.starts_with("Cake: no fresh addresses left.")), "{msgs:?}");
    }
}
