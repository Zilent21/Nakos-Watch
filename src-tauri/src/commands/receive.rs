//! Receive: fresh addresses from a wallet's own descriptor, so coins can go to cold storage
//! without plugging in the hardware wallet. Public-key math only: the descriptor holds public
//! keys, and nothing here can sign or spend.
//!
//! An index is handed out once. The next index is one past the highest index that is either used
//! on chain or already handed out, and Nakos stops at the wallet's gap limit of handed-out unused
//! addresses, past which other wallet apps may not look for coins.
//!
//! The one risk a screen cannot remove is a swapped address (a tampered computer or vault). Each
//! address therefore carries "checked on device" or not, and the page covers the QR code for
//! amounts at or over the person's threshold until they check it on the device.

use crate::db::models::Wallet;
use crate::state::AppState;
use bdk_wallet::miniscript::{Descriptor, DescriptorPublicKey, ForEachKey};
use bdk_wallet::{KeychainKind, Wallet as BdkWallet};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;

/// Wallets with a descriptor to derive from. Address and manual wallets have none.
const RECEIVE_SOURCES: [&str; 5] = ["descriptor", "xpub", "ypub", "zpub", "multisig_descriptor"];
const MAX_LABEL: usize = 100;

#[derive(Debug, Serialize, Clone)]
pub struct ReceiveAddress {
    pub id: String,
    pub index: u32,
    pub address: String,
    /// "m/84'/0'/0'/0/7" when the descriptor gives the origin, else "…/0/7".
    pub path: String,
    pub label: Option<String>,
    pub amount_sats: Option<i64>,
    pub device_checked_at: Option<String>,
    pub created_at: String,
    /// Seen on chain at the last sync.
    pub used: bool,
    /// Coins this address received that the last sync saw (spent ones included).
    pub received_sats: i64,
}

#[derive(Debug, Serialize)]
pub struct ReceiveOverview {
    pub wallet_id: String,
    pub wallet_name: String,
    pub can_receive: bool,
    /// Why not, in words for the screen.
    pub reason: Option<String>,
    pub synced: bool,
    pub fingerprints: Vec<String>,
    pub checksum: Option<String>,
    pub gap_limit: i64,
    pub unused_given_out: i64,
    /// The descriptor has no `/*` range: it is one address, given out once.
    pub fixed: bool,
    pub addresses: Vec<ReceiveAddress>,
}

/// What a descriptor says about its keys, for the screen.
#[derive(Debug, PartialEq)]
pub(crate) struct KeyInfo {
    pub fingerprints: Vec<String>,
    /// Path up to the address index, e.g. "m/84'/0'/0'/0"; "…/0" without an origin or with several keys.
    pub path_prefix: String,
    pub checksum: String,
}

pub(crate) fn key_info(descriptor: &str) -> Result<KeyInfo, String> {
    let d = Descriptor::<DescriptorPublicKey>::from_str(descriptor.trim()).map_err(|e| format!("cannot read this wallet's descriptor: {e}"))?;
    let full = d.to_string();
    let checksum = full.rsplit_once('#').map(|(_, c)| c.to_string()).unwrap_or_default();
    let mut fingerprints = Vec::new();
    let mut prefixes = Vec::new();
    d.for_each_key(|k| {
        fingerprints.push(k.master_fingerprint().to_string());
        if let DescriptorPublicKey::XPub(x) = k {
            let tail: String = x.derivation_path.into_iter().map(|c| format!("/{c}")).collect();
            prefixes.push(match &x.origin {
                Some((_, origin)) => format!("m{}{tail}", origin.into_iter().map(|c| format!("/{c}")).collect::<String>()),
                None => format!("…{tail}"),
            });
        }
        true
    });
    // No `/*`: one fixed address, whatever its keys' paths say.
    let path_prefix = if !d.has_wildcard() { String::new() } else { match prefixes.as_slice() {
        [one] => one.clone(),
        // Several keys (multisig): their shared tail is what identifies the address.
        many => many.first().map(|p| format!("…{}", p.rsplit_once('/').map(|(_, last)| format!("/{last}")).unwrap_or_default())).unwrap_or_else(|| "…".into()),
    } };
    Ok(KeyInfo { fingerprints, path_prefix, checksum })
}

