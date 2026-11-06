//! Bitcoin Core JSON-RPC client wrapper. Read-only usage only — Nakos never
//! sends transactions or wallet RPCs, only chain/network/mempool queries.

use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreRpcError {
    #[error("rpc error: {0}")]
    Rpc(#[from] bitcoincore_rpc::Error),
    /// The RPC transport (jsonrpc `simple_http`) speaks plain HTTP only: it treats an
    /// `https://` URL as http on another default port and never starts TLS. A config that
    /// asks for TLS is refused rather than silently sent in the clear.
    #[error("TLS isn't supported for Bitcoin Core RPC connections")]
    TlsUnsupported,
    /// The cookie file is missing, unreadable, or not a Bitcoin Core cookie. The message never
    /// contains what the file held.
    #[error("cookie file: {0}")]
    Cookie(String),
}

/// Bitcoin Core's cookie is one line, `__cookie__:` plus 32 random bytes as 64 hex characters.
const COOKIE_USER: &str = "__cookie__";
const COOKIE_SECRET_HEX_LEN: usize = 64;
const MAX_COOKIE_FILE_BYTES: u64 = 256;

/// Read a Core cookie file. Nakos reads it itself, rather than letting the RPC library send the
/// first line of whatever file a saved path points at, and accepts only the real cookie shape: a
/// path pointed at some other file (by a mistake, or by a compromised webview paired with a
/// hostile host) can then never put that file's contents on the wire.
fn read_cookie(path: &str) -> Result<(String, String), CoreRpcError> {
    use std::io::Read;
    let bad = |why: &str| CoreRpcError::Cookie(why.to_string());
    let meta = std::fs::metadata(path).map_err(|e| CoreRpcError::Cookie(format!("could not open it ({})", e.kind())))?;
    if !meta.is_file() {
        return Err(bad("that path is not a file"));
    }
    if meta.len() > MAX_COOKIE_FILE_BYTES {
        return Err(bad("that file is too large to be a Bitcoin Core cookie"));
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_COOKIE_FILE_BYTES + 1).read_to_string(&mut text))
        .map_err(|e| CoreRpcError::Cookie(format!("could not read it ({})", e.kind())))?;
    let line = text.strip_suffix('\n').unwrap_or(&text);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let secret = line.strip_prefix(COOKIE_USER).and_then(|rest| rest.strip_prefix(':'));
    match secret {
        Some(s) if s.len() == COOKIE_SECRET_HEX_LEN && s.bytes().all(|b| b.is_ascii_hexdigit()) => Ok((COOKIE_USER.to_string(), s.to_string())),
        _ => Err(bad("that file does not look like a Bitcoin Core cookie (expected __cookie__: followed by 64 hex characters)")),
    }
}

pub struct CoreRpcConfig {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub password: Option<String>,
    /// Path to the node's `.cookie` file (default auth for Bitcoin Core
    /// when no `rpcuser`/`rpcpassword` is set in bitcoin.conf). Takes
    /// precedence over `user`/`password` when set.
    pub cookie_path: Option<String>,
    pub use_tls: bool,
}

impl CoreRpcConfig {
    /// Always plain HTTP. `connect*` refuses `use_tls` before this is used, so an `https`
    /// URL (which the transport would send unencrypted) is never built.
    fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    fn auth(&self) -> Result<Auth, CoreRpcError> {
        if let Some(cookie) = &self.cookie_path {
            let (user, pass) = read_cookie(cookie)?;
            return Ok(Auth::UserPass(user, pass));
        }
        Ok(match (&self.user, &self.password) {
            (Some(u), Some(p)) => Auth::UserPass(u.clone(), p.clone()),
            _ => Auth::None,
        })
    }

    pub fn connect(&self) -> Result<Client, CoreRpcError> {
        self.refuse_tls()?;
        Ok(Client::new(&self.url(), self.auth()?)?)
    }

    /// Checked first in every `connect*`, before the cookie file is read or a socket exists.
    fn refuse_tls(&self) -> Result<(), CoreRpcError> {
        if self.use_tls {
            return Err(CoreRpcError::TlsUnsupported);
        }
        Ok(())
    }

