//! Settings and commands for the optional public explorer. Everything here is
//! gated by the `explorer_enabled` flag (default off) inside
//! `rpc::explorer::Explorer`, so with the toggle off no command in this file
//! makes an HTTP request.
//!
//! Storage (db_meta): `explorer_enabled` ("1"/"0"), `explorer_preset`,
//! `explorer_base_url`, and the optional API token encrypted with the vault
//! key (`explorer_token_ct` / `explorer_token_nonce`, base64), like an RPC
//! password. The token is never returned to the UI, logged, put in an error,
//! or written into any export. It is tied to the server it was saved for
//! (`explorer_token_origin`) and only sent there, and only over https (or to
//! this computer); see `rpc::explorer::check_token_destination`.

use crate::db;
use crate::rpc::explorer::{self, AddressLookup, Explorer, ExplorerConfig, Preset, ReqwestHttp};
use crate::rpc::fees::PublicFeeEstimate;
use crate::state::AppState;
use bitcoin::base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

const K_ENABLED: &str = "explorer_enabled";
const K_PRESET: &str = "explorer_preset";
const K_URL: &str = "explorer_base_url";
const K_TOKEN_CT: &str = "explorer_token_ct";
const K_TOKEN_NONCE: &str = "explorer_token_nonce";
const K_TOKEN_ORIGIN: &str = "explorer_token_origin";

pub const PRIVACY_NOTE: &str = "This server sees the addresses or fee requests you send.";

fn meta(conn: &Connection, key: &str) -> Option<String> {
    db::meta_get(conn, key).ok().flatten()
}

fn has_token(conn: &Connection) -> bool {
    meta(conn, K_TOKEN_CT).is_some() && meta(conn, K_TOKEN_NONCE).is_some()
}

/// Load the saved config. The token is decrypted only when `with_token` and
/// the explorer is enabled; a locked vault then yields a visible error rather
/// than silently dropping the token.
fn load_config(state: &AppState, conn: &Connection, with_token: bool) -> Result<ExplorerConfig, String> {
    let enabled = meta(conn, K_ENABLED).as_deref() == Some("1");
    let preset = meta(conn, K_PRESET).and_then(|p| Preset::parse(&p)).unwrap_or(Preset::Mempool);
    let base_url = meta(conn, K_URL).unwrap_or_else(|| preset.default_base_url().to_string());
    let mut token = None;
    if enabled && with_token && has_token(conn) {
        let ct = STANDARD.decode(meta(conn, K_TOKEN_CT).unwrap_or_default()).map_err(|e| e.to_string())?;
        let nonce = STANDARD.decode(meta(conn, K_TOKEN_NONCE).unwrap_or_default()).map_err(|e| e.to_string())?;
        let vault = state.vault()?;
        let guard = vault.lock();
        let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it to use the explorer API token")?;
        let pt = key.decrypt(&nonce, &ct).map_err(|e| e.to_string())?;
        token = Some(String::from_utf8_lossy(&pt).to_string());
    }
    Ok(ExplorerConfig { enabled, preset, base_url, token, token_origin: meta(conn, K_TOKEN_ORIGIN) })
}

#[derive(Debug, Serialize)]
pub struct ExplorerSettings {
    pub enabled: bool,
    pub preset: String,
    pub base_url: String,
    /// Whether a token is stored. The token itself is never sent to the UI.
    pub has_token: bool,
    /// The server (`https://host:port`) the stored token belongs to; empty for a token saved
    /// before this was recorded, which is not sent until it is entered again.
    pub token_origin: Option<String>,
    pub privacy_note: String,
}

#[tauri::command]
pub fn get_explorer_settings(state: tauri::State<AppState>) -> Result<ExplorerSettings, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let cfg = load_config(&state, &conn, false)?;
    Ok(ExplorerSettings {
        enabled: cfg.enabled,
        preset: cfg.preset.as_str().to_string(),
        base_url: cfg.base_url,
        has_token: has_token(&conn),
        token_origin: meta(&conn, K_TOKEN_ORIGIN),
        privacy_note: PRIVACY_NOTE.to_string(),
    })
}