fn path_for(prefix: &str, index: u32) -> String {
    if prefix.is_empty() {
        return "one fixed address (no /* range)".into();
    }
    format!("{prefix}/{index}")
}

pub(crate) fn derive_receive_address(wallet: &Wallet, index: u32) -> Result<String, String> {
    let ext = wallet.descriptor.clone().ok_or("this wallet has no descriptor to derive addresses from")?;
    let network = crate::bitcoin::sync::parse_network(&wallet.network);
    let w = BdkWallet::create_single(ext).network(network).create_wallet_no_persist().map_err(|e| format!("cannot use this wallet's descriptor: {e}"))?;
    Ok(w.peek_address(KeychainKind::External, index).address.to_string())
}

/// Why a wallet cannot use Receive, or None if it can.
fn not_receivable(wallet: &Wallet) -> Option<String> {
    if wallet.archived {
        return Some("This wallet is archived.".into());
    }
    if !RECEIVE_SOURCES.contains(&wallet.source_type.as_str()) || wallet.descriptor.is_none() {
        return Some(match wallet.source_type.as_str() {
            "address" => "This wallet is a single address, so there is nothing to derive new addresses from. Use Saved addresses instead.".into(),
            "manual" => "This wallet is tracked by hand, with no public keys in Nakos.".into(),
            _ => "This wallet has no descriptor or xpub to derive addresses from.".into(),
        });
    }
    None
}

/// What Receive says when a fixed descriptor's one address is already shown.
const FIXED_ALREADY_SHOWN: &str = "This descriptor names one fixed address (it has no /* range), and it's already shown here. Nakos can't make another from it: paying the same address again links those payments together. For more addresses, add the wallet's xpub or a descriptor ending in /*, or use Saved addresses.";

/// A descriptor without a `/*` range stands for one address: every index gives the same one.
fn is_fixed_descriptor(descriptor: &str) -> bool {
    Descriptor::<DescriptorPublicKey>::from_str(descriptor).is_ok_and(|d| !d.has_wildcard())
}

fn highest_used_index(conn: &Connection, wallet_id: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT MAX(address_index) FROM addresses WHERE wallet_id = ?1 AND keychain = 'external' AND is_used = 1",
        params![wallet_id],
        |r| r.get(0),
    )
}

fn next_index(conn: &Connection, wallet_id: &str) -> rusqlite::Result<u32> {
    let given: Option<i64> = conn.query_row("SELECT MAX(address_index) FROM receive_addresses WHERE wallet_id = ?1", params![wallet_id], |r| r.get(0))?;
    let top = highest_used_index(conn, wallet_id)?.into_iter().chain(given).max();
    Ok(top.map_or(0, |t| (t + 1) as u32))
}

const ROW_SQL: &str = "SELECT r.id, r.address_index, r.address, r.label, r.amount_sats, r.device_checked_at, r.created_at,
        EXISTS (SELECT 1 FROM addresses a WHERE a.wallet_id = r.wallet_id AND a.address = r.address AND a.is_used = 1),
        COALESCE((SELECT SUM(u.amount_sats) FROM utxos u JOIN addresses a ON u.address_id = a.id
                  WHERE a.wallet_id = r.wallet_id AND a.address = r.address), 0)
     FROM receive_addresses r";

fn row(prefix: &str) -> impl Fn(&rusqlite::Row) -> rusqlite::Result<ReceiveAddress> + '_ {
    move |r| {
        let index: i64 = r.get(1)?;
        Ok(ReceiveAddress {
            id: r.get(0)?,
            index: index as u32,
            address: r.get(2)?,
            path: path_for(prefix, index as u32),
            label: r.get(3)?,
            amount_sats: r.get(4)?,
            device_checked_at: r.get(5)?,
            created_at: r.get(6)?,
            used: r.get(7)?,
            received_sats: r.get(8)?,
        })
    }
}

