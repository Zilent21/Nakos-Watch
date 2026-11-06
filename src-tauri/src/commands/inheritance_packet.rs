//! Inheritance packet: what an heir needs to recover *visibility* of these
//! watch-only wallets in Sparrow (or any descriptor wallet), plus optional
//! encrypted notes. Nakos is not the signer. Nothing here reads, stores or
//! exports seeds/private keys, RPC passwords, or the vault passphrase.
//!
//! Plaintext in nakos.db: wallet descriptors/xpubs/fingerprints/paths, node
//! host/port, cookie *path*. Encrypted with the vault key: letter/notes
//! (`instructions_encrypted`) and the reminder note.
//!
//! future: optional cloud / LN dead-man

use crate::commands::inheritance::get_or_create_config;
use crate::commands::wallets::{insert_multisig_wallet, insert_wallet, refuse_private_key_material, row_to_wallet};
use crate::db::models::{Cosigner, NewWallet, Wallet};
use crate::state::AppState;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketCosigner {
    pub label: String,
    pub xpub: String,
    pub fingerprint: Option<String>,
    pub derivation_path: Option<String>,
    pub hardware_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketMultisig {
    pub threshold_m: i64,
    pub total_n: i64,
    pub script_type: String,
    pub cosigners: Vec<PacketCosigner>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketWallet {
    pub id: String,
    pub name: String,
    /// "descriptor" | "xpub" | "multisig" | "address_only" | "manual"
    pub kind: String,
    pub source_type: String,
    pub network: String,
    /// Needed to recreate the wallet on import; older packets omit it.
    #[serde(default = "default_custody")]
    pub custody_type: String,
    pub archived: bool,
    pub master_fingerprint: Option<String>,
    pub derivation_path: Option<String>,
    pub descriptor: Option<String>,
    pub change_descriptor: Option<String>,
    pub xpub: Option<String>,
    pub multisig: Option<PacketMultisig>,
    /// Watched addresses, only for address-only wallets (real stored data).
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub heir_can_reconstruct: bool,
    pub heir_warning: Option<String>,
    /// Last-known balance at export time, so an heir without a server can
    /// still see something. `None` when this machine has nothing real to say.
    #[serde(default)]
    pub snapshot: Option<PacketSnapshot>,
    /// Cost-basis lots for unspent coins. Only ever filled for the advanced
    /// JSON export when the user opts in; the simple letter never carries it.
    #[serde(default)]
    pub cost_basis: Vec<PacketLot>,
    /// Decrypted note; only present when the user opted in to notes.
    pub note: Option<String>,
}

fn default_custody() -> String {
    "other".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacketLot {
    pub txid: String,
    pub vout: i64,
    pub amount_sats: i64,
    pub acquired_at: String,
    pub cost_fiat: f64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacketSnapshot {
    pub balance_sats: i64,
    pub utxo_count: i64,
    pub as_of: String,
}

/// Connection details only. No usernames, no passwords; a cookie file is
/// exported as its path string, never its contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketNode {
    pub label: String,
    pub kind: String,
    pub host: String,
    pub port: i64,
    pub use_tls: bool,
    pub use_tor: bool,
    pub cookie_path: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub format_version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub watch_only_notice: String,
    pub wallets: Vec<PacketWallet>,
    /// Advanced packets only; the simple letter always exports `[]`, and
    /// import ignores this field entirely.
    #[serde(default)]
    pub nodes: Vec<PacketNode>,
    #[serde(default)]
    pub checklist: Vec<String>,
    /// Global letter, only when the user opted in to notes.
    pub letter: Option<String>,
    pub review_by: Option<String>,
    pub review_note: Option<String>,
}

const WATCH_ONLY_NOTICE: &str = "This packet lets someone WATCH and plan spends for these wallets. It contains no seeds, private keys, RPC passwords or vault passphrase, and it cannot move funds. Spending needs the signing devices or seed backups, which Nakos never holds.";

pub fn wallet_to_packet(
    w: &Wallet,
    multisig: Option<PacketMultisig>,
    addresses: Vec<String>,
    snapshot: Option<PacketSnapshot>,
) -> PacketWallet {
    let kind = match w.source_type.as_str() {
        "address" => "address_only",
        "manual" => "manual",
        "multisig_descriptor" => "multisig",
        "descriptor" => "descriptor",
        _ => "xpub", // xpub / ypub / zpub
    };
    let has_material = w.descriptor.is_some() || w.xpub.is_some() || multisig.is_some();
    let (can, warning): (bool, Option<&str>) = match kind {
        "address_only" => (
            false,
            Some("An heir cannot reconstruct signing or spend planning for this wallet without extra information you store in notes. Only the watched addresses are listed."),
        ),
        "manual" => (false, Some("Manual balance entry: there is no on-chain data an heir could import.")),
        _ if !has_material => (false, Some("No descriptor or xpub is stored for this wallet.")),
        "xpub" if w.master_fingerprint.is_none() || w.derivation_path.is_none() => (
            true,
            Some("Fingerprint or derivation path not stored: the xpub can be watched, but spending will need them from the device or your notes."),
        ),
        _ => (true, None),
    };
    PacketWallet {
        id: w.id.clone(),
        name: w.name.clone(),
        kind: kind.to_string(),
        source_type: w.source_type.clone(),
        network: w.network.clone(),
        custody_type: w.custody_type.clone(),
        archived: w.archived,
        master_fingerprint: w.master_fingerprint.clone(),
        derivation_path: w.derivation_path.clone(),
        // Never synthesise a descriptor: export exactly what is stored.
        descriptor: w.descriptor.clone(),
        change_descriptor: w.change_descriptor.clone(),
        xpub: w.xpub.clone(),
        multisig,
        addresses: if kind == "address_only" { addresses } else { Vec::new() },
        heir_can_reconstruct: can,
        heir_warning: warning.map(String::from),
        snapshot,
        cost_basis: Vec::new(),
        note: None,
    }
}

pub fn build_checklist(current_electrum: Option<&str>) -> Vec<String> {
    let electrum_line = match current_electrum {
        Some(h) => format!(
            "Point Sparrow's server setting at an electrs you control. This machine's current electrs: {h}. Replace with yours: ______________"
        ),
        None => "Point Sparrow's server setting at an electrs you control (no Electrum server is saved in Nakos): ______________".to_string(),
    };
    vec![
        "Install Sparrow (or any wallet that imports descriptors) on a computer you trust.".to_string(),
        "Import each wallet below using its descriptor or xpub (with fingerprint and derivation path where listed). Address-only wallets need the extra information in the notes.".to_string(),
        electrum_line,
        "To spend, sign with a hardware wallet or the seed backup you already hold. Nakos does not provide seeds or keys, and you should never type a seed into Nakos.".to_string(),
    ]
}

/// Checklist for the simple letter's data block: never names the sender's
/// own server, because an heir must not be pointed at it as if it were theirs.
pub fn build_helper_checklist() -> Vec<String> {
    let mut c = build_checklist(None);
    c[2] = "Point Sparrow's server setting at an electrs you control: ______________".to_string();
    c
}

/// Plain-language name of the wallet's shape, for the recovery details.
fn wallet_type_label(w: &PacketWallet) -> String {
    match (w.kind.as_str(), &w.multisig) {
        (_, Some(m)) => format!("Multisig, {} of {} ({})", m.threshold_m, m.total_n, m.script_type),
        ("descriptor", _) => "Single-signature, from a descriptor".into(),
        ("xpub", _) => format!("Single-signature, from an extended public key ({})", w.source_type),
        ("address_only", _) => "Watched addresses only (no key information)".into(),
        ("manual", _) => "Manual balance (nothing on-chain to watch)".into(),
        (k, _) => k.to_string(),
    }
}

/// What the heir's helper should do with this wallet, in order.
fn next_steps(w: &PacketWallet) -> Vec<&'static str> {
    match (w.kind.as_str(), w.multisig.is_some()) {
        (_, true) => vec![
            "Import the descriptor into Sparrow (File → New Wallet → Edit Existing, then paste it) or into Nakos, as watch-only.",
            "Check that the first receive address matches the one each signing device shows for this wallet.",
            "Collect enough signing devices to meet the threshold. Each one signs the same PSBT in turn; they never need to be in one place at once.",
            "Before moving everything, send a small test amount to a new wallet the heir controls.",
        ],
        ("descriptor", _) => vec![
            "Import the descriptor into Sparrow or Nakos as watch-only and let it sync.",
            "Check the first receive address against the signing device's screen.",
            "Connect the signing device (or restore its seed onto a new device, offline) to sign.",
            "Send a small test amount first.",
        ],
        ("xpub", _) => vec![
            "Import the extended public key into Sparrow with the script type above (zpub = native segwit) and the fingerprint and path, if listed.",
            "Check the first receive address against the signing device. If it differs, the script type or path is wrong: stop and recheck before going further.",
            "Connect the signing device to sign, and send a small test amount first.",
        ],
        ("address_only", _) => vec![
            "These addresses can be watched but not spent from here. The wallet that made them is needed: see the notes and the letter for where it is kept.",
        ],
        _ => vec!["Nothing to recover on-chain. See the notes for the account this balance belongs to."],
    }
}

/// The structured block the advanced packet gives every wallet: type, where its public keys are,
/// which hardware holds the private keys, and what to do next.
fn recovery_details(w: &PacketWallet) -> String {
    let mut o = String::from("\n**Recovery details**\n\n");
    o.push_str(&format!("- Wallet type: {}\n", wallet_type_label(w)));
    let in_packet = if w.descriptor.is_some() {
        "descriptor in this packet (above)"
    } else if w.xpub.is_some() || w.multisig.is_some() {
        "extended public key(s) in this packet (above)"
    } else {
        "none in this packet"
    };
    o.push_str(&format!("- Descriptor / xpub location: {in_packet}; second copy kept at {BLANK}\n"));
    let models: Vec<String> = w
        .multisig
        .iter()
        .flat_map(|m| m.cosigners.iter())
        .map(|c| format!("{}: {}", c.label, c.hardware_model.as_deref().unwrap_or(BLANK)))
        .collect();
    if models.is_empty() {
        o.push_str(&format!("- Hardware wallet model: {BLANK}\n"));
    } else {
        o.push_str(&format!("- Hardware wallet models: {}\n", models.join("; ")));
    }
    o.push_str("- Recommended next steps:\n");
    for (i, step) in next_steps(w).iter().enumerate() {
        o.push_str(&format!("  {}. {step}\n", i + 1));
    }
    o
}

pub fn render_markdown(p: &Packet) -> String {
    let mut o = String::new();
    o.push_str("# Nakos inheritance packet (watch-only)\n\n");
    o.push_str(&format!("Generated: {}\n\n> {}\n\n", p.generated_at, p.watch_only_notice));
    if let Some(d) = &p.review_by {
        o.push_str(&format!("Review this packet by: **{d}**{}\n\n", p.review_note.as_deref().map(|n| format!(" ({n})")).unwrap_or_default()));
    }
    o.push_str("## What this packet does not contain\n\n");
    for n in DOES_NOT_CONTAIN {
        o.push_str(&format!("- {n}\n"));
    }
    o.push_str("\nIt also leaves out RPC passwords, API tokens and cookie file contents (a cookie file is named by its path only).\n\n");
    o.push_str("## Heir checklist\n\n");
    for c in &p.checklist {
        o.push_str(&format!("- [ ] {c}\n"));
    }
    o.push_str("\n## Wallets\n");
    for w in &p.wallets {
        o.push_str(&format!("\n### {}{}\n\n- Type: {} ({})\n- Network: {}\n", w.name, if w.archived { " (archived)" } else { "" }, w.kind, w.source_type, w.network));
        if let Some(v) = &w.master_fingerprint { o.push_str(&format!("- Master fingerprint: `{v}`\n")); }
        if let Some(v) = &w.derivation_path { o.push_str(&format!("- Derivation path: `{v}`\n")); }
        if let Some(v) = &w.descriptor { o.push_str(&format!("- Descriptor: `{v}`\n")); }
        if let Some(v) = &w.change_descriptor { o.push_str(&format!("- Change descriptor: `{v}`\n")); }
        if let Some(v) = &w.xpub { o.push_str(&format!("- Extended public key: `{v}`\n")); }
        if let Some(m) = &w.multisig {
            o.push_str(&format!("- Multisig: {}-of-{} ({})\n", m.threshold_m, m.total_n, m.script_type));
            for c in &m.cosigners {
                o.push_str(&format!(
                    "  - {}: `{}` fingerprint `{}` path `{}`{}\n",
                    c.label,
                    c.xpub,
                    c.fingerprint.as_deref().unwrap_or("not stored"),
                    c.derivation_path.as_deref().unwrap_or("not stored"),
                    c.hardware_model.as_deref().map(|h| format!(" ({h})")).unwrap_or_default()
                ));
            }
        }
        for a in &w.addresses { o.push_str(&format!("- Watched address: `{a}`\n")); }
        if let Some(sn) = &w.snapshot {
            o.push_str(&format!("- Last known balance: {} sats ({} coins) as of {}\n", sn.balance_sats, sn.utxo_count, sn.as_of));
        }
        if let Some(wn) = &w.heir_warning { o.push_str(&format!("- **Note for heir:** {wn}\n")); }
        o.push_str(&recovery_details(w));
        if let Some(n) = &w.note { o.push_str(&format!("\nNotes:\n\n{n}\n")); }
    }
    o.push_str("\n## Nodes saved in Nakos\n\n");
    if p.nodes.is_empty() {
        o.push_str("None.\n");
    }
    for n in &p.nodes {
        o.push_str(&format!(
            "- {} ({}): {}:{}{}{}{}\n",
            n.label, n.kind, n.host, n.port,
            // Electrum TLS is real. Bitcoin Core RPC is plain HTTP, so it never claims TLS.
            if n.use_tls && n.kind == "electrum" { ", TLS" } else { "" },
            if n.use_tor { ", Tor" } else { "" },
            n.cookie_path.as_deref().map(|c| format!(", cookie file path `{c}`")).unwrap_or_default()
        ));
    }
    if let Some(l) = &p.letter {
        o.push_str(&format!("\n## Letter / notes\n\n{l}\n"));
    }
    o
}

fn decrypt_notes(state: &AppState, conn: &Connection, wallet_id: Option<&str>) -> Result<Option<String>, String> {
    let row: Option<(Option<Vec<u8>>, Option<Vec<u8>>)> = conn
        .query_row(
            "SELECT instructions_encrypted, instructions_nonce FROM inheritance_configs WHERE wallet_id IS ?1",
            params![wallet_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let Some((Some(ct), Some(nonce))) = row else { return Ok(None) };
    let vault = state.vault()?;
    let guard = vault.lock();
    let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it to include notes in the packet")?;
    let pt = key.decrypt(&nonce, &ct).map_err(|e| e.to_string())?;
    Ok(Some(String::from_utf8_lossy(&pt).to_string()))
}

fn decrypt_review_note(state: &AppState, conn: &Connection) -> Result<Option<String>, String> {
    let row: Option<(Option<Vec<u8>>, Option<Vec<u8>>)> = conn
        .query_row(
            "SELECT review_note_encrypted, review_note_nonce FROM inheritance_configs WHERE wallet_id IS NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let Some((Some(ct), Some(nonce))) = row else { return Ok(None) };
    let vault = state.vault()?;
    let guard = vault.lock();
    let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it to read the reminder note")?;
    let pt = key.decrypt(&nonce, &ct).map_err(|e| e.to_string())?;
    Ok(Some(String::from_utf8_lossy(&pt).to_string()))
}

/// Everything except notes: no vault needed. Advanced-filtered nodes.
fn load_packet_data(conn: &Connection, include_archived: bool) -> Result<Packet, String> {
    let sql = if include_archived {
        "SELECT * FROM wallets ORDER BY created_at ASC"
    } else {
        "SELECT * FROM wallets WHERE archived = 0 ORDER BY created_at ASC"
    };
    let wallets: Vec<Wallet> = conn
        .prepare(sql)
        .map_err(|e| e.to_string())?
        .query_map([], row_to_wallet)
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    let mut out_wallets = Vec::with_capacity(wallets.len());
    for w in &wallets {
        let multisig = load_multisig(conn, &w.id)?;
        let addresses: Vec<String> = if w.source_type == "address" {
            conn.prepare("SELECT address FROM addresses WHERE wallet_id = ?1 ORDER BY address_index ASC, address ASC")
                .map_err(|e| e.to_string())?
                .query_map(params![w.id], |r| r.get(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };
        let snapshot = load_snapshot(conn, w)?;
        out_wallets.push(wallet_to_packet(w, multisig, addresses, snapshot));
    }

    let nodes: Vec<PacketNode> = conn
        .prepare("SELECT label, kind, host, port, use_tls, use_tor, cookie_path, is_default FROM node_configs WHERE enabled = 1 ORDER BY is_default DESC, created_at ASC")
        .map_err(|e| e.to_string())?
        .query_map([], |r| {
            let kind: String = r.get(1)?;
            Ok(PacketNode {
                label: r.get(0)?,
                // Only Electrum can really use TLS; a Core row never reports it.
                use_tls: kind == "electrum" && r.get::<_, i64>(4)? != 0,
                kind,
                host: r.get(2)?,
                port: r.get(3)?,
                use_tor: r.get::<_, i64>(5)? != 0,
                cookie_path: r.get(6)?,
                is_default: r.get::<_, i64>(7)? != 0,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let nodes = clean_nodes(nodes);

    let current_electrum = nodes
        .iter()
        .find(|n| n.kind == "electrum")
        .map(|n| format!("{}:{}", n.host, n.port));

    let review_by: Option<String> = conn
        .query_row("SELECT review_by FROM inheritance_configs WHERE wallet_id IS NULL", [], |r| r.get(0))
        .ok()
        .flatten();

    Ok(Packet {
        format_version: 2,
        generated_at: chrono::Utc::now().to_rfc3339(),
        watch_only_notice: WATCH_ONLY_NOTICE.to_string(),
        wallets: out_wallets,
        nodes,
        checklist: build_checklist(current_electrum.as_deref()),
        letter: None,
        review_by,
        review_note: None,
    })
}

/// `load_packet_data` plus, when asked, the decrypted notes (needs the vault).
fn load_packet(state: &AppState, conn: &Connection, include_archived: bool, include_notes: bool) -> Result<Packet, String> {
    let mut packet = load_packet_data(conn, include_archived)?;
    if include_notes {
        for w in packet.wallets.iter_mut() {
            w.note = decrypt_notes(state, conn, Some(&w.id))?;
        }
        packet.letter = decrypt_notes(state, conn, None)?;
        packet.review_note = decrypt_review_note(state, conn)?;
    }
    Ok(packet)
}

/// Node hygiene for the advanced packet. Input is enabled rows only, default
/// first. Drops the retired 9332 port and duplicate host:port rows, then
/// keeps one row per kind (the default if there is one), so lab leftovers
/// don't ride along.
pub fn clean_nodes(nodes: Vec<PacketNode>) -> Vec<PacketNode> {
    let mut seen = std::collections::HashSet::new();
    let mut kinds = std::collections::HashSet::new();
    nodes
        .into_iter()
        .filter(|n| n.port != 9332)
        .filter(|n| seen.insert((n.kind.clone(), n.host.to_lowercase(), n.port)))
        .filter(|n| kinds.insert(n.kind.clone()))
        .collect()
}

/// Live numbers when this machine has synced the wallet; otherwise any
/// snapshot it was itself imported with; otherwise nothing.
fn load_snapshot(conn: &Connection, w: &Wallet) -> Result<Option<PacketSnapshot>, String> {
    if let Some(as_of) = &w.last_synced_at {
        let (sats, count): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_sats),0), COUNT(*) FROM utxos WHERE wallet_id = ?1 AND spent = 0",
                params![w.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        return Ok(Some(PacketSnapshot { balance_sats: sats, utxo_count: count, as_of: as_of.clone() }));
    }
    Ok(conn
        .query_row("SELECT balance_sats, utxo_count, as_of FROM wallet_snapshots WHERE wallet_id = ?1", params![w.id], |r| {
            Ok(PacketSnapshot { balance_sats: r.get(0)?, utxo_count: r.get(1)?, as_of: r.get(2)? })
        })
        .ok())
}

/// Attach recorded lots (unspent coins with both cost and date) to each wallet.
fn attach_basis(conn: &Connection, packet: &mut Packet) -> Result<(), String> {
    for w in packet.wallets.iter_mut() {
        w.cost_basis = conn
            .prepare(
                "SELECT txid, vout, amount_sats, acquisition_date, acquisition_cost_fiat, acquisition_note FROM utxos
                 WHERE wallet_id = ?1 AND spent = 0 AND acquisition_date IS NOT NULL AND acquisition_cost_fiat IS NOT NULL
                 ORDER BY acquisition_date ASC, txid ASC, vout ASC",
            )
            .map_err(|e| e.to_string())?
            .query_map(params![w.id], |r| {
                Ok(PacketLot { txid: r.get(0)?, vout: r.get(1)?, amount_sats: r.get(2)?, acquired_at: r.get(3)?, cost_fiat: r.get(4)?, note: r.get(5)? })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn load_multisig(conn: &Connection, wallet_id: &str) -> Result<Option<PacketMultisig>, String> {
    let cfg: Option<(String, i64, i64, String)> = conn
        .query_row(
            "SELECT id, threshold_m, total_n, script_type FROM multisig_configs WHERE wallet_id = ?1",
            params![wallet_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();
    let Some((id, threshold_m, total_n, script_type)) = cfg else { return Ok(None) };
    let cosigners = conn
        .prepare("SELECT label, xpub, fingerprint, derivation_path, hardware_model FROM multisig_cosigners WHERE multisig_id = ?1 ORDER BY sort_order ASC")
        .map_err(|e| e.to_string())?
        .query_map(params![id], |r| {
            Ok(PacketCosigner { label: r.get(0)?, xpub: r.get(1)?, fingerprint: r.get(2)?, derivation_path: r.get(3)?, hardware_model: r.get(4)? })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    Ok(Some(PacketMultisig { threshold_m, total_n, script_type, cosigners }))
}


// ---------------------------------------------------------------------------
// Simple letter for non-technical heirs
// ---------------------------------------------------------------------------

const ADVANCED_HEADING: &str = "# ADVANCED — ignore unless asked";
const MACHINE_BLOCK_OPEN: &str = "```nakos-inherit-json";

fn sats_to_btc(sats: i64) -> String {
    format!("{:.8}", sats as f64 / 100_000_000.0)
}

/// Which kind of family letter. The words change, the data block does not: every template
/// imports the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LetterTemplate {
    /// Plain steps for a family with no technical background.
    #[default]
    Family,
    /// One hardware wallet and one seed backup in cold storage.
    SingleSigCold,
    /// Several signing devices, some of which are needed together.
    Multisig,
}

impl LetterTemplate {
    pub fn parse(s: Option<&str>) -> Result<Self, String> {
        Ok(match s.unwrap_or("family") {
            "family" => LetterTemplate::Family,
            "single_sig_cold" => LetterTemplate::SingleSigCold,
            "multisig" => LetterTemplate::Multisig,
            other => return Err(format!("unknown letter template: {other}")),
        })
    }
}

/// A blank to fill in by hand once the letter is printed.
const BLANK: &str = "______________________________";

/// Said in both the letter and the advanced packet, in words a family can follow.
const DOES_NOT_CONTAIN: [&str; 5] = [
    "No seed words (the 12 or 24 recovery words).",
    "No PINs, passwords or the Nakos vault passphrase.",
    "No private keys. Nothing in here can move or spend the bitcoin.",
    "No login for any exchange, bank or website.",
    "Not the bitcoin itself. It is a map to what exists and where the real keys are kept.",
];

const NEVER_SHARE: [&str; 5] = [
    "Never share the seed words, a PIN or a passphrase with anyone, including someone who says they are from Nakos, a wallet company, a bank, the police or support.",
    "Never type seed words into a website, an app, an email or a chat. Never photograph them or store them in the cloud.",
    "Nobody legitimate will ever ask for them. Anyone who asks, or who rushes you, is trying to steal the bitcoin.",
    "Don't post about the inheritance or the amount online, and don't tell people outside the family how much there is.",
    "If you're unsure, stop and wait. Bitcoin doesn't expire, and there is no deadline.",
];

/// The human part of the simple letter (everything above the ADVANCED heading). Deliberately free
/// of technical key terms. Honest split: Nakos shows the bitcoin, and spending needs the signing
/// device or seed backup the family already has.
pub fn render_simple_body(p: &Packet) -> String {
    render_letter_body(p, LetterTemplate::Family)
}

pub fn render_letter_body(p: &Packet, t: LetterTemplate) -> String {
    let mut o = String::new();
    o.push_str("# A letter about my bitcoin\n\n");
    o.push_str("*Written with Nakos. Keep this file private and share it only with people you trust.*\n\n");

    o.push_str("## What this is\n\n");
    o.push_str("I own some bitcoin. Bitcoin has no help desk and no password reset, so this letter explains what exists, where the important things are kept, and what to do, step by step. ");
    o.push_str(match t {
        LetterTemplate::Family => "You don't need to understand bitcoin to follow it, and you don't need to hurry.\n\n",
        LetterTemplate::SingleSigCold => "My bitcoin is in cold storage: one small signing device (a hardware wallet) and one written backup of its recovery words. Either one, with its PIN, can be used to recover it.\n\n",
        LetterTemplate::Multisig => "My bitcoin is protected by several signing devices kept in different places. Some of them are needed together to move it, so losing one is not a disaster, and no single device or person can move it alone.\n\n",
    });

    o.push_str("## What you need to do\n\n");
    o.push_str("1. Keep the signing device and any written backup safe where they are listed below. Never photograph them, type them into a website or app, or hand them to anyone who calls or emails you.\n");
    o.push_str(&format!("2. Install Nakos from the USB drive or folder that contains this letter (where to look: {BLANK}), or use the Nakos app I left with this letter. Nakos is not in an app store. It is free.\n"));
    o.push_str("3. Open Nakos, go to Inheritance, choose \"Import a letter or packet\", and pick this file.\n");
    o.push_str("4. You will then see the bitcoin. You cannot send any of it until a helper uses the signing device.\n");
    match t {
        LetterTemplate::Family => o.push_str(&format!("5. When you're ready to do something with it, ask the helper named below. Helper: {BLANK}\n\n")),
        LetterTemplate::SingleSigCold => o.push_str(&format!(
            "5. To move the bitcoin, the helper needs the signing device and its PIN, or the written recovery words if the device is lost or broken. Bring the helper to the device, never the words to the helper's computer. Helper: {BLANK}\n\n"
        )),
        LetterTemplate::Multisig => {
            let need = p.wallets.iter().filter_map(|w| w.multisig.as_ref()).map(|m| format!("{} of the {}", m.threshold_m, m.total_n)).next();
            o.push_str(&format!(
                "5. To move the bitcoin, the helper needs {} signing devices listed below, with their PINs, used one after the other. They don't have to be in the same room. Helper: {BLANK}\n\n",
                need.as_deref().unwrap_or("several of the")
            ));
        }
    }

    o.push_str("## Where things are kept\n\n");
    o.push_str("*Fill these in by hand on the printed copy. Say where, never what the words or PINs are.*\n\n");
    match t {
        LetterTemplate::Family => {
            o.push_str(&format!("- Signing device (hardware wallet): {BLANK}\n"));
            o.push_str(&format!("- Written backup of the recovery words: {BLANK}\n"));
            o.push_str(&format!("- PIN or passphrase (where it is written, not the PIN itself): {BLANK}\n"));
        }
        LetterTemplate::SingleSigCold => {
            o.push_str(&format!("- Hardware wallet (make and model: {BLANK}): {BLANK}\n"));
            o.push_str(&format!("- Recovery words, on paper or metal: {BLANK}\n"));
            o.push_str(&format!("- Device PIN (where it is written): {BLANK}\n"));
            o.push_str(&format!("- Extra passphrase, if I used one (where it is written): {BLANK}\n"));
        }
        LetterTemplate::Multisig => {
            let cosigners: Vec<&PacketCosigner> = p.wallets.iter().filter_map(|w| w.multisig.as_ref()).flat_map(|m| m.cosigners.iter()).collect();
            if cosigners.is_empty() {
                o.push_str(&format!("- Signing device 1: {BLANK}\n- Signing device 2: {BLANK}\n- Signing device 3: {BLANK}\n"));
            }
            for c in cosigners {
                let model = c.hardware_model.as_deref().map(|m| format!(" ({m})")).unwrap_or_default();
                o.push_str(&format!("- {}{model}: {BLANK}. Its PIN is written at: {BLANK}\n", c.label));
            }
            o.push_str(&format!("- The written backup for each device: {BLANK}\n"));
            o.push_str(&format!("- The helper's copy of the technical packet: {BLANK}\n"));
        }
    }
    o.push_str(&format!("- Other papers (exchange accounts, tax records): {BLANK}\n\n"));

    o.push_str("## What never to share\n\n");
    for n in NEVER_SHARE {
        o.push_str(&format!("- {n}\n"));
    }

    o.push_str("\n## What this letter does not contain\n\n");
    for n in DOES_NOT_CONTAIN {
        o.push_str(&format!("- {n}\n"));
    }

    o.push_str("\n## What you will see\n\n");
    if p.wallets.is_empty() {
        o.push_str("- No wallets were included in this letter.\n");
    }
    for w in &p.wallets {
        let status = if w.heir_can_reconstruct { "Nakos can show this one" } else { "this one needs a helper" };
        let balance = w
            .snapshot
            .as_ref()
            .map(|sn| format!(" Last known balance: {} BTC (as of {}).", sats_to_btc(sn.balance_sats), sn.as_of.get(..10).unwrap_or(&sn.as_of)))
            .unwrap_or_default();
        o.push_str(&format!("- **{}**: {status}.{balance}\n", w.name));
        if let Some(n) = &w.note {
            o.push_str(&format!("  - My note: {}\n", n.replace('\n', " ")));
        }
    }
    o.push_str("\nBalances are as of the date shown. To refresh them, ask a helper to connect Nakos to a bitcoin server.\n");
    if let Some(l) = &p.letter {
        o.push_str(&format!("\n## A note from me\n\n{l}\n"));
    }
    if let Some(d) = &p.review_by {
        o.push_str(&format!("\n*I planned to review this letter by {d}.*\n"));
    }
    o
}

/// Build the complete simple-letter file: human body, then the machine
/// block. The data block carries no servers (`nodes: []`, generic checklist),
/// and its `letter` field holds the human body (which already includes any
/// notes the user opted in to), so importing keeps the family's words.
pub fn build_simple_letter(p: Packet) -> Result<String, String> {
    build_letter(p, LetterTemplate::Family)
}

pub fn build_letter(mut p: Packet, t: LetterTemplate) -> Result<String, String> {
    let body = render_letter_body(&p, t);
    p.nodes.clear();
    p.checklist = build_helper_checklist();
    for w in p.wallets.iter_mut() {
        w.note = None; // already in the body
        w.cost_basis.clear(); // tax data never rides in the family letter
    }
    p.letter = Some(body.clone());
    let machine = serde_json::to_string_pretty(&p).map_err(|e| e.to_string())?;
    Ok(format!(
        "{body}\n{ADVANCED_HEADING}\n\nOnly needed by whoever helps with setup. Nakos reads this section by itself.\n\n{MACHINE_BLOCK_OPEN}\n{machine}\n```\n"
    ))
}

/// Accepts a raw packet JSON file (.json / .nakos-inherit) or a simple
/// letter with the machine block. The advanced Markdown packet is readable
/// prose, not importable data, and says so.
pub fn extract_packet(text: &str) -> Result<Packet, String> {
    let trimmed = text.trim_start();
    let json: &str = if trimmed.starts_with('{') {
        trimmed
    } else if let Some(start) = text.find(MACHINE_BLOCK_OPEN) {
        let rest = &text[start + MACHINE_BLOCK_OPEN.len()..];
        let end = rest.find("\n```").ok_or("the machine-readable section of this letter is cut off")?;
        rest[..end].trim()
    } else {
        return Err("this file has nothing Nakos can import. Use the simple letter or the JSON packet (the advanced Markdown packet is for reading).".to_string());
    };
    let packet: Packet = serde_json::from_str(json).map_err(|e| format!("couldn't read the packet data: {e}"))?;
    if packet.format_version == 0 || packet.format_version > 2 {
        return Err(format!("unsupported packet version {}", packet.format_version));
    }
    Ok(packet)
}

// ---------------------------------------------------------------------------
// Import: recreate watch-only wallets through the normal Add Wallet path
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Default)]
pub struct ImportSummary {
    pub created: Vec<String>,
    /// (wallet name, reason)
    pub skipped: Vec<(String, String)>,
    /// True when the letter/notes text in the file was saved (encrypted).
    pub letter_saved: bool,
    /// Why the letter was not saved, or other one-line remarks.
    pub letter_note: Option<String>,
}

const CUSTODY_TYPES: [&str; 9] =
    ["cold_storage", "multisig_vault", "hot_wallet", "lightning", "mining", "exchange", "etf", "retirement", "other"];

/// Why this wallet should not be created again, if it's already here: the
/// same descriptor/xpub/address, or the same key origin (fingerprint + path
/// on the same network). Archived wallets count, and the reason says so.
fn already_present(conn: &Connection, w: &PacketWallet) -> Result<Option<String>, String> {
    let describe = |archived: bool| if archived { "already in this Nakos (archived)" } else { "already in this Nakos" };
    let candidates: Vec<&str> = [w.descriptor.as_deref(), w.xpub.as_deref(), w.addresses.first().map(String::as_str)]
        .into_iter()
        .flatten()
        .collect();
    for c in candidates {
        let hit: Option<i64> = conn
            .query_row("SELECT archived FROM wallets WHERE descriptor = ?1 OR xpub = ?1 LIMIT 1", params![c], |r| r.get(0))
            .ok();
        if let Some(a) = hit {
            return Ok(Some(describe(a != 0).to_string()));
        }
    }
    if let (Some(fp), Some(path)) = (&w.master_fingerprint, &w.derivation_path) {
        let hit: Option<i64> = conn
            .query_row(
                "SELECT archived FROM wallets WHERE master_fingerprint = ?1 AND derivation_path = ?2 AND network = ?3 LIMIT 1",
                params![fp, path, w.network],
                |r| r.get(0),
            )
            .ok();
        if let Some(a) = hit {
            return Ok(Some(describe(a != 0).to_string()));
        }
    }
    Ok(None)
}

/// Every string from the packet that would be written to the database as plain text. A wallet whose
/// text looks like an extended private key is refused: that text would trip the encrypted-backup
/// guard for good, because nothing in the app can edit or delete the rows it lands in.
fn stored_texts(w: &PacketWallet) -> Vec<Option<&str>> {
    let mut v = vec![Some(w.name.as_str()), w.master_fingerprint.as_deref(), w.derivation_path.as_deref(), w.descriptor.as_deref(), w.change_descriptor.as_deref(), w.xpub.as_deref()];
    v.extend(w.addresses.iter().map(|a| Some(a.as_str())));
    if let Some(sn) = &w.snapshot {
        v.push(Some(sn.as_of.as_str()));
    }
    if let Some(m) = &w.multisig {
        for c in &m.cosigners {
            v.extend([Some(c.label.as_str()), Some(c.xpub.as_str()), c.fingerprint.as_deref(), c.derivation_path.as_deref(), c.hardware_model.as_deref()]);
        }
    }
    v
}

/// Most coins a snapshot may claim. A real wallet holds far fewer; this only rejects nonsense.
const MAX_SNAPSHOT_COINS: i64 = 10_000_000;

/// Why a packet's last-known balance cannot be trusted, if it cannot. The numbers come from a
/// file anyone can edit: an impossible amount would otherwise be stored and later added to every
/// other balance in the portfolio.
fn snapshot_problem(sn: &PacketSnapshot) -> Option<String> {
    if !crate::commands::wallets::plausible_sats(sn.balance_sats) {
        return Some("the packet's last-known balance is not a possible amount (0 to 21,000,000 BTC)".into());
    }
    if !(0..=MAX_SNAPSHOT_COINS).contains(&sn.utxo_count) {
        return Some("the packet's last-known coin count is not a possible number".into());
    }
    None
}

/// Create wallets from a parsed packet. Public material only: no keys, no
/// node rows, no credentials are read from or written by the import. Each
/// wallet is atomic (savepoint) and independent, so one bad entry is
/// reported and skipped without blocking the rest.
pub fn import_packet(conn: &Connection, packet: &Packet) -> Result<ImportSummary, String> {
    let mut summary = ImportSummary::default();
    for w in &packet.wallets {
        if let Err(reason) = refuse_private_key_material(stored_texts(w)) {
            // Do not echo a name that is itself the offending text.
            let shown = if refuse_private_key_material([Some(w.name.as_str())]).is_err() { "(unnamed wallet)".to_string() } else { w.name.clone() };
            summary.skipped.push((shown, reason));
            continue;
        }
        if let Some(reason) = w.snapshot.as_ref().and_then(snapshot_problem) {
            summary.skipped.push((w.name.clone(), reason));
            continue;
        }
        if let Some(reason) = already_present(conn, w)? {
            summary.skipped.push((w.name.clone(), reason));
            continue;
        }
        let custody = if CUSTODY_TYPES.contains(&w.custody_type.as_str()) { w.custody_type.clone() } else { "other".to_string() };

        conn.execute_batch("SAVEPOINT import_wallet").map_err(|e| e.to_string())?;
        let result: Result<crate::db::models::Wallet, String> = match w.kind.as_str() {
            "xpub" => match &w.xpub {
                Some(x) => insert_wallet(
                    conn,
                    NewWallet {
                        name: w.name.clone(),
                        custody_type: custody,
                        risk_level: "medium".into(),
                        source_type: if ["xpub", "ypub", "zpub"].contains(&w.source_type.as_str()) { w.source_type.clone() } else { "xpub".into() },
                        descriptor: None,
                        change_descriptor: None,
                        xpub: Some(x.clone()),
                        master_fingerprint: w.master_fingerprint.clone(),
                        derivation_path: w.derivation_path.clone(),
                        network: w.network.clone(),
                        manual_balance_sats: None,
                        manual_cost_basis_fiat: None,
                        color: None,
                        notes: None,
                    },
                ),
                None => Err("no extended public key in the packet".into()),
            },
            "descriptor" => match &w.descriptor {
                Some(d) => insert_wallet(
                    conn,
                    NewWallet {
                        name: w.name.clone(),
                        custody_type: custody,
                        risk_level: "medium".into(),
                        source_type: "descriptor".into(),
                        descriptor: Some(d.clone()),
                        change_descriptor: w.change_descriptor.clone(),
                        xpub: None,
                        master_fingerprint: None,
                        derivation_path: None,
                        network: w.network.clone(),
                        manual_balance_sats: None,
                        manual_cost_basis_fiat: None,
                        color: None,
                        notes: None,
                    },
                ),
                None => Err("no descriptor in the packet".into()),
            },
            "multisig" => match &w.multisig {
                Some(m) => {
                    let cosigners: Vec<Cosigner> = m
                        .cosigners
                        .iter()
                        .map(|c| Cosigner {
                            id: String::new(),
                            label: c.label.clone(),
                            xpub: c.xpub.clone(),
                            fingerprint: c.fingerprint.clone(),
                            derivation_path: c.derivation_path.clone(),
                            is_mine: false,
                            hardware_model: c.hardware_model.clone(),
                        })
                        .collect();
                    u8::try_from(m.threshold_m)
                        .map_err(|_| "invalid multisig threshold".to_string())
                        .and_then(|t| insert_multisig_wallet(conn, w.name.clone(), custody, "medium".into(), w.network.clone(), t, &cosigners, &m.script_type))
                }
                None => Err("no multisig details in the packet".into()),
            },
            "address_only" => match w.addresses.first() {
                Some(a) => insert_wallet(
                    conn,
                    NewWallet {
                        name: w.name.clone(),
                        custody_type: custody,
                        risk_level: "medium".into(),
                        source_type: "address".into(),
                        descriptor: Some(a.clone()),
                        change_descriptor: None,
                        xpub: None,
                        master_fingerprint: None,
                        derivation_path: None,
                        network: w.network.clone(),
                        manual_balance_sats: None,
                        manual_cost_basis_fiat: None,
                        color: None,
                        notes: None,
                    },
                ),
                None => Err("no address in the packet".into()),
            },
            other => Err(format!("{other} wallets can't be imported")),
        };

        match result {
            Ok(created) => {
                if let Some(sn) = &w.snapshot {
                    conn.execute(
                        "INSERT INTO wallet_snapshots (wallet_id, balance_sats, utxo_count, as_of) VALUES (?1,?2,?3,?4)",
                        params![created.id, sn.balance_sats, sn.utxo_count, sn.as_of],
                    )
                    .map_err(|e| e.to_string())?;
                }
                conn.execute_batch("RELEASE import_wallet").map_err(|e| e.to_string())?;
                summary.created.push(w.name.clone());
            }
            Err(reason) => {
                conn.execute_batch("ROLLBACK TO import_wallet; RELEASE import_wallet").map_err(|e| e.to_string())?;
                summary.skipped.push((w.name.clone(), reason));
            }
        }
    }
    Ok(summary)
}

/// Read a packet or simple letter the user picks in the native open dialog (opened here, in
/// Rust, so the webview never names the path) and create watch-only wallets from it. `None` when
/// cancelled. Nodes and notes in the file are ignored: no credentials are ever imported.
#[tauri::command]
pub async fn import_inheritance_packet(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<Option<ImportSummary>, String> {
    let Some(file) = super::file_dialog::pick_open_file(&app, "Choose a letter or packet", "Nakos letter or packet", &["md", "json", "nakos-inherit", "txt"]).await? else {
        return Ok(None);
    };
    import_packet_file(&state, || super::file_dialog::read_picked(&app, &file, MAX_PACKET_BYTES, TOO_LARGE_FOR_A_PACKET)).map(Some)
}

const MAX_PACKET_BYTES: u64 = 8 * 1024 * 1024;
const TOO_LARGE_FOR_A_PACKET: &str = "that file is too large to be a packet";

/// Import from the bytes `read` returns (the chosen file, read with the size cap).
fn import_packet_file(state: &AppState, read: impl FnOnce() -> Result<Vec<u8>, String>) -> Result<ImportSummary, String> {
    let text = String::from_utf8(read()?).map_err(|_| "that file is not a text letter or packet".to_string())?;
    let packet = extract_packet(&text)?;
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut summary = import_packet(&conn, &packet)?;

    // Keep the family's words. Never overwrite a letter the heir already
    // wrote, and never fail the wallet import because of it.
    if let Some(letter) = packet.letter.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        match save_letter_if_empty(state, &conn, letter) {
            Ok(true) => summary.letter_saved = true,
            Ok(false) => summary.letter_note = Some("Your own Inheritance letter was kept; the letter in the file was not saved.".into()),
            Err(e) => summary.letter_note = Some(format!("The letter in the file was not saved: {e}")),
        }
    }
    Ok(summary)
}

/// Save `text` as the global (encrypted) Inheritance letter unless one
/// exists. Returns whether it was saved.
fn save_letter_if_empty(state: &AppState, conn: &Connection, text: &str) -> Result<bool, String> {
    let id = get_or_create_config(conn, &None)?;
    let has: bool = conn
        .query_row("SELECT instructions_encrypted IS NOT NULL FROM inheritance_configs WHERE id = ?1", params![id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if has {
        return Ok(false);
    }
    let vault = state.vault()?;
    let guard = vault.lock();
    let key = guard.master_key.as_ref().ok_or("unlock the vault, then import again to keep it")?;
    let (nonce, ct) = key.encrypt(text.as_bytes()).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE inheritance_configs SET instructions_encrypted = ?1, instructions_nonce = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?3",
        params![ct, nonce, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Packet for on-screen display: never includes notes.
#[tauri::command]
pub fn get_inheritance_packet(state: tauri::State<AppState>, include_archived: Option<bool>) -> Result<Packet, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    load_packet(&state, &conn, include_archived.unwrap_or(false), false)
}

/// Write the packet to a file the user picks in the native save dialog (opened here, in Rust, so
/// the webview never names the path). `format` is "json", "markdown" (advanced) or "letter"
/// (simple, for family). Notes are included only when asked, and that requires an unlocked vault.
/// Returns the path written, or `None` when cancelled. Content is never logged.
#[tauri::command]
pub async fn export_inheritance_packet(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    format: String,
    include_notes: bool,
    include_archived: bool,
    include_basis: Option<bool>,
    template: Option<String>,
) -> Result<Option<String>, String> {
    let template = LetterTemplate::parse(template.as_deref())?;
    let (title, default_name, filter, ext) = match format.as_str() {
        "json" => ("Save advanced packet", "nakos-inheritance-packet.json", "JSON", "json"),
        "markdown" => ("Save advanced packet", "nakos-inheritance-packet.md", "Markdown", "md"),
        "markdown_pdf" => ("Save advanced packet (PDF)", "nakos-recovery-packet.pdf", "PDF", "pdf"),
        "letter" => ("Save letter for family", "letter-for-family.md", "Markdown", "md"),
        "letter_pdf" => ("Save letter for family (PDF)", "letter-for-family.pdf", "PDF", "pdf"),
        other => return Err(format!("unknown export format: {other}")),
    };
    let Some(file) = super::file_dialog::pick_save_file(&app, title, default_name, filter, &[ext]).await? else {
        return Ok(None);
    };
    let write = |bytes: &[u8]| super::file_dialog::write_picked(&app, &file, bytes);
    export_packet_file(&state, &super::file_dialog::shown(&file), write, &format, include_notes, include_archived, include_basis, template).map(Some)
}

/// Build the letter or packet and hand it to `write`; `shown` names the destination.
fn export_packet_file(
    state: &AppState,
    shown: &str,
    write: impl FnOnce(&[u8]) -> Result<(), String>,
    format: &str,
    include_notes: bool,
    include_archived: bool,
    include_basis: Option<bool>,
    template: LetterTemplate,
) -> Result<String, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut packet = load_packet(state, &conn, include_archived, include_notes)?;
    // Cost basis: opt-in, advanced JSON only (never Markdown, never the letter).
    if include_basis.unwrap_or(false) && format == "json" {
        attach_basis(&conn, &mut packet)?;
    }
    let body: Vec<u8> = match format {
        "json" => serde_json::to_string_pretty(&packet).map_err(|e| e.to_string())?.into_bytes(),
        "markdown" => render_markdown(&packet).into_bytes(),
        "markdown_pdf" => super::pdf::markdown_to_pdf("Nakos recovery packet (watch-only)", &render_markdown(&packet)),
        "letter" => build_letter(packet, template)?.into_bytes(),
        // The printable letter is the family's part only. The importable copy is the .md letter.
        "letter_pdf" => {
            let mut body = render_letter_body(&packet, template);
            body.push_str("\n## For whoever helps\n\nTo bring these wallets into Nakos, import the file letter-for-family.md saved with this printout. This paper copy is for reading and for filling in the blanks.\n");
            super::pdf::markdown_to_pdf("A letter about my bitcoin", &body)
        }
        other => return Err(format!("unknown export format: {other}")),
    };
    write(&body)?;
    Ok(shown.to_string())
}

#[derive(Debug, Serialize)]
pub struct ReviewReminder {
    pub review_by: Option<String>,
    pub note: Option<String>,
    pub overdue: bool,
}

#[tauri::command]
pub fn get_review_reminder(state: tauri::State<AppState>) -> Result<ReviewReminder, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let review_by: Option<String> = conn
        .query_row("SELECT review_by FROM inheritance_configs WHERE wallet_id IS NULL", [], |r| r.get(0))
        .ok()
        .flatten();
    let overdue = review_by
        .as_deref()
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .map(|d| d < chrono::Local::now().date_naive())
        .unwrap_or(false);
    // A locked vault just hides the note; the date still shows.
    let note = decrypt_review_note(&state, &conn).unwrap_or(None);
    Ok(ReviewReminder { review_by, note, overdue })
}

/// Local reminder only: a date and an encrypted note. No email, no network.
/// Empty/None `review_by` clears the reminder.
#[tauri::command]
pub fn save_review_reminder(state: tauri::State<AppState>, review_by: Option<String>, note: Option<String>) -> Result<(), String> {
    let review_by = review_by.filter(|d| !d.trim().is_empty());
    if let Some(d) = &review_by {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| "review date must look like YYYY-MM-DD".to_string())?;
    }
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let id = get_or_create_config(&conn, &None)?;

    let (ct, nonce): (Option<Vec<u8>>, Option<Vec<u8>>) = match note.filter(|n| !n.trim().is_empty()) {
        Some(n) => {
            let vault = state.vault()?;
            let guard = vault.lock();
            let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it before saving the reminder note")?;
            let (nonce, ct) = key.encrypt(n.as_bytes()).map_err(|e| e.to_string())?;
            (Some(ct), Some(nonce))
        }
        None => (None, None),
    };
    conn.execute(
        "UPDATE inheritance_configs SET review_by = ?1, review_note_encrypted = ?2, review_note_nonce = ?3,
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?4",
        params![review_by, ct, nonce, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wallet(source_type: &str) -> Wallet {
        Wallet {
            id: "w1".into(),
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

    fn packet_with(wallets: Vec<PacketWallet>) -> Packet {
        Packet {
            format_version: 1,
            generated_at: "2026-01-01T00:00:00Z".into(),
            watch_only_notice: WATCH_ONLY_NOTICE.into(),
            wallets,
            nodes: vec![
                PacketNode {
                    label: "core".into(),
                    kind: "bitcoin_core_rpc".into(),
                    host: "node.example".into(),
                    port: 8332,
                    use_tls: false,
                    use_tor: false,
                    cookie_path: Some("/data/bitcoin/.cookie".into()),
                    is_default: true,
                },
                PacketNode {
                    label: "electrs".into(),
                    kind: "electrum".into(),
                    host: "node.example".into(),
                    port: 50001,
                    use_tls: false,
                    use_tor: false,
                    cookie_path: None,
                    is_default: true,
                },
            ],
            checklist: build_checklist(Some("node.example:50001")),
            letter: None,
            review_by: Some("2027-01-01".into()),
            review_note: None,
        }
    }

    fn collect_keys(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, v) in m {
                    out.push(k.to_lowercase());
                    collect_keys(v, out);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| collect_keys(x, out)),
            _ => {}
        }
    }

    #[test]
    fn export_omits_password_and_secret_fields() {
        let mut w = wallet("xpub");
        w.xpub = Some("xpub-test".into());
        let p = packet_with(vec![wallet_to_packet(&w, None, vec![], None)]);
        let value = serde_json::to_value(&p).unwrap();
        let mut keys = Vec::new();
        collect_keys(&value, &mut keys);
        for banned in ["password", "passphrase", "rpc_user", "rpcuser", "seed", "mnemonic", "private", "secret", "token", "explorer"] {
            assert!(!keys.iter().any(|k| k.contains(banned)), "serialized packet has a `{banned}` field: {keys:?}");
        }
        // Cookie is exported as a path string only.
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("/data/bitcoin/.cookie"));
        // Markdown carries the same data and no credential lines.
        let md = render_markdown(&p).to_lowercase();
        assert!(!md.contains("rpcuser=") && !md.contains("rpcpassword="));
    }

    #[test]
    fn address_only_wallet_is_flagged_and_gets_no_descriptor() {
        let pw = wallet_to_packet(&wallet("address"), None, vec!["bc1qexample".into()], None);
        assert_eq!(pw.kind, "address_only");
        assert!(!pw.heir_can_reconstruct);
        assert!(pw.descriptor.is_none() && pw.change_descriptor.is_none() && pw.xpub.is_none());
        assert!(pw.heir_warning.as_deref().unwrap().contains("extra information you store in notes"));
        assert_eq!(pw.addresses, vec!["bc1qexample".to_string()]);
    }

    #[test]
    fn xpub_wallet_without_fingerprint_can_be_watched_with_a_warning() {
        let mut w = wallet("zpub");
        w.xpub = Some("zpub-test".into());
        let pw = wallet_to_packet(&w, None, vec!["ignored".into()], None);
        assert_eq!(pw.kind, "xpub");
        assert!(pw.heir_can_reconstruct);
        assert!(pw.heir_warning.is_some());
        assert!(pw.addresses.is_empty(), "addresses are only listed for address-only wallets");
    }

    #[test]
    fn complete_descriptor_wallet_has_no_warning() {
        let mut w = wallet("descriptor");
        w.descriptor = Some("wpkh([aabbccdd/84'/0'/0']xpub/0/*)".into());
        let pw = wallet_to_packet(&w, None, vec![], None);
        assert!(pw.heir_can_reconstruct && pw.heir_warning.is_none());
    }

    #[test]
    fn checklist_shows_saved_electrs_as_replaceable_not_mandatory() {
        let c = build_checklist(Some("node.example:50001")).join("\n");
        assert!(c.contains("node.example:50001") && c.contains("Replace with yours"));
        assert!(build_checklist(None).join("\n").contains("no Electrum server is saved"));
    }

    // ---- simple letter / import ------------------------------------------

    const TEST_XPUB_1: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
    const TEST_XPUB_2: &str = "xpub661MyMwAqRbcFW31YEwpkMuc5THy2PSt5bDMsktWQcFF8syAmRUapSCGu8ED9W6oDMSgv6Zz8idoc4a6mr8BDzTJY47LJhkJ8UB7WEGuduB";
    const TEST_XPUB_3: &str = "xpub661MyMwAqRbcEZVB4dScxMAdx6d4nFc9nvyvH3v4gJL378CSRZiYmhRoP7mBy6gSPSCYk6SzXPTf3ND1cZAceL7SfJ1Z3GC8vBgp2epUt13";

    fn snap(sats: i64) -> Option<PacketSnapshot> {
        Some(PacketSnapshot { balance_sats: sats, utxo_count: 2, as_of: "2026-09-01T10:00:00Z".into() })
    }

    /// One xpub wallet, one 2-of-3 multisig, one address-only wallet.
    fn fixture_packet() -> Packet {
        let mut xw = wallet("xpub");
        xw.id = "a".into();
        xw.name = "Savings".into();
        xw.xpub = Some(TEST_XPUB_1.into());
        xw.master_fingerprint = Some("aabbccdd".into());
        xw.derivation_path = Some("84'/0'/0'".into());

        let mut mw = wallet("multisig_descriptor");
        mw.id = "b".into();
        mw.name = "Family vault".into();
        let ms = PacketMultisig {
            threshold_m: 2,
            total_n: 3,
            script_type: "p2wsh".into(),
            cosigners: [(TEST_XPUB_1, "aaaaaaaa"), (TEST_XPUB_2, "bbbbbbbb"), (TEST_XPUB_3, "cccccccc")]
                .iter()
                .enumerate()
                .map(|(i, (x, f))| PacketCosigner {
                    label: format!("Key {}", i + 1),
                    xpub: x.to_string(),
                    fingerprint: Some(f.to_string()),
                    derivation_path: Some("48'/0'/0'/2'".into()),
                    hardware_model: None,
                })
                .collect(),
        };
        // A real export carries the exact stored descriptor; build the same one.
        let cosigner_rows: Vec<Cosigner> = ms
            .cosigners
            .iter()
            .map(|c| Cosigner {
                id: String::new(),
                label: c.label.clone(),
                xpub: c.xpub.clone(),
                fingerprint: c.fingerprint.clone(),
                derivation_path: c.derivation_path.clone(),
                is_mine: false,
                hardware_model: None,
            })
            .collect();
        let pair = crate::bitcoin::descriptors::descriptors_from_multisig(2, &cosigner_rows, bitcoin::Network::Bitcoin, "p2wsh")
            .expect("valid multisig fixture");
        mw.descriptor = Some(pair.external);
        mw.change_descriptor = Some(pair.internal);

        let mut aw = wallet("address");
        aw.id = "c".into();
        aw.name = "Cabin".into();

        packet_with(vec![
            wallet_to_packet(&xw, None, vec![], snap(1_250_000)),
            wallet_to_packet(&mw, Some(ms), vec![], snap(50_000_000)),
            wallet_to_packet(&aw, None, vec!["bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4".into()], None),
        ])
    }

    #[test]
    fn advanced_markdown_is_rejected_with_a_clear_message() {
        let md = render_markdown(&fixture_packet());
        let err = extract_packet(&md).unwrap_err();
        assert!(err.contains("simple letter or the JSON packet"), "{err}");
    }

    fn mem_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn
    }

    #[test]
    fn import_creates_watch_only_wallets_without_secrets() {
        let conn = mem_db();
        let summary = import_packet(&conn, &fixture_packet()).expect("import runs");
        assert_eq!(summary.created, vec!["Savings", "Family vault", "Cabin"], "skipped: {:?}", summary.skipped);
        assert!(summary.skipped.is_empty());

        let watch_only: i64 = conn.query_row("SELECT COUNT(*) FROM wallets WHERE is_watch_only = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(watch_only, 3, "every imported wallet is watch-only");
        let multisig_cosigners: i64 = conn.query_row("SELECT COUNT(*) FROM multisig_cosigners", [], |r| r.get(0)).unwrap();
        assert_eq!(multisig_cosigners, 3);
        // Nothing credential-like is created: no node rows, no stored passwords.
        let nodes: i64 = conn.query_row("SELECT COUNT(*) FROM node_configs", [], |r| r.get(0)).unwrap();
        assert_eq!(nodes, 0, "import must not create node configs");
        let pw: i64 = conn.query_row("SELECT COUNT(*) FROM node_configs WHERE rpc_password_encrypted IS NOT NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(pw, 0);
        // Snapshot balances are stored for wallets that had one.
        let snaps: i64 = conn.query_row("SELECT COUNT(*) FROM wallet_snapshots", [], |r| r.get(0)).unwrap();
        assert_eq!(snaps, 2);
        // Address-only wallet holds its real address and no invented descriptor material.
        let (desc, xpub): (Option<String>, Option<String>) = conn
            .query_row("SELECT descriptor, xpub FROM wallets WHERE name = 'Cabin'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(desc.as_deref(), Some("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"));
        assert!(xpub.is_none());
    }

    #[test]
    fn importing_twice_skips_duplicates_and_reports_bad_entries() {
        let conn = mem_db();
        import_packet(&conn, &fixture_packet()).unwrap();
        let again = import_packet(&conn, &fixture_packet()).unwrap();
        assert!(again.created.is_empty());
        assert_eq!(again.skipped.len(), 3);

        // A wallet whose key is garbage is skipped with a reason and leaves nothing behind.
        let mut bad = fixture_packet();
        bad.wallets.truncate(1);
        bad.wallets[0].name = "Broken".into();
        bad.wallets[0].xpub = Some("not-an-xpub".into());
        let conn2 = mem_db();
        let s = import_packet(&conn2, &bad).unwrap();
        assert!(s.created.is_empty() && s.skipped.len() == 1);
        let n: i64 = conn2.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn import_refuses_a_packet_wallet_carrying_an_extended_private_key() {
        let mut bad = fixture_packet();
        bad.wallets.truncate(1);
        bad.wallets[0].xpub = Some("xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi".into());
        let conn = mem_db();
        let s = import_packet(&conn, &bad).unwrap();
        assert!(s.created.is_empty() && s.skipped.len() == 1);
        assert!(s.skipped[0].1.contains("private key material is not stored"), "reason: {}", s.skipped[0].1);
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn node_hygiene_drops_9332_and_duplicates() {
        let node = |label: &str, kind: &str, host: &str, port: i64| PacketNode {
            label: label.into(),
            kind: kind.into(),
            host: host.into(),
            port,
            use_tls: false,
            use_tor: false,
            cookie_path: None,
            is_default: false,
        };
        let cleaned = clean_nodes(vec![
            node("core", "bitcoin_core_rpc", "node.example", 8332),
            node("core dup", "bitcoin_core_rpc", "NODE.example", 8332),
            node("dead", "bitcoin_core_rpc", "node.example", 9332),
            node("electrs", "electrum", "node.example", 50001),
        ]);
        let labels: Vec<&str> = cleaned.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, vec!["core", "electrs"]);
    }

    // ---- simple letter v2 --------------------------------------------------

    fn letter_fixture() -> Packet {
        let mut p = fixture_packet();
        p.letter = Some("Call my sister Anna. The safe key is in the blue book.".into());
        p.wallets[0].note = Some("Ledger is in the desk drawer".into());
        p
    }

    #[test]
    fn simple_letter_body_has_no_technical_terms_outside_advanced() {
        let letter = build_simple_letter(letter_fixture()).unwrap();
        let (body, advanced) = letter.split_once(ADVANCED_HEADING).expect("has an ADVANCED section");
        let body_lc = body.to_lowercase();
        for banned in ["xpub", "electrs", "fingerprint", "descriptor", "derivation"] {
            assert!(!body_lc.contains(banned), "body mentions `{banned}`:\n{body}");
        }
        assert!(body_lc.contains("you cannot send"), "must say spending needs the signing device");
        assert!(body_lc.contains("usb drive or folder") && body_lc.contains("not in an app store"), "step 2 says where Nakos comes from");
        assert!(body_lc.contains("cabin") && body_lc.contains("needs a helper"));
        assert!(body_lc.contains("0.01250000 btc"));
        assert!(body.contains("Call my sister Anna") && body.contains("Ledger is in the desk drawer"), "user's words are in the body");
        assert!(advanced.contains("xpub") && advanced.contains(MACHINE_BLOCK_OPEN));
    }

    #[test]
    fn simple_letter_data_block_keeps_the_prose_and_carries_no_servers() {
        let letter = build_simple_letter(letter_fixture()).unwrap();
        let p = extract_packet(&letter).expect("letter imports");
        let saved = p.letter.as_deref().expect("`letter` is not null");
        assert!(saved.contains("Call my sister Anna") && saved.contains("Ledger is in the desk drawer"));
        assert!(saved.contains("## What you need to do"), "the family prose itself is persisted");
        assert!(p.nodes.is_empty(), "simple letter exports nodes: []");
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("node.example") && !json.contains("50001"), "no server details leak into the simple data block");
        let mut keys = Vec::new();
        collect_keys(&serde_json::to_value(&p).unwrap(), &mut keys);
        for banned in ["password", "passphrase", "rpc_user", "seed", "mnemonic", "private", "secret"] {
            assert!(!keys.iter().any(|k| k.contains(banned)), "`{banned}` key in simple data block");
        }
    }

    #[test]
    fn every_letter_template_has_the_sections_says_what_it_lacks_and_stays_non_technical() {
        for t in [LetterTemplate::Family, LetterTemplate::SingleSigCold, LetterTemplate::Multisig] {
            let letter = build_letter(letter_fixture(), t).unwrap();
            let (body, _) = letter.split_once(ADVANCED_HEADING).unwrap();
            for heading in ["## What this is", "## What you need to do", "## Where things are kept", "## What never to share", "## What this letter does not contain", "## What you will see"] {
                assert!(body.contains(heading), "{t:?} lacks {heading}");
            }
            assert!(body.contains("No seed words") && body.contains("No private keys"), "{t:?}");
            assert!(body.contains(BLANK), "{t:?} has blanks to fill in by hand");
            let lc = body.to_lowercase();
            for banned in ["xpub", "electrs", "fingerprint", "descriptor", "derivation"] {
                assert!(!lc.contains(banned), "{t:?} body mentions `{banned}`");
            }
            let back = extract_packet(&letter).expect("every template imports");
            assert_eq!(back.wallets.len(), 3, "{t:?}");
        }
        let ms = build_letter(letter_fixture(), LetterTemplate::Multisig).unwrap();
        assert!(ms.contains("2 of the 3 signing devices"), "the multisig letter says how many are needed");
        assert!(ms.contains("- Key 1: ") && ms.contains("- Key 3: "), "one line per signing device");
        assert!(build_letter(letter_fixture(), LetterTemplate::SingleSigCold).unwrap().contains("Recovery words, on paper or metal"));
        assert_eq!(LetterTemplate::parse(None).unwrap(), LetterTemplate::Family);
        assert!(LetterTemplate::parse(Some("../x")).is_err());
    }

    #[test]
    fn the_advanced_packet_gives_every_wallet_structured_recovery_details() {
        let md = render_markdown(&fixture_packet());
        assert!(md.contains("## What this packet does not contain") && md.contains("No seed words"));
        assert_eq!(md.matches("**Recovery details**").count(), 3, "one block per wallet");
        assert!(md.contains("- Wallet type: Multisig, 2 of 3 (p2wsh)"));
        assert!(md.contains("- Wallet type: Single-signature, from an extended public key"));
        assert!(md.contains("Descriptor / xpub location: "));
        assert!(md.contains("- Hardware wallet models: Key 1: "));
        assert!(md.contains("- Recommended next steps:\n  1. "));
    }

    #[test]
    fn letter_is_persisted_even_when_the_user_wrote_no_notes() {
        let p = extract_packet(&build_simple_letter(fixture_packet()).unwrap()).unwrap();
        assert!(p.letter.as_deref().is_some_and(|l| l.contains("A letter about my bitcoin")));
    }

    #[test]
    fn letter_and_json_both_round_trip_through_the_importer() {
        let p = fixture_packet();
        let machine = serde_json::to_string_pretty(&p).unwrap();
        let from_letter = extract_packet(&build_simple_letter(p.clone()).unwrap()).expect("letter imports");
        let from_json = extract_packet(&machine).expect("json imports");
        assert_eq!(from_letter.wallets.len(), 3);
        assert_eq!(from_json.wallets[0].xpub.as_deref(), Some(TEST_XPUB_1));
        assert_eq!(from_letter.wallets[0].snapshot, snap(1_250_000));
    }

    #[test]
    fn hand_written_sample_fence_imports_two_watch_only_wallets() {
        let letter = format!(
            "# A letter\n\nSome words.\n\n# ADVANCED — ignore unless asked\n\n```nakos-inherit-json\n{{\n  \"format_version\": 2,\n  \"wallets\": [\n    {{\"id\":\"1\",\"name\":\"Savings\",\"kind\":\"xpub\",\"source_type\":\"xpub\",\"network\":\"bitcoin\",\"archived\":false,\n     \"master_fingerprint\":\"aabbccdd\",\"derivation_path\":\"84'/0'/0'\",\"descriptor\":null,\"change_descriptor\":null,\"xpub\":\"{TEST_XPUB_1}\",\n     \"multisig\":null,\"heir_warning\":null,\"snapshot\":{{\"balance_sats\":42000,\"utxo_count\":1,\"as_of\":\"2026-09-01T00:00:00Z\"}},\"note\":null}},\n    {{\"id\":\"2\",\"name\":\"Cabin\",\"kind\":\"address_only\",\"source_type\":\"address\",\"network\":\"bitcoin\",\"archived\":false,\n     \"master_fingerprint\":null,\"derivation_path\":null,\"descriptor\":null,\"change_descriptor\":null,\"xpub\":null,\n     \"multisig\":null,\"addresses\":[\"bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4\"],\"heir_warning\":null,\"snapshot\":null,\"note\":null}}\n  ],\n  \"letter\": \"Some words.\",\n  \"review_by\": null,\n  \"review_note\": null\n}}\n```\n"
        );
        let packet = extract_packet(&letter).expect("fence parses");
        let conn = mem_db();
        let s = import_packet(&conn, &packet).unwrap();
        assert_eq!(s.created, vec!["Savings", "Cabin"], "skipped: {:?}", s.skipped);
        let watch_only: i64 = conn.query_row("SELECT COUNT(*) FROM wallets WHERE is_watch_only = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(watch_only, 2);
        let snap_sats: i64 = conn.query_row("SELECT balance_sats FROM wallet_snapshots", [], |r| r.get(0)).unwrap();
        assert_eq!(snap_sats, 42_000, "snapshot is stored so Portfolio shows it before any sync");
    }

    #[test]
    fn corrupt_or_missing_data_block_is_an_error_not_a_silent_no_op() {
        assert!(extract_packet("# just a letter, no data").is_err());
        assert!(extract_packet("x\n```nakos-inherit-json\n{ not json\n```\n").is_err());
        assert!(extract_packet("x\n```nakos-inherit-json\n{\"format_version\": 2").is_err(), "cut-off fence");
    }

    #[test]
    fn same_key_origin_is_skipped_even_if_the_key_text_differs() {
        let conn = mem_db();
        import_packet(&conn, &fixture_packet()).unwrap();
        let mut again = fixture_packet();
        again.wallets.truncate(1);
        again.wallets[0].xpub = Some(TEST_XPUB_2.into()); // same fingerprint + path as "Savings"
        let s = import_packet(&conn, &again).unwrap();
        assert!(s.created.is_empty(), "created: {:?}", s.created);
        assert!(s.skipped[0].1.contains("already in this Nakos"));
    }

    fn insert_node(conn: &Connection, kind: &str, label: &str, host: &str, port: i64, enabled: bool, is_default: bool) {
        conn.execute(
            "INSERT INTO node_configs (id, kind, label, host, port, enabled, is_default) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![uuid::Uuid::new_v4().to_string(), kind, label, host, port, enabled as i64, is_default as i64],
        )
        .unwrap();
    }

    fn file_state(tag: &str) -> (crate::test_dir::TestDir, AppState) {
        let dir = crate::test_dir::TestDir::new(&format!("packet-file-{tag}"));
        let state = AppState::default();
        state.inner.lock().pool = Some(crate::db::open_pool(&dir).unwrap());
        (dir, state)
    }

    /// R11: the file steps behind the export and import commands (Rust opens the dialogs and passes
    /// the chosen path): a letter written to a file reads back on another machine, an unknown
    /// format and an oversize or missing file are refused with errors.
    #[test]
    fn a_packet_file_round_trips_and_bad_files_are_refused() {
        let (_sender_dir, sender) = file_state("sender");
        {
            let conn = sender.pool().unwrap().get().unwrap();
            insert_wallet(
                &conn,
                NewWallet {
                    name: "Savings".into(),
                    custody_type: "cold_storage".into(),
                    risk_level: "low".into(),
                    source_type: "xpub".into(),
                    descriptor: None,
                    change_descriptor: None,
                    xpub: Some(TEST_XPUB_1.into()),
                    master_fingerprint: Some("aabbccdd".into()),
                    derivation_path: Some("84'/0'/0'".into()),
                    network: "bitcoin".into(),
                    manual_balance_sats: None,
                    manual_cost_basis_fiat: None,
                    color: None,
                    notes: None,
                },
            )
            .unwrap();
        }
        let dir = crate::test_dir::TestDir::new("packet-out");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("letter.md");
        use super::super::file_dialog::{read_path, write_path};
        let to = |p: &std::path::Path| { let p = p.to_path_buf(); move |b: &[u8]| write_path(&p, b) };
        let from = |p: &std::path::Path| { let p = p.to_path_buf(); move || read_path(&p, MAX_PACKET_BYTES, TOO_LARGE_FOR_A_PACKET) };
        let written = export_packet_file(&sender, file.to_str().unwrap(), to(&file), "letter", false, false, None, LetterTemplate::Family).unwrap();
        assert_eq!(written, file.to_str().unwrap());
        assert!(std::fs::read_to_string(&file).unwrap().contains("Savings"));

        let (_heir_dir, heir) = file_state("heir");
        let summary = import_packet_file(&heir, from(&file)).unwrap();
        assert_eq!(summary.created, vec!["Savings"]);

        assert!(export_packet_file(&sender, file.to_str().unwrap(), to(&file), "pdf", false, false, None, LetterTemplate::Family).unwrap_err().contains("unknown export format"));
        for (fmt, t) in [("letter_pdf", LetterTemplate::Multisig), ("markdown_pdf", LetterTemplate::Family)] {
            let pdf = dir.join(format!("{fmt}.pdf"));
            export_packet_file(&sender, pdf.to_str().unwrap(), to(&pdf), fmt, false, false, None, t).unwrap();
            let bytes = std::fs::read(&pdf).unwrap();
            assert!(bytes.starts_with(b"%PDF-1.4") && bytes.ends_with(b"%%EOF\n"), "{fmt} is a PDF");
            assert!(!crate::crypto::backup::contains_extended_private_key(&bytes), "{fmt}");
        }
        let nowhere = dir.join("no-dir").join("x.md");
        assert!(export_packet_file(&sender, nowhere.to_str().unwrap(), to(&nowhere), "letter", false, false, None, LetterTemplate::Family).unwrap_err().contains("could not write"));
        let big = dir.join("big.md");
        std::fs::write(&big, vec![b'a'; 8 * 1024 * 1024 + 1]).unwrap();
        assert_eq!(import_packet_file(&heir, from(&big)).unwrap_err(), TOO_LARGE_FOR_A_PACKET);
        assert!(import_packet_file(&heir, from(&dir.join("missing.md"))).unwrap_err().contains("could not open"));
        let binary = dir.join("binary.md");
        std::fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(import_packet_file(&heir, from(&binary)).unwrap_err(), "that file is not a text letter or packet");
    }

    #[test]
    fn real_database_round_trip_and_node_filtering() {
        // Sender machine: two wallets, a snapshot, and messy lab node rows.
        let src = mem_db();
        insert_wallet(
            &src,
            NewWallet {
                name: "Savings".into(),
                custody_type: "cold_storage".into(),
                risk_level: "low".into(),
                source_type: "xpub".into(),
                descriptor: None,
                change_descriptor: None,
                xpub: Some(TEST_XPUB_1.into()),
                master_fingerprint: Some("aabbccdd".into()),
                derivation_path: Some("84'/0'/0'".into()),
                network: "bitcoin".into(),
                manual_balance_sats: None,
                manual_cost_basis_fiat: None,
                color: None,
                notes: None,
            },
        )
        .unwrap();
        let wid: String = src.query_row("SELECT id FROM wallets", [], |r| r.get(0)).unwrap();
        src.execute("INSERT INTO wallet_snapshots (wallet_id, balance_sats, utxo_count, as_of) VALUES (?1, 777000, 3, '2026-09-02T00:00:00Z')", params![wid]).unwrap();
        insert_node(&src, "bitcoin_core_rpc", "lab core", "umbrel.local", 8332, true, true);
        insert_node(&src, "bitcoin_core_rpc", "test 4", "umbrel.local", 8332, true, false); // duplicate
        insert_node(&src, "bitcoin_core_rpc", "dead", "10.0.0.5", 9332, true, false); // retired port
        insert_node(&src, "bitcoin_core_rpc", "other lab", "10.0.0.6", 8332, true, false); // second core
        insert_node(&src, "bitcoin_core_rpc", "off", "10.0.0.7", 8332, false, false); // disabled
        insert_node(&src, "electrum", "electrs", "umbrel.local", 50001, true, true);

        let advanced = load_packet_data(&src, false).unwrap();
        let hosts: Vec<(String, String, i64)> = advanced.nodes.iter().map(|n| (n.kind.clone(), n.host.clone(), n.port)).collect();
        assert_eq!(
            hosts,
            vec![
                ("bitcoin_core_rpc".to_string(), "umbrel.local".to_string(), 8332),
                ("electrum".to_string(), "umbrel.local".to_string(), 50001)
            ],
            "advanced export keeps one enabled, non-9332, unique row per kind"
        );

        let letter = build_simple_letter(advanced).unwrap();
        assert!(!letter.contains("umbrel.local"), "the sender's server never appears in the simple letter");

        // Heir machine: clean database.
        let heir = mem_db();
        let s = import_packet(&heir, &extract_packet(&letter).unwrap()).unwrap();
        assert_eq!(s.created, vec!["Savings"]);
        let (sats, count): (i64, i64) = heir.query_row("SELECT balance_sats, utxo_count FROM wallet_snapshots", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((sats, count), (777_000, 3));
        let nodes: i64 = heir.query_row("SELECT COUNT(*) FROM node_configs", [], |r| r.get(0)).unwrap();
        assert_eq!(nodes, 0, "import never creates, enables or defaults a node");
    }

    #[test]
    fn cost_basis_is_opt_in_json_only_and_never_in_the_simple_letter() {
        let conn = mem_db();
        insert_wallet(
            &conn,
            NewWallet {
                name: "Savings".into(),
                custody_type: "cold_storage".into(),
                risk_level: "low".into(),
                source_type: "xpub".into(),
                descriptor: None,
                change_descriptor: None,
                xpub: Some(TEST_XPUB_1.into()),
                master_fingerprint: Some("aabbccdd".into()),
                derivation_path: Some("84'/0'/0'".into()),
                network: "bitcoin".into(),
                manual_balance_sats: None,
                manual_cost_basis_fiat: None,
                color: None,
                notes: None,
            },
        )
        .unwrap();
        let wid: String = conn.query_row("SELECT id FROM wallets", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO utxos (id, wallet_id, txid, vout, amount_sats, spent, acquisition_cost_fiat, acquisition_date, acquisition_note)
             VALUES ('u1', ?1, ?2, 0, 250000, 0, 123.45, '2024-03-01', 'bought at exchange')",
            params![wid, "ab".repeat(32)],
        )
        .unwrap();

        // Default packet: no basis. With attach: present, in JSON.
        let mut p = load_packet_data(&conn, false).unwrap();
        assert!(p.wallets[0].cost_basis.is_empty());
        attach_basis(&conn, &mut p).unwrap();
        assert_eq!(p.wallets[0].cost_basis.len(), 1);
        assert_eq!(p.wallets[0].cost_basis[0].cost_fiat, 123.45);
        assert!(serde_json::to_string(&p).unwrap().contains("123.45"));

        // The simple letter drops it everywhere, body and data block.
        let letter = build_simple_letter(p).unwrap();
        assert!(!letter.contains("123.45") && !letter.contains("bought at exchange") && !letter.contains("cost_basis\": [\n        {"));
        assert!(extract_packet(&letter).unwrap().wallets[0].cost_basis.is_empty());
    }

    #[test]
    fn explorer_token_and_settings_never_reach_any_export() {
        let conn = mem_db();
        insert_wallet(
            &conn,
            NewWallet {
                name: "Savings".into(),
                custody_type: "cold_storage".into(),
                risk_level: "low".into(),
                source_type: "xpub".into(),
                descriptor: None,
                change_descriptor: None,
                xpub: Some(TEST_XPUB_1.into()),
                master_fingerprint: Some("aabbccdd".into()),
                derivation_path: Some("84'/0'/0'".into()),
                network: "bitcoin".into(),
                manual_balance_sats: None,
                manual_cost_basis_fiat: None,
                color: None,
                notes: None,
            },
        )
        .unwrap();
        // Explorer settings as Settings would store them, with a recognisable marker.
        for (k, v) in [
            ("explorer_enabled", "1"),
            ("explorer_base_url", "https://explorer.example.test"),
            ("explorer_token_ct", "TOKEN-CIPHERTEXT-MARKER"),
            ("explorer_token_nonce", "TOKEN-NONCE-MARKER"),
        ] {
            crate::db::meta_set(&conn, k, v).unwrap();
        }

        let p = load_packet_data(&conn, false).unwrap();
        let advanced_json = serde_json::to_string_pretty(&p).unwrap();
        let advanced_md = render_markdown(&p);
        let letter = build_simple_letter(p).unwrap();
        for (name, text) in [("advanced json", &advanced_json), ("advanced markdown", &advanced_md), ("simple letter", &letter)] {
            for needle in ["TOKEN-CIPHERTEXT-MARKER", "TOKEN-NONCE-MARKER", "explorer.example.test", "explorer_"] {
                assert!(!text.contains(needle), "{name} leaked `{needle}`");
            }
        }
    }

    /// Bitcoin Core RPC is plain HTTP, so no export may say the Core node uses TLS: not the
    /// advanced packet's node list, not its data block, and not the simple family letter.
    /// (Electrum TLS is real and is still reported.)
    #[test]
    fn exports_never_claim_tls_for_a_core_node() {
        // The renderer, given a Core node that is (wrongly) flagged TLS.
        let mut p = letter_fixture();
        for n in p.nodes.iter_mut() {
            n.use_tls = true;
        }
        let md = render_markdown(&p);
        let core_line = md.lines().find(|l| l.contains("bitcoin_core_rpc")).expect("core node line");
        let electrum_line = md.lines().find(|l| l.contains("(electrum)")).expect("electrum node line");
        assert!(!core_line.contains("TLS"), "core line: {core_line}");
        assert!(electrum_line.contains(", TLS"), "Electrum TLS is still reported: {electrum_line}");
        assert!(!build_simple_letter(p).unwrap().contains(", TLS"), "the simple letter carries no node details");

        // From a database: a Core row flagged TLS (stored before the rule) exports as no TLS.
        let conn = mem_db();
        conn.execute("INSERT INTO node_configs (id, kind, label, host, port, use_tls) VALUES ('c','bitcoin_core_rpc','core','node.example',8332,1)", []).unwrap();
        conn.execute("INSERT INTO node_configs (id, kind, label, host, port, use_tls) VALUES ('e','electrum','electrs','node.example',50002,1)", []).unwrap();
        let packet = load_packet_data(&conn, false).unwrap();
        let tls_by_kind: Vec<(&str, bool)> = packet.nodes.iter().map(|n| (n.kind.as_str(), n.use_tls)).collect();
        assert_eq!(tls_by_kind, vec![("bitcoin_core_rpc", false), ("electrum", true)]);
    }

    /// R7: the snapshot's numbers come from a file anyone can edit. An impossible balance or coin
    /// count skips that wallet (reported, nothing written for it); the others still import, and
    /// the largest possible values are accepted.
    #[test]
    fn import_skips_a_wallet_whose_snapshot_is_not_a_possible_balance() {
        use crate::commands::wallets::MAX_SATS;
        type Tweak = Box<dyn Fn(&mut PacketSnapshot)>;
        let hostile: Vec<(&str, Tweak)> = vec![
            ("i64::MAX balance", Box::new(|s: &mut PacketSnapshot| s.balance_sats = i64::MAX)),
            ("negative balance", Box::new(|s: &mut PacketSnapshot| s.balance_sats = -1)),
            ("just over 21 million BTC", Box::new(|s: &mut PacketSnapshot| s.balance_sats = MAX_SATS + 1)),
            ("negative coin count", Box::new(|s: &mut PacketSnapshot| s.utxo_count = -1)),
            ("absurd coin count", Box::new(|s: &mut PacketSnapshot| s.utxo_count = i64::MAX)),
        ];
        for (what, tweak) in hostile {
            let mut p = fixture_packet();
            tweak(p.wallets[0].snapshot.as_mut().unwrap());
            let conn = mem_db();
            let summary = import_packet(&conn, &p).unwrap();
            assert_eq!(summary.skipped.len(), 1, "{what}: {:?}", summary.skipped);
            assert!(summary.skipped[0].1.contains("not a possible"), "{what}: {:?}", summary.skipped);
            assert_eq!(summary.skipped[0].0, p.wallets[0].name, "{what}");
            assert_eq!(summary.created.len(), 2, "{what}: the other wallets still import");
            let wallets: i64 = conn.query_row("SELECT COUNT(*) FROM wallets WHERE name = ?1", params![p.wallets[0].name], |r| r.get(0)).unwrap();
            assert_eq!(wallets, 0, "{what}: nothing written for the skipped wallet");
            let bad_rows: i64 = conn.query_row("SELECT COUNT(*) FROM wallet_snapshots WHERE balance_sats < 0 OR balance_sats > ?1", params![MAX_SATS], |r| r.get(0)).unwrap();
            assert_eq!(bad_rows, 0, "{what}");
        }
        // The edges themselves are fine.
        let mut p = fixture_packet();
        let sn = p.wallets[0].snapshot.as_mut().unwrap();
        sn.balance_sats = MAX_SATS;
        sn.utxo_count = MAX_SNAPSHOT_COINS;
        let conn = mem_db();
        let summary = import_packet(&conn, &p).unwrap();
        assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
        let stored: i64 = conn.query_row("SELECT balance_sats FROM wallet_snapshots ORDER BY balance_sats DESC LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(stored, MAX_SATS);
        let mut p = fixture_packet();
        let sn = p.wallets[0].snapshot.as_mut().unwrap();
        sn.balance_sats = 0;
        sn.utxo_count = 0;
        assert!(import_packet(&mem_db(), &p).unwrap().skipped.is_empty());
    }

    /// A packet is untrusted text. Anything in it that would be stored as plain text and looks like an
    /// extended private key would block encrypted backups for good (the rows it lands in can't be
    /// edited or deleted), so that wallet is skipped whole and nothing is written for it.
    #[test]
    fn import_skips_a_wallet_whose_text_looks_like_a_private_key() {
        const KEY_LIKE: &str = "xprvAAAAAAAAAAAAAAAAAAAAAAAA";
        type Tweak = Box<dyn Fn(&mut Packet)>;
        let hostile: Vec<(&str, usize, Tweak)> = vec![
            ("wallet name", 0, Box::new(|p: &mut Packet| p.wallets[0].name = KEY_LIKE.into())),
            ("snapshot date", 0, Box::new(|p: &mut Packet| p.wallets[0].snapshot.as_mut().unwrap().as_of = KEY_LIKE.into())),
            ("cosigner label", 1, Box::new(|p: &mut Packet| p.wallets[1].multisig.as_mut().unwrap().cosigners[0].label = KEY_LIKE.into())),
            ("cosigner hardware model", 1, Box::new(|p: &mut Packet| p.wallets[1].multisig.as_mut().unwrap().cosigners[1].hardware_model = Some(KEY_LIKE.into()))),
        ];
        for (what, wallet_index, tweak) in hostile {
            let mut p = fixture_packet();
            tweak(&mut p);
            let conn = mem_db();
            let summary = import_packet(&conn, &p).unwrap();
            assert_eq!(summary.skipped.len(), 1, "{what}: {:?}", summary.skipped);
            assert!(summary.skipped[0].1.contains("private key material is not stored"), "{what}: {:?}", summary.skipped);
            assert!(!summary.skipped[0].0.contains("xprv"), "{what}: the offending text is not echoed");
            assert_eq!(summary.created.len(), 2, "{what}: the other wallets still import");
            assert!(!summary.created.contains(&p.wallets[wallet_index].name), "{what}");

            // Nothing key-like was written anywhere the app can't clean up.
            let mut stored = Vec::new();
            for (table, column) in [("wallets", "name"), ("multisig_cosigners", "label"), ("multisig_cosigners", "hardware_model"), ("wallet_snapshots", "as_of")] {
                let mut stmt = conn.prepare(&format!("SELECT {column} FROM {table} WHERE {column} IS NOT NULL")).unwrap();
                stored.extend(stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|v| v.unwrap()));
            }
            assert!(stored.iter().all(|v| !crate::crypto::backup::contains_extended_private_key(v.as_bytes())), "{what}: {stored:?}");
        }
    }
}