#[derive(Debug, Deserialize)]
pub struct SaveExplorerSettings {
    pub enabled: bool,
    pub preset: String,
    pub base_url: String,
    /// New token to store. `None`/empty keeps whatever is stored.
    pub token: Option<String>,
    pub clear_token: bool,
}

/// What a save does to the stored token.
#[derive(Debug, PartialEq)]
enum TokenPlan {
    Keep,
    Remove,
    Replace { token: String, origin: String },
}

/// Decide the token's fate before anything is written. A token belongs to one server: a new one
/// is bound to the URL being saved, and an existing one is only kept if the URL it will be sent
/// to is still that server. `normalized` is the validated URL (`None` only when the explorer is
/// off and the box is empty).
fn plan_token(
    enabled: bool,
    normalized: Option<&str>,
    stored_origin: Option<&str>,
    has_stored: bool,
    new_token: Option<&str>,
    clear: bool,
) -> Result<TokenPlan, String> {
    if let Some(token) = new_token {
        let url = normalized.ok_or("enter the server address before saving an API token")?;
        explorer::check_token_transport(url).map_err(|e| e.to_string())?;
        let origin = explorer::origin_of(url).ok_or("not a valid URL")?;
        return Ok(TokenPlan::Replace { token: token.to_string(), origin });
    }
    if clear {
        return Ok(TokenPlan::Remove);
    }
    if has_stored && enabled {
        let url = normalized.ok_or("enter the server address")?;
        explorer::check_token_destination(url, stored_origin).map_err(|e| e.to_string())?;
    }
    Ok(TokenPlan::Keep)
}

/// Saving makes no request. Turning the explorer on requires a valid URL.
#[tauri::command]
pub fn save_explorer_settings(state: tauri::State<AppState>, input: SaveExplorerSettings) -> Result<(), String> {
    save_explorer_settings_inner(&state, input)
}

