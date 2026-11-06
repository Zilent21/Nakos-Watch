//! Electrum protocol client. Used both as a fallback data source for
//! balances/UTXOs (via BDK, see `bitcoin::utxo`) and directly here for
//! single-address wallets and node/server diagnostics.

use bitcoin::{Address, Script};
use electrum_client::{Client, ConfigBuilder, ElectrumApi};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ElectrumError {
    #[error("electrum error: {0}")]
    Client(#[from] electrum_client::Error),
}

pub struct ElectrumConfig {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
}

impl ElectrumConfig {
    fn url(&self) -> String {
        let scheme = if self.use_tls { "ssl" } else { "tcp" };
        format!("{scheme}://{}:{}", self.host, self.port)
    }

    /// 10 s timeout: shared with wallet sync, so do not shorten it here.
    pub fn connect(&self) -> Result<Client, ElectrumError> {
        self.connect_with_timeout(10)
    }

    pub fn connect_with_timeout(&self, secs: u8) -> Result<Client, ElectrumError> {
        self.build_client(secs, None)
    }

    /// Like `connect_with_timeout`, but the client gives up after one attempt. The library
    /// retries a failed call once by default, which doubles every timeout; the status, fee and
    /// health polls want a single short wait instead. `connect()` (wallet sync) is unchanged.
    pub fn connect_once_with_timeout(&self, secs: u8) -> Result<Client, ElectrumError> {
        self.build_client(secs, Some(0))
    }

    fn build_client(&self, secs: u8, retry: Option<u8>) -> Result<Client, ElectrumError> {
        super::ensure_tls_provider();
        let mut builder = ConfigBuilder::new().timeout(Some(Duration::from_secs(secs.into())));
        if let Some(retry) = retry {
            builder = builder.retry(retry);
        }
        Ok(Client::from_config(&self.url(), builder.build())?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectrumServerStatus {
    pub server_version: String,
    pub protocol_version: String,
    pub tip_height: u32,
    pub connected: bool,
}

pub fn get_server_status(client: &Client) -> Result<ElectrumServerStatus, ElectrumError> {
    let features = client.server_features()?;
    let header = client.block_headers_subscribe()?;
    Ok(ElectrumServerStatus {
        server_version: features.server_version,
        protocol_version: features.protocol_max,
        tip_height: header.height as u32,
        connected: true,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectrumUtxo {
    pub txid: String,
    pub vout: u32,
    pub amount_sats: u64,
    pub height: i64,
}

/// List UTXOs for a single watch-only address (used for `source_type =
/// address` wallets that have no descriptor to derive from).
pub fn list_address_utxos(client: &Client, address: &Address) -> Result<Vec<ElectrumUtxo>, ElectrumError> {
    let spk = address.script_pubkey();
    let script: &Script = spk.as_script();
    let unspent = client.script_list_unspent(script)?;
    Ok(unspent
        .into_iter()
        .map(|u| ElectrumUtxo {
            txid: u.tx_hash.to_string(),
            vout: u.tx_pos as u32,
            amount_sats: u.value,
            height: u.height as i64,
        })
        .collect())
}

pub fn estimate_fee_sat_per_vb(client: &Client, target_blocks: usize) -> Result<Option<f64>, ElectrumError> {
    let btc_per_kb = client.estimate_fee(target_blocks, None)?;
    if btc_per_kb <= 0.0 {
        return Ok(None);
    }
    Ok(Some(btc_per_kb * 100_000_000.0 / 1000.0))
}

pub fn ping(host: &str, port: u16, use_tls: bool) -> Result<Duration, ElectrumError> {
    let start = std::time::Instant::now();
    let cfg = ElectrumConfig { host: host.to_string(), port, use_tls };
    let client = cfg.connect()?;
    client.ping()?;
    Ok(start.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A TLS Electrum config pointed at a loopback listener that accepts and hangs up must
    /// fail with an error. Before the TLS provider was installed this panicked inside rustls
    /// (and aborted the app) as soon as the client config was built.
    #[test]
    fn tls_client_against_a_non_tls_listener_errors_instead_of_panicking() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);
            }
        });
        let cfg = ElectrumConfig { host: "127.0.0.1".into(), port, use_tls: true };
        let result = cfg.connect_with_timeout(3).and_then(|c| c.ping().map_err(ElectrumError::from));
        assert!(result.is_err(), "a TLS handshake against a non-TLS listener cannot succeed");
        server.join().unwrap();
    }
}