pub(crate) fn overview(conn: &Connection, wallet: &Wallet) -> Result<ReceiveOverview, String> {
    let reason = not_receivable(wallet);
    let info = match (&reason, &wallet.descriptor) {
        (None, Some(d)) => Some(key_info(d)?),
        _ => None,
    };
    let prefix = info.as_ref().map_or("…/0".to_string(), |i| i.path_prefix.clone());
    let mut stmt = conn.prepare(&format!("{ROW_SQL} WHERE r.wallet_id = ?1 ORDER BY r.address_index DESC")).map_err(|e| e.to_string())?;
    let addresses = stmt.query_map(params![wallet.id], row(&prefix)).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let unused_given_out = unused_given_out(conn, &wallet.id).map_err(|e| e.to_string())?;
    Ok(ReceiveOverview {
        wallet_id: wallet.id.clone(),
        wallet_name: wallet.name.clone(),
        can_receive: reason.is_none(),
        reason,
        synced: wallet.last_synced_at.is_some(),
        fingerprints: info.as_ref().map(|i| i.fingerprints.clone()).unwrap_or_default(),
        checksum: info.map(|i| i.checksum),
        gap_limit: wallet.gap_limit,
        unused_given_out,
        fixed: wallet.descriptor.as_deref().is_some_and(is_fixed_descriptor),
        addresses,
    })
}

/// Handed-out addresses past the highest used index that have not been used: the run of unused
/// addresses another wallet app has to scan past.
fn unused_given_out(conn: &Connection, wallet_id: &str) -> rusqlite::Result<i64> {
    let top = highest_used_index(conn, wallet_id)?.unwrap_or(-1);
    conn.query_row(
        "SELECT COUNT(*) FROM receive_addresses r WHERE r.wallet_id = ?1 AND r.address_index > ?2
           AND NOT EXISTS (SELECT 1 FROM addresses a WHERE a.wallet_id = r.wallet_id AND a.address = r.address AND a.is_used = 1)",
        params![wallet_id, top],
        |r| r.get(0),
    )
}

fn clean_label(label: Option<String>) -> Result<Option<String>, String> {
    let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
    if let Some(l) = &label {
        if l.chars().count() > MAX_LABEL {
            return Err(format!("keep the label under {MAX_LABEL} characters"));
        }
        crate::commands::wallets::refuse_private_key_material([Some(l.as_str())])?;
    }
    Ok(label)
}

fn check_amount(amount: Option<i64>) -> Result<Option<i64>, String> {
    match amount {
        Some(a) if a <= 0 || !crate::commands::wallets::plausible_sats(a) => Err("the amount must be more than 0 and at most 21 million BTC".into()),
        other => Ok(other),
    }
}

pub(crate) fn hand_out(conn: &Connection, wallet: &Wallet, label: Option<String>, amount_sats: Option<i64>) -> Result<ReceiveAddress, String> {
    if let Some(reason) = not_receivable(wallet) {
        return Err(reason);
    }
    if wallet.last_synced_at.is_none() {
        return Err("Sync this wallet first, so Nakos knows which addresses are already used.".into());
    }
    let label = clean_label(label)?;
    let amount_sats = check_amount(amount_sats)?;
    let fixed = wallet.descriptor.as_deref().is_some_and(is_fixed_descriptor);
    if fixed {
        let shown: bool = conn
            .query_row("SELECT EXISTS (SELECT 1 FROM receive_addresses WHERE wallet_id = ?1)", params![wallet.id], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if shown {
            return Err(FIXED_ALREADY_SHOWN.into());
        }
    }
    let waiting = unused_given_out(conn, &wallet.id).map_err(|e| e.to_string())?;
    if !fixed && waiting >= wallet.gap_limit {
        return Err(format!(
            "{waiting} addresses handed out here are still unused. Most wallet apps stop looking after {} unused addresses in a row, so coins sent to a new one might not show up there. Use one of the unused addresses below, or wait until one receives coins.",
            wallet.gap_limit
        ));
    }
    let info = key_info(wallet.descriptor.as_deref().unwrap_or_default())?;
    let index = if fixed { 0 } else { next_index(conn, &wallet.id).map_err(|e| e.to_string())? };
    let address = derive_receive_address(wallet, index)?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO receive_addresses (id, wallet_id, address_index, address, label, amount_sats) VALUES (?1,?2,?3,?4,?5,?6)",
        params![id, wallet.id, index, address, label, amount_sats],
    )
    .map_err(|e| e.to_string())?;
    one(conn, &id, &info.path_prefix)
}

