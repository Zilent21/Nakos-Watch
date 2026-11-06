use crate::db::models::{NewNodeConfig, NodeConfig};
use crate::rpc::{core_rpc, electrum, fees};
use crate::state::AppState;
use rusqlite::{params, Connection};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(crate) fn row_to_config(row: &rusqlite::Row) -> rusqlite::Result<NodeConfig> {
    Ok(NodeConfig {
        id: row.get("id")?,
        kind: row.get("kind")?,
        label: row.get("label")?,
        host: row.get("host")?,
        port: row.get("port")?,
        rpc_user: row.get("rpc_user")?,
        cookie_path: row.get("cookie_path")?,
        use_tls: row.get::<_, i64>("use_tls")? != 0,
        use_tor: row.get::<_, i64>("use_tor")? != 0,
        enabled: row.get::<_, i64>("enabled")? != 0,
        is_default: row.get::<_, i64>("is_default")? != 0,
        last_connected_at: row.get("last_connected_at")?,
        third_party: row.get::<_, String>("kind")? == "electrum" && !is_own_network_host(&row.get::<_, String>("host")?),
    })
}

/// True for a host on this computer or the person's own network: loopback, private and
/// link-local addresses, carrier-grade NAT (100.64.0.0/10, used by Tailscale), IPv6 unique-local
/// and link-local, single-label names and local suffixes. Anything else, `.onion` included (Tor
/// hides the IP address, not the wallet addresses), is someone else's server.
pub(crate) fn is_own_network_host(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
    if h.is_empty() {
        return false;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                let o = v4.octets();
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || (o[0] == 100 && (o[1] & 0xC0) == 64)
            }
            std::net::IpAddr::V6(v6) => {
                let s0 = v6.segments()[0];
                v6.is_loopback() || (s0 & 0xfe00) == 0xfc00 || (s0 & 0xffc0) == 0xfe80
            }
        };
    }
    // Any other dotted name, `.onion` included, falls through to "someone else's".
    !h.contains('.') || [".local", ".lan", ".home", ".home.arpa", ".internal", ".localdomain", ".localhost"].iter().any(|s| h.ends_with(s))
}

/// Error prefix the frontend recognises to show the privacy warning before a first sync.
pub(crate) const PUBLIC_SERVER_GATE: &str = "PUBLIC_SERVER_NOT_ACKNOWLEDGED";

fn public_ack_key(host: &str, port: u16) -> String {
    format!("public_electrum_ack:{}:{port}", host.trim().to_ascii_lowercase())
}

/// Sync talks to someone else's Electrum server only after the person accepted the privacy
/// warning for that exact host and port. A server on their own network needs no warning.
pub(crate) fn public_server_gate(conn: &Connection, cfg: &electrum::ElectrumConfig) -> Result<(), String> {
    if is_own_network_host(&cfg.host) || crate::db::meta_get(conn, &public_ack_key(&cfg.host, cfg.port)).map_err(|e| e.to_string())?.is_some() {
        return Ok(());
    }
    Err(format!("{PUBLIC_SERVER_GATE}|{}|{}", cfg.host, cfg.port))
}

/// The person read the privacy warning for this server and chose to sync through it.
#[tauri::command]
pub fn acknowledge_public_electrum(state: tauri::State<AppState>, host: String, port: u16) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    acknowledge_public_electrum_inner(&conn, &host, port)
}