    /// Same client, but with a finite connect/read/write timeout instead of
    /// the transport's 15 s default. Used by the status poll; `connect()` is
    /// unchanged for the other callers.
    pub fn connect_with_timeout(&self, timeout: std::time::Duration) -> Result<Client, CoreRpcError> {
        use bitcoincore_rpc::jsonrpc::{self, simple_http::SimpleHttpTransport};
        self.refuse_tls()?;
        let (user, pass) = self.auth()?.get_user_pass()?;
        let mut builder = SimpleHttpTransport::builder()
            .url(&self.url())
            .map_err(|e| bitcoincore_rpc::Error::JsonRpc(e.into()))?
            .timeout(timeout);
        if let Some(user) = user {
            builder = builder.auth(user, pass);
        }
        Ok(Client::from_jsonrpc(jsonrpc::Client::with_transport(builder.build())))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub chain: String,
    pub blocks: u64,
    pub headers: u64,
    pub verification_progress: f64,
    pub is_synced: bool,
    pub initial_block_download: bool,
    pub size_on_disk_bytes: u64,
    pub prune_height: Option<u64>,
    pub is_pruned: bool,
    pub connections_in: i64,
    pub connections_out: i64,
    pub version: u64,
    pub subversion: String,
    pub mempool_tx_count: u64,
    pub mempool_size_bytes: u64,
    pub network_reachable_via_tor: bool,
}

pub fn get_node_status(client: &Client) -> Result<NodeStatus, CoreRpcError> {
    let bc = client.get_blockchain_info()?;
    let net = client.get_network_info()?;
    let mempool = client.get_mempool_info()?;

    let onion_reachable = net
        .networks
        .iter()
        .any(|n| n.name == "onion" && n.reachable);

    Ok(NodeStatus {
        chain: bc.chain.to_string(),
        blocks: bc.blocks,
        headers: bc.headers,
        verification_progress: bc.verification_progress,
        is_synced: bc.blocks == bc.headers && bc.verification_progress > 0.9999 && !bc.initial_block_download,
        initial_block_download: bc.initial_block_download,
        size_on_disk_bytes: bc.size_on_disk,
        prune_height: bc.prune_height,
        is_pruned: bc.pruned,
        connections_in: net.connections_in.unwrap_or(0) as i64,
        connections_out: net.connections_out.unwrap_or(0) as i64,
        version: net.version as u64,
        subversion: net.subversion,
        mempool_tx_count: mempool.size as u64,
        mempool_size_bytes: mempool.bytes as u64,
        network_reachable_via_tor: onion_reachable,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeEstimate {
    pub target_blocks: u16,
    pub sat_per_vb: Option<f64>,
}

pub fn estimate_fees(client: &Client) -> Result<Vec<FeeEstimate>, CoreRpcError> {
    let targets = [1u16, 3, 6, 12, 24, 144];
    let mut out = Vec::with_capacity(targets.len());
    for t in targets {
        let est = client.estimate_smart_fee(t, None)?;
        let sat_per_vb = est.fee_rate.map(|amt| amt.to_sat() as f64 / 1000.0);
        out.push(FeeEstimate { target_blocks: t, sat_per_vb });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config that asks for TLS is refused before the cookie file is read (the path below
    /// does not exist, so any read would surface a different error) and before any socket.
    #[test]
    fn a_tls_config_is_refused_by_both_connect_paths() {
        let cfg = CoreRpcConfig {
            host: "127.0.0.1".into(),
            port: 1,
            user: None,
            password: None,
            cookie_path: Some("/nonexistent/.cookie".into()),
            use_tls: true,
        };
        assert!(matches!(cfg.connect(), Err(CoreRpcError::TlsUnsupported)));
        assert!(matches!(cfg.connect_with_timeout(std::time::Duration::from_secs(1)), Err(CoreRpcError::TlsUnsupported)));
    }

    // ---- R11: the cookie file path is not a way to send some other file's first line ----

    const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// A cookie file in its own scratch folder; keep the folder until the test is done with the file.
    fn temp_file(contents: &[u8]) -> (crate::test_dir::TestDir, std::path::PathBuf) {
        let dir = crate::test_dir::TestDir::new("cookie");
        let path = dir.join(".cookie");
        std::fs::write(&path, contents).unwrap();
        (dir, path)
    }

    fn cfg_with_cookie(path: &str, port: u16) -> CoreRpcConfig {
        CoreRpcConfig { host: "127.0.0.1".into(), port, user: None, password: None, cookie_path: Some(path.into()), use_tls: false }
    }

    #[test]
    fn a_real_cookie_file_is_accepted_with_or_without_a_trailing_newline() {
        for body in [format!("__cookie__:{SECRET}\n"), format!("__cookie__:{SECRET}"), format!("__cookie__:{SECRET}\r\n"), format!("__cookie__:{}", SECRET.to_uppercase())] {
            let (_dir, path) = temp_file(body.as_bytes());
            let (user, pass) = read_cookie(path.to_str().unwrap()).unwrap();
            assert_eq!(user, "__cookie__");
            assert_eq!(pass.to_lowercase(), SECRET);
        }
    }

    #[test]
    fn a_file_that_is_not_a_core_cookie_is_refused_and_its_contents_never_appear_in_the_error() {
        let secret_line = "root:hunter2-very-secret";
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("some other file", format!("{secret_line}\nsecond line\n").into_bytes()),
            ("an ssh-key-like file", b"-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n".to_vec()),
            ("wrong user", format!("rpcuser:{SECRET}\n").into_bytes()),
            ("secret too short", b"__cookie__:abcd\n".to_vec()),
            ("secret not hex", format!("__cookie__:{}\n", "z".repeat(64)).into_bytes()),
            ("an extra line", format!("__cookie__:{SECRET}\n{secret_line}\n").into_bytes()),
            ("trailing text", format!("__cookie__:{SECRET} {secret_line}\n").into_bytes()),
            ("empty", Vec::new()),
            ("too large", vec![b'a'; 5_000]),
        ];
        for (what, body) in cases {
            let (_dir, path) = temp_file(&body);
            let err = read_cookie(path.to_str().unwrap()).unwrap_err().to_string();
            assert!(!err.contains(secret_line) && !err.contains("PRIVATE KEY") && !err.contains(SECRET), "{what}: {err}");
            assert!(err.starts_with("cookie file:"), "{what}: {err}");
        }
        // A directory and a missing path are refused too.
        let (dir, _) = temp_file(b"x");
        assert!(read_cookie(dir.to_str().unwrap()).is_err());
        assert!(read_cookie("/nonexistent/dir/.cookie").is_err());
    }

    /// The worst case from the review: a saved cookie path pointing at some secret file, and a
    /// host that is an attacker's. Nothing may be sent: no connection is even opened.
    #[test]
    fn a_cookie_path_pointing_at_another_file_sends_nothing_to_the_host() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (_dir, path) = temp_file(b"root:hunter2-very-secret\n");
        let cfg = cfg_with_cookie(path.to_str().unwrap(), port);
        assert!(matches!(cfg.connect(), Err(CoreRpcError::Cookie(_))));
        let client = cfg.connect_with_timeout(std::time::Duration::from_secs(2));
        assert!(matches!(client, Err(CoreRpcError::Cookie(_))));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(listener.accept().is_err(), "a connection reached the host");
    }

    /// And the real cookie still authenticates: the request carries `__cookie__:<secret>` as Basic auth.
    #[test]
    fn a_real_cookie_is_sent_as_basic_auth() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                *seen2.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = r#"{"result":123,"error":null,"id":1}"#;
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
            }
        });
        let (_dir, path) = temp_file(format!("__cookie__:{SECRET}\n").as_bytes());
        let client = cfg_with_cookie(path.to_str().unwrap(), port).connect_with_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(bitcoincore_rpc::RpcApi::get_block_count(&client).unwrap(), 123);
        let request = seen.lock().unwrap().to_lowercase();
        let want = {
            use bitcoin::base64::{engine::general_purpose::STANDARD, Engine};
            STANDARD.encode(format!("__cookie__:{SECRET}"))
        };
        assert!(request.contains(&format!("authorization: basic {}", want.to_lowercase())), "{request}");
    }

    /// A named pipe at the cookie path would block a read forever; it must be refused up front.
    #[cfg(unix)]
    #[test]
    fn a_named_pipe_at_the_cookie_path_is_refused_instead_of_hanging() {
        let (dir, _) = temp_file(b"x");
        let fifo = dir.join("pipe.cookie");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status().map(|s| s.success()).unwrap_or(false);
        assert!(made, "mkfifo is needed for this test");
        let (tx, rx) = std::sync::mpsc::channel();
        let path = fifo.to_str().unwrap().to_string();
        std::thread::spawn(move || {
            let _ = tx.send(read_cookie(&path).is_err());
        });
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(3)), Ok(true), "reading a named pipe hung or succeeded");
    }
}