fn save_explorer_settings_inner(state: &AppState, input: SaveExplorerSettings) -> Result<(), String> {
    let preset = Preset::parse(&input.preset).ok_or("unknown explorer preset")?;
    let base_url = input.base_url.trim().to_string();
    let normalized = if input.enabled || !base_url.is_empty() {
        Some(explorer::validate_base_url(&base_url).map_err(|e| e.to_string())?)
    } else {
        None
    };

    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    let new_token = input.token.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let plan = plan_token(
        input.enabled,
        normalized.as_deref(),
        meta(&conn, K_TOKEN_ORIGIN).as_deref(),
        has_token(&conn),
        new_token,
        input.clear_token,
    )?;
    // Encrypt first, so a locked vault or a refusal above leaves everything as it was.
    let sealed = match &plan {
        TokenPlan::Replace { token, origin } => {
            let vault = state.vault()?;
            let guard = vault.lock();
            let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it before saving an API token")?;
            let (nonce, ct) = key.encrypt(token.as_bytes()).map_err(|e| e.to_string())?;
            Some((STANDARD.encode(ct), STANDARD.encode(nonce), origin.clone()))
        }
        _ => None,
    };

    match (&plan, sealed) {
        (TokenPlan::Remove, _) => {
            conn.execute(
                "DELETE FROM db_meta WHERE key IN (?1, ?2, ?3)",
                rusqlite::params![K_TOKEN_CT, K_TOKEN_NONCE, K_TOKEN_ORIGIN],
            )
            .map_err(|e| e.to_string())?;
        }
        (_, Some((ct, nonce, origin))) => {
            db::meta_set(&conn, K_TOKEN_CT, &ct).map_err(|e| e.to_string())?;
            db::meta_set(&conn, K_TOKEN_NONCE, &nonce).map_err(|e| e.to_string())?;
            db::meta_set(&conn, K_TOKEN_ORIGIN, &origin).map_err(|e| e.to_string())?;
        }
        _ => {}
    }

    db::meta_set(&conn, K_PRESET, preset.as_str()).map_err(|e| e.to_string())?;
    db::meta_set(&conn, K_URL, normalized.as_deref().unwrap_or("")).map_err(|e| e.to_string())?;
    db::meta_set(&conn, K_ENABLED, if input.enabled { "1" } else { "0" }).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ExplorerHeight {
    pub enabled: bool,
    pub height: Option<u64>,
    pub error: Option<String>,
}

/// Explorer chain tip, as a cross-check against Core's height. Disabled makes
/// no request; failures are returned as data so the UI can show them. Async so a slow or
/// unreachable explorer (up to the 8 s timeout) never holds the window: the settings are read
/// here (database and vault only) and the request itself runs off the async workers.
#[tauri::command]
pub async fn get_explorer_height(state: tauri::State<'_, AppState>) -> Result<ExplorerHeight, String> {
    let cfg = {
        let pool = state.pool()?;
        let conn = pool.get().map_err(|e| e.to_string())?;
        let cfg = load_config(&state, &conn, false)?;
        if !cfg.enabled {
            return Ok(ExplorerHeight { enabled: false, height: None, error: None });
        }
        match load_config(&state, &conn, true) {
            Ok(c) => c,
            Err(e) => return Ok(ExplorerHeight { enabled: true, height: None, error: Some(e) }),
        }
    };
    Ok(explorer_height_for(cfg).await)
}

/// The network half of `get_explorer_height`, on a blocking-pool thread.
async fn explorer_height_for(cfg: ExplorerConfig) -> ExplorerHeight {
    let result = tokio::task::spawn_blocking(move || Explorer::new(&cfg, &ReqwestHttp).tip_height().map_err(|e| e.to_string())).await;
    match result.map_err(|e| format!("the explorer request failed: {e}")).and_then(|r| r) {
        Ok(h) => ExplorerHeight { enabled: true, height: Some(h), error: None },
        Err(e) => ExplorerHeight { enabled: true, height: None, error: Some(e) },
    }
}

/// One address, only when the user clicks "Look up". Async for the same reason.
#[tauri::command]
pub async fn explorer_lookup_address(state: tauri::State<'_, AppState>, address: String) -> Result<AddressLookup, String> {
    let cfg = {
        let pool = state.pool()?;
        let conn = pool.get().map_err(|e| e.to_string())?;
        load_config(&state, &conn, true)?
    };
    explorer_lookup_for(cfg, address).await
}

async fn explorer_lookup_for(cfg: ExplorerConfig, address: String) -> Result<AddressLookup, String> {
    tokio::task::spawn_blocking(move || Explorer::new(&cfg, &ReqwestHttp).lookup_address(&address).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("the explorer request failed: {e}"))?
}

/// Fee fallback for `get_fee_estimates`, used only when Core and Electrum
/// have nothing. Returns (estimate, source label, status). With the explorer
/// off this makes no request and the status says so.
pub(crate) fn explorer_fee_fallback(state: &AppState, conn: &Connection) -> (Option<PublicFeeEstimate>, String, String) {
    let cfg = match load_config(state, conn, false) {
        Ok(c) => c,
        Err(e) => return (None, "none".into(), format!("error: {e}")),
    };
    if !cfg.enabled {
        return (None, "none".into(), "disabled".into());
    }
    let cfg = match load_config(state, conn, true) {
        Ok(c) => c,
        Err(e) => return (None, "none".into(), format!("error: {e}")),
    };
    match Explorer::new(&cfg, &ReqwestHttp).fee_estimates() {
        Ok(f) => (Some(f), format!("explorer ({})", cfg.preset.as_str()), "ok".into()),
        Err(e) => (None, "none".into(), format!("error: {e}")),
    }
}

#[cfg(test)]
mod token_binding_tests {
    use super::*;
    use crate::crypto;
    use crate::rpc::explorer::{ExplorerError, Http};
    use crate::state::VaultState;
    use parking_lot::Mutex;
    use std::cell::RefCell;
    use std::sync::Arc;

    const FEES: &str = r#"{"fastestFee":12,"halfHourFee":9,"hourFee":6,"economyFee":3,"minimumFee":1}"#;

    struct Recorder(RefCell<Vec<(String, Option<String>)>>);
    impl Http for Recorder {
        fn get(&self, url: &str, bearer: Option<&str>) -> Result<String, String> {
            self.0.borrow_mut().push((url.to_string(), bearer.map(String::from)));
            Ok(FEES.into())
        }
    }

    fn setup() -> (crate::test_dir::TestDir, AppState, r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>) {
        let dir = crate::test_dir::TestDir::new("explorer");
        let pool = crate::db::open_pool(&dir).unwrap();
        let (_, key) = crypto::init_passphrase("correct horse battery staple").unwrap();
        let state = AppState::default();
        {
            let mut inner = state.inner.lock();
            inner.pool = Some(pool.clone());
            inner.vault = Some(Arc::new(Mutex::new(VaultState { master_key: Some(key) })));
        }
        let conn = pool.get().unwrap();
        (dir, state, conn)
    }

    fn save(state: &AppState, enabled: bool, preset: &str, url: &str, token: Option<&str>, clear: bool) -> Result<(), String> {
        save_explorer_settings_inner(
            state,
            SaveExplorerSettings { enabled, preset: preset.into(), base_url: url.into(), token: token.map(String::from), clear_token: clear },
        )
    }

    /// Everything a save can touch.
    fn stored(conn: &Connection) -> Vec<Option<String>> {
        [K_ENABLED, K_PRESET, K_URL, K_TOKEN_CT, K_TOKEN_NONCE, K_TOKEN_ORIGIN].iter().map(|k| meta(conn, k)).collect()
    }

    fn fee_request(state: &AppState, conn: &Connection) -> (Result<(), ExplorerError>, Vec<(String, Option<String>)>) {
        let cfg = load_config(state, conn, true).unwrap();
        let http = Recorder(RefCell::new(vec![]));
        let r = Explorer::new(&cfg, &http).fee_estimates().map(|_| ());
        let calls = http.0.borrow().clone();
        (r, calls)
    }

    #[test]
    fn a_new_url_cannot_inherit_a_token_saved_for_another_server() {
        let (_dir, state, conn) = setup();
        save(&state, true, "mempool", "https://mempool.space", Some("tok-for-mempool"), false).unwrap();
        assert_eq!(meta(&conn, K_TOKEN_ORIGIN).as_deref(), Some("https://mempool.space"));
        let (_, calls) = fee_request(&state, &conn);
        assert_eq!(calls[0].1.as_deref(), Some("tok-for-mempool"));
        let before = stored(&conn);

        // Choosing another provider in Settings only rewrites the URL: the old token must not follow.
        let err = save(&state, true, "esplora", "https://blockstream.info", None, false).unwrap_err();
        assert!(err.contains("belongs to https://mempool.space"), "{err}");
        assert!(!err.contains("tok-for-mempool"));
        assert_eq!(stored(&conn), before, "a refused save writes nothing");
        // Same host, other port or scheme is another origin too.
        assert!(save(&state, true, "custom", "https://mempool.space:8443", None, false).is_err());
        assert!(save(&state, true, "custom", "http://mempool.space", None, false).is_err());
        assert_eq!(stored(&conn), before);

        // Re-saving the same server keeps the token, byte for byte.
        save(&state, true, "mempool", "https://mempool.space/", None, false).unwrap();
        assert_eq!(stored(&conn), before);

        // Removing the token is the explicit way to move on.
        save(&state, true, "custom", "https://blockstream.info", None, true).unwrap();
        assert_eq!((meta(&conn, K_TOKEN_CT), meta(&conn, K_TOKEN_NONCE), meta(&conn, K_TOKEN_ORIGIN)), (None, None, None));
        assert_eq!(meta(&conn, K_URL).as_deref(), Some("https://blockstream.info"));
        let (r, calls) = fee_request(&state, &conn);
        r.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, None, "no token left to send");
    }

    #[test]
    fn a_new_token_is_bound_to_the_url_it_is_saved_with() {
        let (_dir, state, conn) = setup();
        save(&state, true, "mempool", "https://mempool.space", Some("first"), false).unwrap();
        save(&state, true, "custom", "https://blockstream.info", Some("second"), false).unwrap();
        assert_eq!(meta(&conn, K_TOKEN_ORIGIN).as_deref(), Some("https://blockstream.info"));
        let (r, calls) = fee_request(&state, &conn);
        r.unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].0.starts_with("https://blockstream.info/"), "{}", calls[0].0);
        assert_eq!(calls[0].1.as_deref(), Some("second"));
    }

    #[test]
    fn turning_the_explorer_off_does_not_touch_the_token_but_turning_it_on_elsewhere_is_refused() {
        let (_dir, state, conn) = setup();
        save(&state, true, "mempool", "https://mempool.space", Some("tok"), false).unwrap();
        let token = (meta(&conn, K_TOKEN_CT), meta(&conn, K_TOKEN_NONCE), meta(&conn, K_TOKEN_ORIGIN));
        save(&state, false, "custom", "", None, false).unwrap();
        assert_eq!((meta(&conn, K_TOKEN_CT), meta(&conn, K_TOKEN_NONCE), meta(&conn, K_TOKEN_ORIGIN)), token);
        assert!(save(&state, true, "custom", "https://my.example", None, false).is_err());
        save(&state, true, "mempool", "https://mempool.space", None, false).unwrap();
    }

    #[test]
    fn a_token_is_refused_over_plain_http_or_without_a_server_and_nothing_is_stored() {
        let (_dir, state, conn) = setup();
        let none = stored(&conn);
        for url in ["http://mempool.lan:3006", "http://192.168.1.50:3006"] {
            let err = save(&state, true, "custom", url, Some("tok"), false).unwrap_err();
            assert!(err.contains("only sent over https"), "{url}: {err}");
        }
        let err = save(&state, false, "custom", "", Some("tok"), false).unwrap_err();
        assert!(err.contains("server address"), "{err}");
        assert_eq!(stored(&conn), none);
        // To this computer, plain http is fine.
        save(&state, true, "custom", "http://localhost:3006", Some("tok"), false).unwrap();
        assert_eq!(meta(&conn, K_TOKEN_ORIGIN).as_deref(), Some("http://localhost:3006"));
        // A kept token cannot be moved onto a LAN http address either.
        assert!(save(&state, true, "custom", "http://mempool.lan:3006", None, false).is_err());
        // With no token at all, plain http to a LAN server is accepted as before.
        let (_dir2, state2, conn2) = setup();
        save(&state2, true, "custom", "http://mempool.lan:3006", None, false).unwrap();
        assert_eq!(meta(&conn2, K_URL).as_deref(), Some("http://mempool.lan:3006"));
    }

    #[test]
    fn a_token_saved_before_origins_were_recorded_is_not_sent_until_entered_again() {
        let (_dir, state, conn) = setup();
        // What an older version left behind: ciphertext and nonce, no origin.
        save(&state, true, "mempool", "https://mempool.space", Some("old-token"), false).unwrap();
        conn.execute("DELETE FROM db_meta WHERE key = ?1", rusqlite::params![K_TOKEN_ORIGIN]).unwrap();

        let (r, calls) = fee_request(&state, &conn);
        assert!(matches!(r, Err(ExplorerError::TokenNotAllowed(_))));
        assert!(calls.is_empty(), "no request, so the token is not sent anywhere");
        let err = save(&state, true, "mempool", "https://mempool.space", None, false).unwrap_err();
        assert!(err.contains("isn't tied to a server"), "{err}");

        save(&state, true, "mempool", "https://mempool.space", Some("old-token"), false).unwrap();
        let (r, calls) = fee_request(&state, &conn);
        r.unwrap();
        assert_eq!(calls[0].1.as_deref(), Some("old-token"));
    }

    // ---- R8: explorer requests must not hold the window or an async worker ----

    /// A loopback explorer that answers after `delay_ms`. Returns its port.
    fn serve_after(delay_ms: u64) -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut s = stream.unwrap();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 2048];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).to_string();
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    let body = if request.contains("/api/address/") {
                        r#"{"chain_stats":{"funded_txo_sum":10000,"spent_txo_sum":4000,"tx_count":3},"mempool_stats":{"funded_txo_sum":0,"spent_txo_sum":500,"tx_count":1}}"#.to_string()
                    } else if request.contains("/fees/recommended") {
                        FEES.to_string()
                    } else {
                        "845000".to_string()
                    };
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
                });
            }
        });
        port
    }

    fn local_cfg(port: u16) -> ExplorerConfig {
        ExplorerConfig { enabled: true, preset: Preset::Custom, base_url: format!("http://127.0.0.1:{port}"), token: None, token_origin: None }
    }

    /// Run `request` next to a task that should finish in ~50 ms, on a runtime with ONE thread. If
    /// the request held that thread, the quick task could not finish before the slow server did.
    fn quick_task_delay_while<T>(request: impl std::future::Future<Output = T>) -> (T, std::time::Duration, std::time::Duration) {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let start = std::time::Instant::now();
            let quick = async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                start.elapsed()
            };
            let (out, quick_after) = tokio::join!(request, quick);
            (out, quick_after, start.elapsed())
        })
    }

    #[test]
    fn a_slow_tip_height_request_does_not_hold_the_async_runtime() {
        let port = serve_after(1500);
        let (height, quick_after, total) = quick_task_delay_while(explorer_height_for(local_cfg(port)));
        assert_eq!((height.enabled, height.height, height.error), (true, Some(845_000), None));
        assert!(total >= std::time::Duration::from_millis(1400), "the server was supposed to be slow: {total:?}");
        assert!(quick_after < std::time::Duration::from_millis(600), "the runtime thread was held for {quick_after:?}");
    }

    #[test]
    fn a_slow_address_lookup_does_not_hold_the_async_runtime() {
        let port = serve_after(1500);
        let (lookup, quick_after, total) =
            quick_task_delay_while(explorer_lookup_for(local_cfg(port), "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4".into()));
        let lookup = lookup.unwrap();
        assert_eq!((lookup.confirmed_sats, lookup.tx_count), (6000, 4));
        assert!(total >= std::time::Duration::from_millis(1400), "{total:?}");
        assert!(quick_after < std::time::Duration::from_millis(600), "the runtime thread was held for {quick_after:?}");
    }

    #[test]
    fn a_failing_explorer_is_reported_as_data_not_a_panic() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let h = rt.block_on(explorer_height_for(local_cfg(port)));
        assert_eq!((h.enabled, h.height), (true, None));
        assert!(h.error.as_deref().is_some_and(|e| e.contains("unreachable")), "{:?}", h.error);
        let err = rt.block_on(explorer_lookup_for(local_cfg(port), "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4".into())).unwrap_err();
        assert!(err.contains("unreachable"), "{err}");
    }

    /// The fee fallback runs inside the async `get_fee_estimates`. With the explorer on, the old
    /// blocking client panicked there (and would have aborted a release build).
    #[test]
    fn the_fee_fallback_works_when_called_from_inside_an_async_task() {
        let port = serve_after(0);
        let (_dir, state, conn) = setup();
        save(&state, true, "custom", &format!("http://127.0.0.1:{port}"), None, false).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (estimate, source, status) = rt.block_on(async { explorer_fee_fallback(&state, &conn) });
        assert_eq!(status, "ok", "{source}");
        assert_eq!(estimate.unwrap().fastest_fee, 12.0);
    }
}