fn acknowledge_public_electrum_inner(conn: &Connection, host: &str, port: u16) -> Result<(), String> {
    if host.trim().is_empty() || port == 0 {
        return Err("no server to acknowledge".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    crate::db::meta_set(conn, &public_ack_key(host, port), &now).map_err(|e| e.to_string())
}

/// Build a `CoreRpcConfig` for a stored node config, decrypting its saved
/// rpc_password (if any) — cookie_path takes precedence when both are set.
pub(crate) fn build_core_rpc_config(state: &AppState, conn: &Connection, cfg: &NodeConfig) -> Result<core_rpc::CoreRpcConfig, String> {
    let password = decrypt_password(state, conn, &cfg.id).unwrap_or(None);
    Ok(core_rpc::CoreRpcConfig {
        host: cfg.host.clone(),
        port: cfg.port as u16,
        user: cfg.rpc_user.clone(),
        password,
        cookie_path: cfg.cookie_path.clone(),
        use_tls: cfg.use_tls,
    })
}

/// The Bitcoin Core RPC transport is plain HTTP only (see `rpc::core_rpc`), so a Core
/// connection can never be saved, tested or used as "TLS". Electrum TLS is real (rustls) and is
/// not affected.
const CORE_TLS_UNSUPPORTED: &str = "TLS isn't supported for Bitcoin Core RPC connections. Turn TLS off for this connection.";

fn refuse_core_tls(kind: &str, use_tls: bool) -> Result<(), String> {
    if kind == "bitcoin_core_rpc" && use_tls {
        return Err(CORE_TLS_UNSUPPORTED.to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn list_node_configs(state: tauri::State<AppState>) -> Result<Vec<NodeConfig>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT * FROM node_configs ORDER BY created_at ASC").map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_config)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub fn add_node_config(state: tauri::State<AppState>, input: NewNodeConfig) -> Result<NodeConfig, String> {
    add_node_config_inner(&state, input)
}

fn add_node_config_inner(state: &AppState, input: NewNodeConfig) -> Result<NodeConfig, String> {
    refuse_core_tls(&input.kind, input.use_tls)?;
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();

    let (pw_ct, pw_nonce): (Option<Vec<u8>>, Option<Vec<u8>>) = if let Some(pw) = &input.rpc_password {
        let vault = state.vault()?;
        let guard = vault.lock();
        let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it before storing RPC credentials")?;
        let (nonce, ct) = key.encrypt(pw.as_bytes()).map_err(|e| e.to_string())?;
        (Some(ct), Some(nonce))
    } else {
        (None, None)
    };

    conn.execute(
        "INSERT INTO node_configs (id, kind, label, host, port, rpc_user, rpc_password_encrypted, rpc_password_nonce, cookie_path, use_tls, use_tor)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id, input.kind, input.label, input.host, input.port, input.rpc_user,
            pw_ct, pw_nonce, input.cookie_path, input.use_tls as i64, input.use_tor as i64
        ],
    )
    .map_err(|e| e.to_string())?;

    conn.query_row("SELECT * FROM node_configs WHERE id = ?1", params![id], row_to_config)
        .map_err(|e| e.to_string())
}

/// Update an existing node connection in place. Reuses `NewNodeConfig`'s
/// shape rather than adding a new type. `rpc_password: None` (or empty)
/// means "keep the currently stored password" — the frontend never has the
/// plaintext to send back anyway, since `NodeConfig`/`list_node_configs`
/// never return it. Switching auth mode (cookie vs rpcuser/password) clears
/// whichever fields the new mode doesn't use, so a stale credential from the
/// old mode can never silently take precedence again.
#[tauri::command]
pub fn update_node_config(state: tauri::State<AppState>, id: String, input: NewNodeConfig) -> Result<NodeConfig, String> {
    update_node_config_inner(&state, id, input)
}

fn update_node_config_inner(state: &AppState, id: String, input: NewNodeConfig) -> Result<NodeConfig, String> {
    refuse_core_tls(&input.kind, input.use_tls)?;
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    conn.query_row("SELECT 1 FROM node_configs WHERE id = ?1", params![id], |_| Ok(()))
        .map_err(|_| format!("node connection '{id}' not found"))?;

    match &input.rpc_user {
        Some(rpc_user) => {
            // userpass mode: cookie_path is no longer relevant.
            match input.rpc_password.as_deref().filter(|p| !p.is_empty()) {
                Some(pw) => {
                    let vault = state.vault()?;
                    let guard = vault.lock();
                    let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it before storing RPC credentials")?;
                    let (nonce, ct) = key.encrypt(pw.as_bytes()).map_err(|e| e.to_string())?;
                    conn.execute(
                        "UPDATE node_configs SET kind=?1, label=?2, host=?3, port=?4, rpc_user=?5, \
                         rpc_password_encrypted=?6, rpc_password_nonce=?7, cookie_path=NULL, use_tls=?8, use_tor=?9 \
                         WHERE id=?10",
                        params![input.kind, input.label, input.host, input.port, rpc_user, ct, nonce, input.use_tls as i64, input.use_tor as i64, id],
                    )
                }
                // Blank password field: leave the stored ciphertext untouched.
                None => conn.execute(
                    "UPDATE node_configs SET kind=?1, label=?2, host=?3, port=?4, rpc_user=?5, \
                     cookie_path=NULL, use_tls=?6, use_tor=?7 WHERE id=?8",
                    params![input.kind, input.label, input.host, input.port, rpc_user, input.use_tls as i64, input.use_tor as i64, id],
                ),
            }
        }
        // Cookie mode (or Electrum, where neither applies): drop any
        // previously stored rpcuser/password so they can't linger unused.
        None => conn.execute(
            "UPDATE node_configs SET kind=?1, label=?2, host=?3, port=?4, rpc_user=NULL, \
             rpc_password_encrypted=NULL, rpc_password_nonce=NULL, cookie_path=?5, use_tls=?6, use_tor=?7 \
             WHERE id=?8",
            params![input.kind, input.label, input.host, input.port, input.cookie_path, input.use_tls as i64, input.use_tor as i64, id],
        ),
    }
    .map_err(|e| e.to_string())?;

    // The saved settings changed, so the last successful test no longer vouches for them.
    // A connection must pass a fresh test before it can be made primary.
    conn.execute("UPDATE node_configs SET last_connected_at = NULL WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;

    conn.query_row("SELECT * FROM node_configs WHERE id = ?1", params![id], row_to_config)
        .map_err(|e| e.to_string())
}

/// Make a tested connection the primary one for its kind (Bitcoin Core or Electrum). The
/// primary is what status, fee estimates, the health score and wallet sync use; with no
/// explicit primary the oldest enabled connection of the kind is used. Only a connection
/// that has passed "Test connection" (which sets `last_connected_at`) can be promoted.
#[tauri::command]
pub fn set_primary_node(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    set_primary_node_on(&conn, &id)
}

fn set_primary_node_on(conn: &Connection, id: &str) -> Result<(), String> {
    let (kind, tested): (String, Option<String>) = conn
        .query_row("SELECT kind, last_connected_at FROM node_configs WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| format!("node connection '{id}' not found"))?;
    if tested.is_none() {
        return Err("test this connection successfully before making it primary".into());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE node_configs SET is_default = 0 WHERE kind = ?1", params![kind]).map_err(|e| e.to_string())?;
    tx.execute("UPDATE node_configs SET is_default = 1, enabled = 1 WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// Archive (or restore) a saved connection, like wallets. Nothing is deleted. An archived
/// connection is stored as `enabled = 0`, which every selector (status, fees, health, sync,
/// the inheritance packet) already ignores, and it can no longer be the primary. Restoring
/// sets `enabled = 1` again; it does not make the connection primary.
#[tauri::command]
pub fn archive_node_config(state: tauri::State<AppState>, id: String, archived: bool) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    set_node_archived_on(&conn, &id, archived)
}

fn set_node_archived_on(conn: &Connection, id: &str, archived: bool) -> Result<(), String> {
    let changed = if archived {
        conn.execute("UPDATE node_configs SET enabled = 0, is_default = 0 WHERE id = ?1", params![id])
    } else {
        conn.execute("UPDATE node_configs SET enabled = 1 WHERE id = ?1", params![id])
    }
    .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("node connection '{id}' not found"));
    }
    Ok(())
}

fn decrypt_password(state: &AppState, conn: &Connection, config_id: &str) -> Result<Option<String>, String> {
    let row: Option<(Vec<u8>, Vec<u8>)> = conn
        .query_row(
            "SELECT rpc_password_encrypted, rpc_password_nonce FROM node_configs WHERE id = ?1",
            params![config_id],
            |r| {
                let ct: Option<Vec<u8>> = r.get(0)?;
                let nonce: Option<Vec<u8>> = r.get(1)?;
                Ok(ct.zip(nonce))
            },
        )
        .map_err(|e| e.to_string())?;

    let Some((ct, nonce)) = row else { return Ok(None) };
    let vault = state.vault()?;
    let guard = vault.lock();
    let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it to use saved RPC credentials")?;
    let pt = key.decrypt(&nonce, &ct).map_err(|e| e.to_string())?;
    Ok(Some(String::from_utf8_lossy(&pt).to_string()))
}

#[tauri::command]
pub async fn test_node_connection(state: tauri::State<'_, AppState>, id: String) -> Result<bool, String> {
    test_node_connection_inner(&state, id)
}

fn test_node_connection_inner(state: &AppState, id: String) -> Result<bool, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let cfg = conn.query_row("SELECT * FROM node_configs WHERE id = ?1", params![id], row_to_config).map_err(|e| e.to_string())?;
    // A TLS request is refused before the password is decrypted, a cookie file is read or a
    // socket is opened; a refused test never records a successful connection.
    refuse_core_tls(&cfg.kind, cfg.use_tls)?;

    match cfg.kind.as_str() {
        "bitcoin_core_rpc" => {
            let rpc_cfg = build_core_rpc_config(state, &conn, &cfg)?;
            let client = rpc_cfg.connect().map_err(|e| e.to_string())?;
            core_rpc::get_node_status(&client).map_err(|e| e.to_string())?;
        }
        "electrum" => {
            electrum::ping(&cfg.host, cfg.port as u16, cfg.use_tls).map_err(|e| e.to_string())?;
        }
        other => return Err(format!("unknown node kind: {other}")),
    }

    conn.execute(
        "UPDATE node_configs SET last_connected_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;

    Ok(true)
}

pub fn default_electrum_config(conn: &Connection) -> Result<Option<electrum::ElectrumConfig>, String> {
    let row: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT host, port, use_tls FROM node_configs WHERE kind = 'electrum' AND enabled = 1 ORDER BY is_default DESC, created_at ASC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
        .map_err(|e| e.to_string())?;

    Ok(row.map(|(host, port, use_tls)| electrum::ElectrumConfig { host, port: port as u16, use_tls: use_tls != 0 }))
}

#[derive(Debug, serde::Serialize)]
pub struct NodeStatusResponse {
    pub core: Option<core_rpc::NodeStatus>,
    pub electrum: Option<electrum::ElectrumServerStatus>,
}

/// Status poll budget: a dead or black-holed host must not hold the caller on
/// the transport defaults (15 s for Core, 10 s for Electrum, and then the OS).
const STATUS_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const STATUS_TOTAL_BUDGET: Duration = Duration::from_secs(5);

/// Per-node outcome of one bounded status poll. `None` = that node is not configured.
pub(crate) struct NodeStatusProbe {
    pub core: Option<Result<core_rpc::NodeStatus, String>>,
    pub electrum: Option<Result<electrum::ElectrumServerStatus, String>>,
}

/// Run `f` on its own thread and wait for it only until `deadline`. The worker
/// is itself bounded by the client timeouts, so a late thread ends on its own.
fn spawn_bounded<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> mpsc::Receiver<Result<T, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx
}

fn recv_by<T>(rx: mpsc::Receiver<Result<T, String>>, deadline: Instant) -> Result<T, String> {
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "node did not answer in time".to_string())?
}

/// Poll Core and Electrum in parallel under one shared deadline.
pub(crate) fn probe_node_status(core: Option<core_rpc::CoreRpcConfig>, electrum_cfg: Option<electrum::ElectrumConfig>) -> NodeStatusProbe {
    let deadline = Instant::now() + STATUS_TOTAL_BUDGET;
    let core_rx = core.map(|cfg| {
        spawn_bounded(move || {
            let client = cfg.connect_with_timeout(STATUS_CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
            core_rpc::get_node_status(&client).map_err(|e| e.to_string())
        })
    });
    let electrum_rx = electrum_cfg.map(|cfg| {
        spawn_bounded(move || {
            let client = cfg.connect_once_with_timeout(STATUS_CONNECT_TIMEOUT.as_secs() as u8).map_err(|e| e.to_string())?;
            electrum::get_server_status(&client).map_err(|e| e.to_string())
        })
    });
    NodeStatusProbe {
        core: core_rx.map(|rx| recv_by(rx, deadline)),
        electrum: electrum_rx.map(|rx| recv_by(rx, deadline)),
    }
}

#[tauri::command]
pub async fn get_node_status(state: tauri::State<'_, AppState>) -> Result<NodeStatusResponse, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    let core_cfg = match conn.query_row(
        "SELECT * FROM node_configs WHERE kind = 'bitcoin_core_rpc' AND enabled = 1 ORDER BY is_default DESC, created_at ASC LIMIT 1",
        [],
        row_to_config,
    ) {
        Ok(cfg) => Some(build_core_rpc_config(&state, &conn, &cfg)?),
        Err(_) => None,
    };
    let electrum_cfg = default_electrum_config(&conn)?;
    // Release the pooled connection before any network wait.
    drop(conn);

    // Unreachable nodes still read as "no status" (None), as before; only the wait is bounded now.
    let probe = probe_node_status(core_cfg, electrum_cfg);
    Ok(NodeStatusResponse {
        core: probe.core.and_then(|r| r.ok()),
        electrum: probe.electrum.and_then(|r| r.ok()),
    })
}

#[derive(Debug, serde::Serialize)]
pub struct FeeEstimatesResponse {
    pub source: String,
    pub core_estimates: Vec<core_rpc::FeeEstimate>,
    pub public_estimate: Option<fees::PublicFeeEstimate>,
    /// "disabled" (default: no explorer request was made), "ok", "error: …",
    /// or "not_needed" when Core/Electrum answered.
    pub explorer_status: String,
}

#[tauri::command]
pub async fn get_fee_estimates(state: tauri::State<'_, AppState>) -> Result<FeeEstimatesResponse, String> {
    get_fee_estimates_inner(&state)
}

fn get_fee_estimates_inner(state: &AppState) -> Result<FeeEstimatesResponse, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    if let Ok(cfg) = conn.query_row(
        "SELECT * FROM node_configs WHERE kind = 'bitcoin_core_rpc' AND enabled = 1 ORDER BY is_default DESC, created_at ASC LIMIT 1",
        [],
        row_to_config,
    ) {
        let rpc_cfg = build_core_rpc_config(state, &conn, &cfg)?;
        if let Ok(client) = rpc_cfg.connect_with_timeout(STATUS_CONNECT_TIMEOUT) {
            if let Ok(estimates) = core_rpc::estimate_fees(&client) {
                return Ok(FeeEstimatesResponse { source: "bitcoin_core".into(), core_estimates: estimates, public_estimate: None, explorer_status: "not_needed".into() });
            }
        }
    }

    if let Some(electrum_cfg) = default_electrum_config(&conn)? {
        if let Ok(client) = electrum_cfg.connect_once_with_timeout(STATUS_CONNECT_TIMEOUT.as_secs() as u8) {
            let mut ests = Vec::new();
            for blocks in [1usize, 3, 6, 12, 24, 144] {
                match electrum::estimate_fee_sat_per_vb(&client, blocks) {
                    Ok(Some(rate)) => ests.push(core_rpc::FeeEstimate { target_blocks: blocks as u16, sat_per_vb: Some(rate) }),
                    Ok(None) => {}
                    // A failed call means the server is not answering; do not wait out the
                    // timeout again for each remaining target.
                    Err(_) => break,
                }
            }
            if !ests.is_empty() {
                return Ok(FeeEstimatesResponse { source: "electrum".into(), core_estimates: ests, public_estimate: None, explorer_status: "not_needed".into() });
            }
        }
    }

    // Last resort, and only if the user switched the public explorer on in
    // Settings. Off by default: no request is made and the status says so.
    let (public, source, status) = crate::commands::explorer_cmd::explorer_fee_fallback(state, &conn);
    Ok(FeeEstimatesResponse { source, core_estimates: vec![], public_estimate: public, explorer_status: status })
}

#[cfg(test)]
mod node_tests {
    use super::*;
    use crate::crypto;
    use crate::state::VaultState;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn userpass_input(rpc_password: Option<&str>) -> NewNodeConfig {
        NewNodeConfig {
            kind: "bitcoin_core_rpc".into(),
            label: "core".into(),
            host: "node.example".into(),
            port: 8332,
            rpc_user: Some("user".into()),
            rpc_password: rpc_password.map(String::from),
            cookie_path: None,
            use_tls: false,
            use_tor: false,
        }
    }

    fn stored_ciphertext(conn: &Connection, id: &str) -> (Vec<u8>, Vec<u8>) {
        conn.query_row("SELECT rpc_password_encrypted, rpc_password_nonce FROM node_configs WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
    }

    #[test]
    fn own_network_hosts_need_no_warning_and_others_do() {
        for own in [
            "localhost", "127.0.0.1", "10.0.0.5", "172.16.0.1", "172.31.255.254", "192.168.1.10", "169.254.3.4", "100.64.0.1", "100.127.255.1",
            "::1", "[::1]", "fd12:3456::1", "fe80::1", "umbrel.local", "node.lan", "mynode", "router.home.arpa", "nas.internal", "box.home", "Node.Local.", "printer.localdomain", "app.localhost", "  umbrel.local ",
        ] {
            assert!(is_own_network_host(own), "{own} is on the person's own network");
        }
        for other in [
            "electrum.blockstream.info", "[2001:4860::8888]", "8.8.8.8", "172.32.0.1", "11.0.0.1", "100.128.0.1", "100.63.255.255", "2001:4860::8888",
            "abcdefghijklmnop.onion", "example.com", "",
        ] {
            assert!(!is_own_network_host(other), "{other} is someone else's server");
        }
    }

    #[test]
    fn the_privacy_warning_is_accepted_per_host_and_port() {
        let dir = crate::test_dir::TestDir::new("gate");
        let pool = crate::db::open_pool(&dir).unwrap();
        let conn = pool.get().unwrap();
        let cfg = |host: &str, port: u16| electrum::ElectrumConfig { host: host.into(), port, use_tls: true };

        let err = public_server_gate(&conn, &cfg("electrum.blockstream.info", 50002)).unwrap_err();
        assert_eq!(err, format!("{PUBLIC_SERVER_GATE}|electrum.blockstream.info|50002"));
        assert!(public_server_gate(&conn, &cfg("192.168.1.5", 50002)).is_ok(), "own network: no warning");

        assert!(acknowledge_public_electrum_inner(&conn, "  ", 50002).is_err());
        assert!(acknowledge_public_electrum_inner(&conn, "electrum.blockstream.info", 0).is_err());
        acknowledge_public_electrum_inner(&conn, "electrum.blockstream.info", 50002).unwrap();
        assert!(public_server_gate(&conn, &cfg("electrum.blockstream.info", 50002)).is_ok());
        assert!(public_server_gate(&conn, &cfg(" Electrum.Blockstream.Info ", 50002)).is_ok(), "same server, other spelling");
        assert!(public_server_gate(&conn, &cfg("electrum.blockstream.info", 50001)).is_err(), "another port is another server");
        assert!(public_server_gate(&conn, &cfg("electrum.example.org", 50002)).is_err(), "another host needs its own warning");

        // The Node page learns which saved connections are someone else's.
        conn.execute_batch(
            "INSERT INTO node_configs (id, kind, label, host, port) VALUES ('pub', 'electrum', 'public', 'electrum.blockstream.info', 50002);
             INSERT INTO node_configs (id, kind, label, host, port) VALUES ('own', 'electrum', 'mine', 'umbrel.local', 50001);
             INSERT INTO node_configs (id, kind, label, host, port) VALUES ('core', 'bitcoin_core_rpc', 'core', 'node.example.com', 8332);",
        )
        .unwrap();
        let flag = |id: &str| conn.query_row("SELECT * FROM node_configs WHERE id = ?1", params![id], row_to_config).unwrap().third_party;
        assert!(flag("pub"));
        assert!(!flag("own"));
        assert!(!flag("core"), "only Electrum is used for sync");
    }

    #[test]
    fn blank_rpc_password_keeps_ciphertext_byte_identical_and_new_one_replaces_it() {
        let dir = crate::test_dir::TestDir::new("node");
        let pool = crate::db::open_pool(&dir).unwrap();
        let (_, key) = crypto::init_passphrase("correct horse battery staple").unwrap();
        let state = AppState::default();
        {
            let mut inner = state.inner.lock();
            inner.pool = Some(pool.clone());
            inner.vault = Some(Arc::new(Mutex::new(VaultState { master_key: Some(key) })));
        }
        let conn = pool.get().unwrap();
        conn.execute("INSERT INTO node_configs (id, kind, label, host, port) VALUES ('n1', 'bitcoin_core_rpc', 'core', 'node.example', 8332)", [])
            .unwrap();

        // Store a non-blank password through the real update path.
        update_node_config_inner(&state, "n1".into(), userpass_input(Some("original-rpc-pw"))).unwrap();
        let (ct, nonce) = stored_ciphertext(&conn, "n1");
        assert_eq!(decrypt_password(&state, &conn, "n1").unwrap().as_deref(), Some("original-rpc-pw"));

        // Blank (the empty string the UI sends): no wipe, no re-encrypt. A rewrite
        // would draw a fresh random nonce, so identical bytes prove the write was skipped.
        update_node_config_inner(&state, "n1".into(), userpass_input(Some(""))).unwrap();
        let (ct_after, nonce_after) = stored_ciphertext(&conn, "n1");
        assert_eq!(ct_after, ct, "ciphertext changed on blank password");
        assert_eq!(nonce_after, nonce, "nonce changed on blank password");
        assert_eq!(decrypt_password(&state, &conn, "n1").unwrap().as_deref(), Some("original-rpc-pw"));

        // A non-blank new password replaces the ciphertext and decrypts to the new value.
        update_node_config_inner(&state, "n1".into(), userpass_input(Some("replacement-rpc-pw"))).unwrap();
        let (ct_new, nonce_new) = stored_ciphertext(&conn, "n1");
        assert_ne!(ct_new, ct, "ciphertext not replaced");
        assert_ne!(nonce_new, nonce, "nonce not replaced");
        assert_eq!(decrypt_password(&state, &conn, "n1").unwrap().as_deref(), Some("replacement-rpc-pw"));

        drop(conn);
        drop(pool);
        state.inner.lock().pool = None;
    }

    fn assert_fails_within_budget(core: core_rpc::CoreRpcConfig) {
        let start = Instant::now();
        let probe = probe_node_status(Some(core), None);
        let elapsed = start.elapsed();
        assert!(matches!(probe.core, Some(Err(_))), "unreachable node must return Err");
        assert!(elapsed < STATUS_TOTAL_BUDGET + Duration::from_millis(1500), "status call took {elapsed:?}");
    }

    fn core_cfg(port: u16) -> core_rpc::CoreRpcConfig {
        core_rpc::CoreRpcConfig { host: "127.0.0.1".into(), port, user: Some("u".into()), password: Some("p".into()), cookie_path: None, use_tls: false }
    }

    #[test]
    fn status_call_returns_err_within_budget_on_a_refused_port() {
        // Bind then drop to get a loopback port with nothing listening.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert_fails_within_budget(core_cfg(port));
    }

    #[test]
    fn status_call_returns_err_within_budget_when_the_host_accepts_but_never_answers() {
        // Loopback listener that never reads or replies: the real read timeout has to fire.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_fails_within_budget(core_cfg(port));
        drop(listener);
    }

    /// A TLS Electrum server that accepts the connection and never answers (as a plain-TCP
    /// port does when a TLS handshake is attempted) must cost one short timeout, not one per
    /// fee target, and must not hold the caller.
    #[test]
    fn fee_estimates_do_not_wait_out_the_timeout_once_per_target_on_a_silent_electrum_server() {
        let dir = crate::test_dir::TestDir::new("fees");
        let pool = crate::db::open_pool(&dir).unwrap();
        let state = AppState::default();
        state.inner.lock().pool = Some(pool.clone());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Hold accepted sockets open without replying.
        let held = std::thread::spawn(move || {
            let mut keep = Vec::new();
            while let Ok((s, _)) = listener.accept() {
                keep.push(s);
                if keep.len() >= 8 {
                    break;
                }
            }
            std::thread::sleep(Duration::from_secs(12));
        });
        pool.get()
            .unwrap()
            .execute("INSERT INTO node_configs (id, kind, label, host, port, use_tls, enabled) VALUES ('e1','electrum','e','127.0.0.1',?1,1,1)", params![port])
            .unwrap();

        let start = Instant::now();
        let res = get_fee_estimates_inner(&state).unwrap();
        let elapsed = start.elapsed();
        assert!(res.core_estimates.is_empty(), "a silent server yields no estimates");
        assert!(elapsed < Duration::from_secs(8), "fee estimates took {elapsed:?}; six per-target timeouts would be ~18s");

        state.inner.lock().pool = None;
        drop(pool);
        drop(held);
    }

    fn mem_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::migrations().to_latest(&mut conn).unwrap();
        conn
    }

    fn add_node(conn: &Connection, id: &str, kind: &str, port: i64, tested: bool) {
        conn.execute(
            "INSERT INTO node_configs (id, kind, label, host, port, last_connected_at) VALUES (?1, ?2, ?1, 'node.example', ?3, ?4)",
            params![id, kind, port, tested.then_some("2026-10-03T00:00:00Z")],
        )
        .unwrap();
        // Distinct, ordered creation times so "oldest first" is deterministic.
        conn.execute("UPDATE node_configs SET created_at = ?2 WHERE id = ?1", params![id, format!("2026-10-03T00:00:0{}Z", conn.query_row("SELECT COUNT(*) FROM node_configs", [], |r| r.get::<_, i64>(0)).unwrap())]).unwrap();
    }

    fn active_electrum_port(conn: &Connection) -> u16 {
        default_electrum_config(conn).unwrap().unwrap().port
    }

    #[test]
    fn without_a_primary_the_oldest_enabled_connection_of_a_kind_is_used() {
        // The behaviour that made a tested connection lose to an older, dead one.
        let conn = mem_db();
        add_node(&conn, "old-dead", "electrum", 50001, false);
        add_node(&conn, "new-good", "electrum", 50002, true);
        assert_eq!(active_electrum_port(&conn), 50001);
    }

    #[test]
    fn set_primary_node_switches_the_active_connection_within_its_kind_only() {
        let conn = mem_db();
        add_node(&conn, "old-dead", "electrum", 50001, false);
        add_node(&conn, "new-good", "electrum", 50002, true);
        add_node(&conn, "core", "bitcoin_core_rpc", 8332, true);
        set_primary_node_on(&conn, "core").unwrap();
        assert_eq!(active_electrum_port(&conn), 50001, "promoting a Core node leaves Electrum alone");

        set_primary_node_on(&conn, "new-good").unwrap();
        assert_eq!(active_electrum_port(&conn), 50002);
        let primaries: i64 = conn.query_row("SELECT COUNT(*) FROM node_configs WHERE kind = 'electrum' AND is_default = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(primaries, 1, "exactly one primary per kind");
        let core_primary: i64 = conn.query_row("SELECT is_default FROM node_configs WHERE id = 'core'", [], |r| r.get(0)).unwrap();
        assert_eq!(core_primary, 1, "the Core primary is untouched");

        // The older row never passed a test, so it cannot take the role back.
        set_primary_node_on(&conn, "old-dead").unwrap_err();
        assert_eq!(active_electrum_port(&conn), 50002);
    }

    #[test]
    fn an_untested_connection_cannot_be_made_primary() {
        let conn = mem_db();
        add_node(&conn, "a", "electrum", 50001, true);
        add_node(&conn, "b", "electrum", 50002, false);
        set_primary_node_on(&conn, "a").unwrap();
        let err = set_primary_node_on(&conn, "b").unwrap_err();
        assert!(err.contains("test this connection"), "{err}");
        assert_eq!(active_electrum_port(&conn), 50001, "the existing primary stays");
        assert!(set_primary_node_on(&conn, "missing").unwrap_err().contains("not found"));
    }

    #[test]
    fn a_disabled_tested_connection_is_enabled_when_made_primary() {
        let conn = mem_db();
        add_node(&conn, "a", "electrum", 50001, true);
        conn.execute("UPDATE node_configs SET enabled = 0 WHERE id = 'a'", []).unwrap();
        assert!(default_electrum_config(&conn).unwrap().is_none());
        set_primary_node_on(&conn, "a").unwrap();
        assert_eq!(active_electrum_port(&conn), 50001);
    }

    #[test]
    fn editing_a_connection_clears_its_test_result_so_it_must_be_retested() {
        let dir = crate::test_dir::TestDir::new("primary");
        let pool = crate::db::open_pool(&dir).unwrap();
        let state = AppState::default();
        state.inner.lock().pool = Some(pool.clone());
        let conn = pool.get().unwrap();
        add_node(&conn, "n1", "electrum", 50001, true);
        let input = NewNodeConfig {
            kind: "electrum".into(),
            label: "n1".into(),
            host: "node.example".into(),
            port: 50002,
            rpc_user: None,
            rpc_password: None,
            cookie_path: None,
            use_tls: true,
            use_tor: false,
        };
        update_node_config_inner(&state, "n1".into(), input).unwrap();
        let tested: Option<String> = conn.query_row("SELECT last_connected_at FROM node_configs WHERE id = 'n1'", [], |r| r.get(0)).unwrap();
        assert!(tested.is_none(), "an edit invalidates the earlier test");
        assert!(set_primary_node_on(&conn, "n1").is_err());
        drop(conn);
        state.inner.lock().pool = None;
        drop(pool);
    }

    #[test]
    fn archiving_removes_a_connection_from_use_and_clears_its_primary_flag() {
        let conn = mem_db();
        add_node(&conn, "a", "electrum", 50001, true);
        add_node(&conn, "b", "electrum", 50002, true);
        set_primary_node_on(&conn, "b").unwrap();
        assert_eq!(active_electrum_port(&conn), 50002);

        set_node_archived_on(&conn, "b", true).unwrap();
        assert_eq!(active_electrum_port(&conn), 50001, "the archived primary is no longer used");
        let (enabled, primary): (i64, i64) = conn.query_row("SELECT enabled, is_default FROM node_configs WHERE id = 'b'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((enabled, primary), (0, 0));
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM node_configs", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 2, "archiving never deletes");
    }

    #[test]
    fn restoring_an_archived_connection_does_not_take_over_an_explicit_primary() {
        let conn = mem_db();
        add_node(&conn, "old", "electrum", 50001, true);
        add_node(&conn, "new", "electrum", 50002, true);
        set_node_archived_on(&conn, "old", true).unwrap();
        set_primary_node_on(&conn, "new").unwrap();

        set_node_archived_on(&conn, "old", false).unwrap();
        assert_eq!(active_electrum_port(&conn), 50002, "the oldest row is back in the list but the chosen primary still wins");
        assert!(set_node_archived_on(&conn, "missing", true).unwrap_err().contains("not found"));
    }

    /// Bitcoin Core RPC is plain HTTP only. A TLS request is refused before any socket is
    /// opened, and a Core row is never stored (or tested "successfully") with `use_tls` set.
    #[test]
    fn core_tls_is_refused_before_any_socket_and_never_stored() {
        let dir = crate::test_dir::TestDir::new("coretls");
        let pool = crate::db::open_pool(&dir).unwrap();
        let state = AppState::default();
        state.inner.lock().pool = Some(pool.clone());
        let conn = pool.get().unwrap();

        // A loopback listener stands in for the node: any connection attempt shows up here.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port() as i64;
        let input = |kind: &str, use_tls: bool| NewNodeConfig {
            kind: kind.into(),
            label: "node".into(),
            host: "127.0.0.1".into(),
            port,
            rpc_user: None,
            rpc_password: None,
            cookie_path: None,
            use_tls,
            use_tor: false,
        };
        let rows = |c: &Connection| -> i64 { c.query_row("SELECT COUNT(*) FROM node_configs", [], |r| r.get(0)).unwrap() };

        // Add with TLS: refused, nothing written.
        assert_eq!(add_node_config_inner(&state, input("bitcoin_core_rpc", true)).unwrap_err(), CORE_TLS_UNSUPPORTED);
        assert_eq!(rows(&conn), 0, "a refused add stores nothing");

        // Add without TLS is fine and stores no TLS flag; Electrum TLS is real and still allowed.
        let core = add_node_config_inner(&state, input("bitcoin_core_rpc", false)).unwrap();
        assert!(!core.use_tls);
        let electrum = add_node_config_inner(&state, input("electrum", true)).unwrap();
        assert!(electrum.use_tls, "Electrum TLS is unaffected");

        // Edit a Core row to TLS: refused, the stored flag is unchanged.
        assert_eq!(update_node_config_inner(&state, core.id.clone(), input("bitcoin_core_rpc", true)).unwrap_err(), CORE_TLS_UNSUPPORTED);
        let stored: i64 = conn.query_row("SELECT use_tls FROM node_configs WHERE id = ?1", params![core.id], |r| r.get(0)).unwrap();
        assert_eq!(stored, 0);

        // A TLS-flagged Core row that predates the rule (written straight to the table):
        // Test connection refuses it and never records success.
        conn.execute("INSERT INTO node_configs (id, kind, label, host, port, use_tls) VALUES ('legacy','bitcoin_core_rpc','old','127.0.0.1',?1,1)", params![port]).unwrap();
        assert_eq!(test_node_connection_inner(&state, "legacy".into()).unwrap_err(), CORE_TLS_UNSUPPORTED);
        let tested: Option<String> = conn.query_row("SELECT last_connected_at FROM node_configs WHERE id = 'legacy'", [], |r| r.get(0)).unwrap();
        assert!(tested.is_none(), "a refused test must not record a connection");

        // None of the above opened a socket to the node.
        match listener.accept() {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("a socket was opened to the node: {other:?}"),
        }

        drop(conn);
        state.inner.lock().pool = None;
        drop(pool);
    }
}