fn one(conn: &Connection, id: &str, prefix: &str) -> Result<ReceiveAddress, String> {
    conn.query_row(&format!("{ROW_SQL} WHERE r.id = ?1"), params![id], row(prefix)).map_err(|e| e.to_string())
}

fn wallet_of(conn: &Connection, id: &str) -> Result<Wallet, String> {
    let wallet_id: String = conn
        .query_row("SELECT wallet_id FROM receive_addresses WHERE id = ?1", params![id], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("no such address")?;
    crate::commands::wallets::get_wallet(conn, &wallet_id)
}

fn prefix_of(wallet: &Wallet) -> String {
    wallet.descriptor.as_deref().and_then(|d| key_info(d).ok()).map_or("…/0".into(), |i| i.path_prefix)
}

#[tauri::command]
pub fn receive_overview(state: tauri::State<AppState>, wallet_id: String) -> Result<ReceiveOverview, String> {
    let conn = state.pool()?.get().map_err(|e| e.to_string())?;
    let wallet = crate::commands::wallets::get_wallet(&conn, &wallet_id)?;
    overview(&conn, &wallet)
}

#[tauri::command]
pub fn new_receive_address(state: tauri::State<AppState>, wallet_id: String, label: Option<String>, amount_sats: Option<i64>) -> Result<ReceiveAddress, String> {
    let conn = state.pool()?.get().map_err(|e| e.to_string())?;
    let wallet = crate::commands::wallets::get_wallet(&conn, &wallet_id)?;
    hand_out(&conn, &wallet, label, amount_sats)
}

#[tauri::command]
pub fn update_receive_address(state: tauri::State<AppState>, id: String, label: Option<String>, amount_sats: Option<i64>) -> Result<ReceiveAddress, String> {
    let conn = state.pool()?.get().map_err(|e| e.to_string())?;
    let label = clean_label(label)?;
    let amount_sats = check_amount(amount_sats)?;
    let wallet = wallet_of(&conn, &id)?;
    conn.execute("UPDATE receive_addresses SET label = ?2, amount_sats = ?3 WHERE id = ?1", params![id, label, amount_sats]).map_err(|e| e.to_string())?;
    one(&conn, &id, &prefix_of(&wallet))
}

/// The person compared this address with the one their signing device shows (or undoes that).
#[tauri::command]
pub fn set_receive_checked(state: tauri::State<AppState>, id: String, checked: bool) -> Result<ReceiveAddress, String> {
    let conn = state.pool()?.get().map_err(|e| e.to_string())?;
    let wallet = wallet_of(&conn, &id)?;
    conn.execute(
        "UPDATE receive_addresses SET device_checked_at = CASE WHEN ?2 THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE NULL END WHERE id = ?1",
        params![id, checked],
    )
    .map_err(|e| e.to_string())?;
    one(&conn, &id, &prefix_of(&wallet))
}

// -- QR codes -------------------------------------------------------------------------------------

/// A QR code as a square of modules, row by row, '1' dark and '0' light. The screen draws it, so
/// no JavaScript library and no outside service ever sees the address.
#[derive(Debug, Serialize)]
pub struct Qr {
    pub size: usize,
    pub modules: String,
}

pub(crate) fn qr_for(text: &str) -> Result<Qr, String> {
    let t = text.trim();
    let scheme = t.get(..8).is_some_and(|s| s.eq_ignore_ascii_case("bitcoin:"));
    if !scheme || t.len() > 600 {
        return Err("only a bitcoin: payment link is drawn as a QR code".into());
    }
    crate::commands::wallets::refuse_private_key_material([Some(t)])?;
    let code = qrcode::QrCode::with_error_correction_level(t.as_bytes(), qrcode::EcLevel::M).map_err(|e| e.to_string())?;
    let modules = code.to_colors().iter().map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' }).collect();
    Ok(Qr { size: code.width(), modules })
}

#[tauri::command]
pub fn payment_qr(text: String) -> Result<Qr, String> {
    qr_for(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The BIP84 test vector (abandon … about): first receive address bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu.
    const XPUB: &str = "xpub6CatWdiZiodmUeTDp8LT5or8nmbKNcuyvz7WyksVFkKB4RHwCD3XyuvPEbvqAQY3rAPshWcMLoP2fMFMKHPJ4ZeZXYVUhLv1VMrjPC7PW6V";
    const DESC: &str = "wpkh([73c5da0a/84h/0h/0h]xpub6CatWdiZiodmUeTDp8LT5or8nmbKNcuyvz7WyksVFkKB4RHwCD3XyuvPEbvqAQY3rAPshWcMLoP2fMFMKHPJ4ZeZXYVUhLv1VMrjPC7PW6V/0/*)";

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO wallets (id,name,custody_type,source_type,descriptor,network,last_synced_at) VALUES ('w','Cold','cold_storage','descriptor',?1,'bitcoin','2026-11-07T00:00:00Z')",
            params![DESC],
        )
        .unwrap();
        conn
    }

    fn db_wallet_ranged() -> Wallet {
        let conn = db();
        wallet(&conn)
    }

    fn wallet(conn: &Connection) -> Wallet {
        crate::commands::wallets::get_wallet(conn, "w").unwrap()
    }

    fn mark_used(conn: &Connection, index: u32) {
        let addr = derive_receive_address(&wallet(conn), index).unwrap();
        conn.execute(
            "INSERT INTO addresses (id, wallet_id, address, keychain, address_index, is_used) VALUES (?1,'w',?2,'external',?3,1)",
            params![format!("a{index}"), addr, index],
        )
        .unwrap();
    }

    #[test]
    fn the_descriptor_gives_fingerprint_path_and_checksum() {
        let info = key_info(DESC).unwrap();
        assert_eq!(info.fingerprints, vec!["73c5da0a"]);
        assert_eq!(info.path_prefix, "m/84'/0'/0'/0");
        assert_eq!(info.checksum.len(), 8);
        let bare = key_info(&format!("wpkh({XPUB}/0/*)")).unwrap();
        assert_eq!(bare.path_prefix, "…/0");
    }

    #[test]
    fn the_first_address_matches_the_bip84_vector_and_indexes_never_repeat() {
        let conn = db();
        let a = hand_out(&conn, &wallet(&conn), Some("from Kraken".into()), Some(250_000)).unwrap();
        assert_eq!((a.index, a.address.as_str()), (0, "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"));
        assert_eq!(a.path, "m/84'/0'/0'/0/0");
        assert!(a.device_checked_at.is_none() && !a.used);
        let b = hand_out(&conn, &wallet(&conn), None, None).unwrap();
        assert_eq!(b.index, 1);
    }

    #[test]
    fn the_next_index_skips_past_addresses_used_on_chain() {
        let conn = db();
        mark_used(&conn, 0);
        mark_used(&conn, 4);
        let a = hand_out(&conn, &wallet(&conn), None, None).unwrap();
        assert_eq!(a.index, 5);
        let o = overview(&conn, &wallet(&conn)).unwrap();
        assert_eq!(o.unused_given_out, 1);
    }

    #[test]
    fn the_gap_limit_stops_new_addresses_until_one_is_used() {
        let conn = db();
        for _ in 0..20 {
            hand_out(&conn, &wallet(&conn), None, None).unwrap();
        }
        let err = hand_out(&conn, &wallet(&conn), None, None).unwrap_err();
        assert!(err.contains("still unused"), "{err}");
        mark_used(&conn, 3);
        // Indexes 0..=3 are no longer part of the unused run past the last used one.
        let next = hand_out(&conn, &wallet(&conn), None, None).unwrap();
        assert_eq!(next.index, 20);
    }

    #[test]
    fn an_unsynced_or_address_wallet_cannot_hand_out() {
        let conn = db();
        conn.execute("UPDATE wallets SET last_synced_at = NULL", []).unwrap();
        assert!(hand_out(&conn, &wallet(&conn), None, None).unwrap_err().contains("Sync this wallet first"));
        conn.execute("UPDATE wallets SET source_type = 'address', descriptor = 'bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu', last_synced_at = 'x'", []).unwrap();
        let o = overview(&conn, &wallet(&conn)).unwrap();
        assert!(!o.can_receive && o.reason.unwrap().contains("Saved addresses"));
    }

    #[test]
    fn device_check_marks_and_unmarks_and_labels_refuse_keys() {
        let conn = db();
        let a = hand_out(&conn, &wallet(&conn), None, None).unwrap();
        conn.execute("UPDATE receive_addresses SET device_checked_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1", params![a.id]).unwrap();
        assert!(one(&conn, &a.id, "m").unwrap().device_checked_at.is_some());
        let seed = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        assert!(hand_out(&conn, &wallet(&conn), Some(seed.into()), None).is_err());
        assert!(hand_out(&conn, &wallet(&conn), None, Some(0)).is_err());
    }

    #[test]
    fn a_used_address_shows_what_it_received() {
        let conn = db();
        let a = hand_out(&conn, &wallet(&conn), None, None).unwrap();
        mark_used(&conn, 0);
        conn.execute("INSERT INTO utxos (id,wallet_id,address_id,txid,vout,amount_sats) VALUES ('u','w','a0','tx',0,250000)", []).unwrap();
        let got = one(&conn, &a.id, "m").unwrap();
        assert!(got.used);
        assert_eq!(got.received_sats, 250_000);
    }

    #[test]
    fn qr_codes_are_only_for_payment_links() {
        let q = qr_for("bitcoin:bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu?amount=0.0025").unwrap();
        assert_eq!(q.modules.len(), q.size * q.size);
        assert!(qr_for("https://example.com").is_err());
        assert!(qr_for(&format!("bitcoin:{}", "x".repeat(700))).is_err());
        assert!(qr_for("bitcoin:xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi").is_err());
    }

    #[test]
    fn a_descriptor_without_a_range_shows_its_one_address_once() {
        // Sparrow exports a single-key wallet as wpkh(<pubkey>); this is the same kind: one address.
        let conn = db();
        conn.execute("UPDATE wallets SET descriptor = ?1", params![format!("wpkh([73c5da0a/84h/0h/0h]{XPUB}/0/0)")]).unwrap();
        let w = wallet(&conn);
        assert!(not_receivable(&w).is_none());
        let ov = overview(&conn, &w).unwrap();
        assert!(ov.fixed && ov.can_receive);
        let a = hand_out(&conn, &w, None, None).unwrap();
        assert_eq!((a.index, a.address.as_str()), (0, "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"));
        assert_eq!(a.path, "one fixed address (no /* range)");
        // The second press explains, instead of failing on the database's unique address.
        assert_eq!(hand_out(&conn, &w, None, None).unwrap_err(), FIXED_ALREADY_SHOWN);
        assert!(!overview(&conn, &db_wallet_ranged()).unwrap().fixed);

        // A bare public key, as Sparrow writes it (BIP173's example key and address).
        let conn = db();
        conn.execute("UPDATE wallets SET descriptor = 'wpkh(0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798)'", []).unwrap();
        let w = wallet(&conn);
        assert_eq!(hand_out(&conn, &w, None, None).unwrap().address, "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4");
    }
}
