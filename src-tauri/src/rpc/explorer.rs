//! Optional public-explorer client. OFF by default; nothing in this module
//! makes an HTTP request unless the user has switched it on in Settings.
//!
//! Electrum/electrs is the wallet index and Bitcoin Core RPC is node health;
//! this is neither. It is never used to discover addresses from an xpub, and
//! it only ever makes three kinds of request:
//!   * fee estimates (planner / Node Ops, only when local sources have none),
//!   * chain tip height (a cross-check against Core),
//!   * an explicit, user-clicked lookup of ONE address.
//! The server sees whatever you send it: the addresses you look up and your
//! fee requests.
//!
//! future: LND/CLN or other optional providers would be added beside this,
//! behind their own opt-in; none exist in this pass.

use crate::rpc::fees::PublicFeeEstimate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// mempool.space API (also self-hosted mempool instances).
    Mempool,
    /// Esplora API (blockstream.info).
    Esplora,
    Blockcypher,
    /// Your own URL, treated as a mempool-compatible API.
    Custom,
}

impl Preset {
    pub fn as_str(&self) -> &'static str {
        match self {
            Preset::Mempool => "mempool",
            Preset::Esplora => "esplora",
            Preset::Blockcypher => "blockcypher",
            Preset::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<Preset> {
        match s {
            "mempool" => Some(Preset::Mempool),
            "esplora" => Some(Preset::Esplora),
            "blockcypher" => Some(Preset::Blockcypher),
            "custom" => Some(Preset::Custom),
            _ => None,
        }
    }

    /// Prefilled into the URL box. Choosing a preset calls nothing.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            Preset::Mempool => "https://mempool.space",
            Preset::Esplora => "https://blockstream.info",
            Preset::Blockcypher => "https://api.blockcypher.com/v1/btc/main",
            Preset::Custom => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExplorerConfig {
    pub enabled: bool,
    pub preset: Preset,
    pub base_url: String,
    pub token: Option<String>,
    /// The server origin (`https://host:port`) the token was saved for. A token is only ever
    /// sent to that origin; `None` (a token saved before this was recorded) means it is not sent.
    pub token_origin: Option<String>,
}

impl Default for ExplorerConfig {
    fn default() -> Self {
        ExplorerConfig {
            enabled: false,
            preset: Preset::Mempool,
            base_url: Preset::Mempool.default_base_url().into(),
            token: None,
            token_origin: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum ExplorerError {
    #[error("public explorer is disabled")]
    Disabled,
    #[error("invalid explorer URL: {0}")]
    BadUrl(String),
    #[error("that doesn't look like a Bitcoin address")]
    BadAddress,
    #[error("explorer unreachable: {0}")]
    Unreachable(String),
    #[error("unexpected explorer response: {0}")]
    BadResponse(String),
    /// The API token may not be sent to this server. Nothing was requested.
    #[error("{0}")]
    TokenNotAllowed(String),
}

/// The one place HTTP happens, behind a trait so tests can prove that a
/// disabled explorer never reaches it.
pub trait Http {
    /// GET `url`, optionally with a bearer token. Errors must not contain the URL
    /// (it can carry a token).
    fn get(&self, url: &str, bearer: Option<&str>) -> Result<String, String>;
}

pub struct ReqwestHttp;

impl Http for ReqwestHttp {
    /// `reqwest::blocking` builds and drops its own runtime, which panics (and with
    /// `panic = "abort"` ends the app) if done on a thread that belongs to an async runtime. The
    /// commands that call this are async, so the request always runs on a fresh OS thread of its
    /// own, which is safe from any caller. The calling thread waits for it, at most the timeout.
    fn get(&self, url: &str, bearer: Option<&str>) -> Result<String, String> {
        std::thread::scope(|s| {
            s.spawn(|| blocking_get(url, bearer)).join().unwrap_or_else(|_| Err("the request thread failed".to_string()))
        })
    }
}

fn blocking_get(url: &str, bearer: Option<&str>) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| e.without_url().to_string())?;
    let mut req = client.get(url);
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    let resp = req.send().map_err(|e| e.without_url().to_string())?;
    let resp = resp.error_for_status().map_err(|e| e.without_url().to_string())?;
    resp.text().map_err(|e| e.without_url().to_string())
}

/// Normalise and validate a base URL: http(s) only, a host, no embedded
/// credentials, no query or fragment. Trailing slashes are dropped.
pub fn validate_base_url(raw: &str) -> Result<String, ExplorerError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ExplorerError::BadUrl("enter a server address".into()));
    }
    let url = reqwest::Url::parse(raw).map_err(|_| ExplorerError::BadUrl("not a valid URL".into()))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(ExplorerError::BadUrl("must start with http:// or https://".into()));
    }
    if url.host_str().is_none() {
        return Err(ExplorerError::BadUrl("missing host".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ExplorerError::BadUrl("don't put credentials in the URL; use the API token field".into()));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ExplorerError::BadUrl("leave off ?query and #fragment parts".into()));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// `scheme://host:port` of a URL (default ports normalised, host lower-cased), the unit a token
/// is tied to. `None` if the text is not a URL.
pub fn origin_of(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url.trim()).ok()?;
    url.has_host().then(|| url.origin().ascii_serialization())
}

/// An API token is a secret, so it only goes over https, or over plain http to this computer
/// (localhost, 127.0.0.0/8, ::1). Plain http to any other host, a LAN address included, would
/// put it on the network in the clear. Requests that carry no token are not affected.
pub fn check_token_transport(base_url: &str) -> Result<(), ExplorerError> {
    let url = reqwest::Url::parse(base_url.trim()).map_err(|_| ExplorerError::BadUrl("not a valid URL".into()))?;
    let host = url.host_str().unwrap_or("");
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
        || host.trim_matches(['[', ']']).parse::<std::net::Ipv6Addr>().is_ok_and(|ip| ip.is_loopback());
    if url.scheme() == "https" || (url.scheme() == "http" && loopback) {
        Ok(())
    } else {
        Err(ExplorerError::TokenNotAllowed(
            "an API token is only sent over https (or to this computer). Use an https:// server address, or remove the token".into(),
        ))
    }
}

/// May the stored token be sent to `base_url`? Only to the origin it was saved for, and only
/// over an acceptable transport.
pub fn check_token_destination(base_url: &str, token_origin: Option<&str>) -> Result<(), ExplorerError> {
    check_token_transport(base_url)?;
    match (token_origin, origin_of(base_url)) {
        (Some(saved), Some(now)) if saved == now => Ok(()),
        (Some(saved), _) => Err(ExplorerError::TokenNotAllowed(format!(
            "the saved API token belongs to {saved}, not this server. Enter a token for this server, or remove the token"
        ))),
        (None, _) => Err(ExplorerError::TokenNotAllowed(
            "the saved API token isn't tied to a server (it was saved by an older version). Enter it again for this server, or remove it".into(),
        )),
    }
}

/// Only well-formed Bitcoin addresses are ever placed into a request path.
pub fn validate_address(addr: &str) -> Result<String, ExplorerError> {
    let a = addr.trim();
    if a.is_empty() || a.len() > 100 {
        return Err(ExplorerError::BadAddress);
    }
    a.parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>().map_err(|_| ExplorerError::BadAddress)?;
    Ok(a.to_string())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddressLookup {
    pub address: String,
    pub confirmed_sats: i64,
    pub unconfirmed_sats: i64,
    pub total_received_sats: u64,
    pub tx_count: u64,
}

pub struct Explorer<'a, H: Http> {
    cfg: &'a ExplorerConfig,
    http: &'a H,
}

impl<'a, H: Http> Explorer<'a, H> {
    pub fn new(cfg: &'a ExplorerConfig, http: &'a H) -> Self {
        Explorer { cfg, http }
    }

    /// Every public method starts here, so a disabled explorer can't reach HTTP.
    fn gate(&self) -> Result<String, ExplorerError> {
        if !self.cfg.enabled {
            return Err(ExplorerError::Disabled);
        }
        let base = validate_base_url(&self.cfg.base_url)?;
        // A token that may not go to this server stops the request, visibly, before any HTTP.
        if self.cfg.token.as_deref().is_some_and(|t| !t.is_empty()) {
            check_token_destination(&base, self.cfg.token_origin.as_deref())?;
        }
        Ok(base)
    }

    /// (url, bearer): BlockCypher takes the token as a query parameter, the
    /// others as a bearer header.
    fn request(&self, base: &str, path: &str) -> (String, Option<&str>) {
        let token = self.cfg.token.as_deref().filter(|t| !t.is_empty());
        let mut url = format!("{base}{path}");
        match (self.cfg.preset, token) {
            (Preset::Blockcypher, Some(t)) => {
                url.push_str(if url.contains('?') { "&token=" } else { "?token=" });
                url.push_str(t);
                (url, None)
            }
            (_, t) => (url, t),
        }
    }

    fn fetch(&self, base: &str, path: &str) -> Result<String, ExplorerError> {
        let (url, bearer) = self.request(base, path);
        self.http.get(&url, bearer).map_err(ExplorerError::Unreachable)
    }

    pub fn fee_estimates(&self) -> Result<PublicFeeEstimate, ExplorerError> {
        let base = self.gate()?;
        match self.cfg.preset {
            Preset::Mempool | Preset::Custom => {
                #[derive(Deserialize)]
                struct R {
                    #[serde(rename = "fastestFee")]
                    fastest: f64,
                    #[serde(rename = "halfHourFee")]
                    half_hour: f64,
                    #[serde(rename = "hourFee")]
                    hour: f64,
                    #[serde(rename = "economyFee")]
                    economy: f64,
                    #[serde(rename = "minimumFee")]
                    minimum: f64,
                }
                let r: R = parse(&self.fetch(&base, "/api/v1/fees/recommended")?)?;
                Ok(PublicFeeEstimate { fastest_fee: r.fastest, half_hour_fee: r.half_hour, hour_fee: r.hour, economy_fee: r.economy, minimum_fee: r.minimum })
            }
            Preset::Esplora => {
                let map: HashMap<String, f64> = parse(&self.fetch(&base, "/api/fee-estimates")?)?;
                let est = |target: u32| nearest_target(&map, target);
                let economy = est(144).ok_or_else(|| ExplorerError::BadResponse("empty fee table".into()))?;
                Ok(PublicFeeEstimate {
                    fastest_fee: est(1).unwrap_or(economy),
                    half_hour_fee: est(3).unwrap_or(economy),
                    hour_fee: est(6).unwrap_or(economy),
                    economy_fee: economy,
                    minimum_fee: est(1008).unwrap_or(economy),
                })
            }
            Preset::Blockcypher => {
                #[derive(Deserialize)]
                struct R {
                    high_fee_per_kb: f64,
                    medium_fee_per_kb: f64,
                    low_fee_per_kb: f64,
                }
                let r: R = parse(&self.fetch(&base, "")?)?;
                let per_vb = |kb: f64| kb / 1000.0;
                Ok(PublicFeeEstimate {
                    fastest_fee: per_vb(r.high_fee_per_kb),
                    half_hour_fee: per_vb(r.medium_fee_per_kb),
                    hour_fee: per_vb(r.low_fee_per_kb),
                    economy_fee: per_vb(r.low_fee_per_kb),
                    minimum_fee: per_vb(r.low_fee_per_kb),
                })
            }
        }
    }

    pub fn tip_height(&self) -> Result<u64, ExplorerError> {
        let base = self.gate()?;
        match self.cfg.preset {
            Preset::Blockcypher => {
                #[derive(Deserialize)]
                struct R {
                    height: u64,
                }
                Ok(parse::<R>(&self.fetch(&base, "")?)?.height)
            }
            _ => self
                .fetch(&base, "/api/blocks/tip/height")?
                .trim()
                .parse::<u64>()
                .map_err(|_| ExplorerError::BadResponse("tip height wasn't a number".into())),
        }
    }

    /// One address per request, only on an explicit user action.
    pub fn lookup_address(&self, address: &str) -> Result<AddressLookup, ExplorerError> {
        let base = self.gate()?;
        let addr = validate_address(address)?;
        match self.cfg.preset {
            Preset::Blockcypher => {
                #[derive(Deserialize)]
                struct R {
                    balance: i64,
                    unconfirmed_balance: i64,
                    total_received: u64,
                    n_tx: u64,
                }
                let r: R = parse(&self.fetch(&base, &format!("/addrs/{addr}/balance"))?)?;
                Ok(AddressLookup { address: addr, confirmed_sats: r.balance, unconfirmed_sats: r.unconfirmed_balance, total_received_sats: r.total_received, tx_count: r.n_tx })
            }
            _ => {
                #[derive(Deserialize)]
                struct Stats {
                    funded_txo_sum: u64,
                    spent_txo_sum: u64,
                    tx_count: u64,
                }
                #[derive(Deserialize)]
                struct R {
                    chain_stats: Stats,
                    mempool_stats: Stats,
                }
                let r: R = parse(&self.fetch(&base, &format!("/api/address/{addr}"))?)?;
                Ok(AddressLookup {
                    address: addr,
                    confirmed_sats: r.chain_stats.funded_txo_sum as i64 - r.chain_stats.spent_txo_sum as i64,
                    unconfirmed_sats: r.mempool_stats.funded_txo_sum as i64 - r.mempool_stats.spent_txo_sum as i64,
                    total_received_sats: r.chain_stats.funded_txo_sum,
                    tx_count: r.chain_stats.tx_count + r.mempool_stats.tx_count,
                })
            }
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, ExplorerError> {
    serde_json::from_str(body).map_err(|e| ExplorerError::BadResponse(e.to_string()))
}

/// Esplora fee table value for `target` blocks, or the nearest slower target.
fn nearest_target(map: &HashMap<String, f64>, target: u32) -> Option<f64> {
    let mut keyed: Vec<(u32, f64)> = map.iter().filter_map(|(k, v)| k.parse::<u32>().ok().map(|k| (k, *v))).collect();
    keyed.sort_by_key(|(k, _)| *k);
    keyed.iter().find(|(k, _)| *k >= target).or(keyed.last()).map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records every request and answers from a canned body.
    struct Mock {
        calls: RefCell<Vec<(String, Option<String>)>>,
        body: String,
        fail: bool,
    }

    impl Mock {
        fn new(body: &str) -> Mock {
            Mock { calls: RefCell::new(vec![]), body: body.into(), fail: false }
        }
        fn count(&self) -> usize {
            self.calls.borrow().len()
        }
    }

    impl Http for Mock {
        fn get(&self, url: &str, bearer: Option<&str>) -> Result<String, String> {
            self.calls.borrow_mut().push((url.to_string(), bearer.map(String::from)));
            if self.fail {
                Err("connection refused".into())
            } else {
                Ok(self.body.clone())
            }
        }
    }

    const ADDR: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";

    fn cfg(preset: Preset, enabled: bool, token: Option<&str>) -> ExplorerConfig {
        ExplorerConfig {
            enabled,
            preset,
            base_url: preset.default_base_url().to_string(),
            token: token.map(String::from),
            token_origin: token.and_then(|_| origin_of(preset.default_base_url())),
        }
    }

    #[test]
    fn disabled_explorer_makes_no_http_at_all() {
        for preset in [Preset::Mempool, Preset::Esplora, Preset::Blockcypher, Preset::Custom] {
            let mut c = cfg(preset, false, Some("secret"));
            c.base_url = "https://example.test".into();
            let http = Mock::new("{}");
            let ex = Explorer::new(&c, &http);
            assert!(matches!(ex.fee_estimates(), Err(ExplorerError::Disabled)));
            assert!(matches!(ex.tip_height(), Err(ExplorerError::Disabled)));
            assert!(matches!(ex.lookup_address(ADDR), Err(ExplorerError::Disabled)));
            assert_eq!(http.count(), 0, "{preset:?}: a disabled explorer must never reach HTTP");
        }
    }

    #[test]
    fn default_config_is_off() {
        assert!(!ExplorerConfig::default().enabled);
    }

    #[test]
    fn mempool_endpoints_and_bearer_token() {
        let c = cfg(Preset::Mempool, true, Some("tok123"));
        let http = Mock::new(r#"{"fastestFee":12,"halfHourFee":9,"hourFee":6,"economyFee":3,"minimumFee":1}"#);
        let f = Explorer::new(&c, &http).fee_estimates().unwrap();
        assert_eq!((f.fastest_fee, f.half_hour_fee, f.minimum_fee), (12.0, 9.0, 1.0));
        let (url, bearer) = http.calls.borrow()[0].clone();
        assert_eq!(url, "https://mempool.space/api/v1/fees/recommended");
        assert_eq!(bearer.as_deref(), Some("tok123"), "token goes in a header, not the URL");
        assert!(!url.contains("tok123"));

        let http = Mock::new("845000\n");
        assert_eq!(Explorer::new(&c, &http).tip_height().unwrap(), 845_000);
        assert_eq!(http.calls.borrow()[0].0, "https://mempool.space/api/blocks/tip/height");

        let http = Mock::new(r#"{"chain_stats":{"funded_txo_sum":10000,"spent_txo_sum":4000,"tx_count":3},"mempool_stats":{"funded_txo_sum":0,"spent_txo_sum":500,"tx_count":1}}"#);
        let a = Explorer::new(&c, &http).lookup_address(ADDR).unwrap();
        assert_eq!((a.confirmed_sats, a.unconfirmed_sats, a.total_received_sats, a.tx_count), (6000, -500, 10_000, 4));
        assert_eq!(http.calls.borrow()[0].0, format!("https://mempool.space/api/address/{ADDR}"));
    }

    #[test]
    fn esplora_fee_table_maps_to_targets() {
        let c = cfg(Preset::Esplora, true, None);
        let http = Mock::new(r#"{"1":40.0,"2":30.0,"3":22.5,"6":15.0,"25":8.0,"144":2.0,"1008":1.0}"#);
        let f = Explorer::new(&c, &http).fee_estimates().unwrap();
        assert_eq!((f.fastest_fee, f.half_hour_fee, f.hour_fee, f.economy_fee, f.minimum_fee), (40.0, 22.5, 15.0, 2.0, 1.0));
        assert_eq!(http.calls.borrow()[0].0, "https://blockstream.info/api/fee-estimates");
    }

    #[test]
    fn blockcypher_uses_query_token_and_chain_info() {
        let c = cfg(Preset::Blockcypher, true, Some("bc-token"));
        let http = Mock::new(r#"{"height":845001,"high_fee_per_kb":30000,"medium_fee_per_kb":15000,"low_fee_per_kb":5000}"#);
        let ex = Explorer::new(&c, &http);
        let f = ex.fee_estimates().unwrap();
        assert_eq!((f.fastest_fee, f.half_hour_fee, f.economy_fee), (30.0, 15.0, 5.0));
        assert_eq!(ex.tip_height().unwrap(), 845_001);
        let (url, bearer) = http.calls.borrow()[0].clone();
        assert_eq!(url, "https://api.blockcypher.com/v1/btc/main?token=bc-token");
        assert_eq!(bearer, None);

        let http = Mock::new(r#"{"balance":7000,"unconfirmed_balance":0,"total_received":9000,"n_tx":2}"#);
        let a = Explorer::new(&c, &http).lookup_address(ADDR).unwrap();
        assert_eq!((a.confirmed_sats, a.total_received_sats, a.tx_count), (7000, 9000, 2));
        assert_eq!(http.calls.borrow()[0].0, format!("https://api.blockcypher.com/v1/btc/main/addrs/{ADDR}/balance?token=bc-token"));
    }

    #[test]
    fn bad_addresses_never_reach_http() {
        let c = cfg(Preset::Mempool, true, None);
        let http = Mock::new("{}");
        let ex = Explorer::new(&c, &http);
        for bad in ["", "   ", "../../etc/passwd", "bc1q not an address", "https://evil.test/x"] {
            assert!(matches!(ex.lookup_address(bad), Err(ExplorerError::BadAddress)), "{bad}");
        }
        assert_eq!(http.count(), 0);
    }

    #[test]
    fn base_url_validation() {
        assert_eq!(validate_base_url(" https://mempool.space/ ").unwrap(), "https://mempool.space");
        assert_eq!(validate_base_url("http://my-node.local:3006").unwrap(), "http://my-node.local:3006");
        for bad in ["", "ftp://x.test", "mempool.space", "https://user:pw@x.test", "https://x.test/?token=abc", "https://x.test/#a"] {
            assert!(validate_base_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn errors_are_visible_and_never_contain_the_token() {
        let c = cfg(Preset::Blockcypher, true, Some("super-secret-token"));
        let mut http = Mock::new("");
        http.fail = true;
        let err = Explorer::new(&c, &http).fee_estimates().unwrap_err().to_string();
        assert!(err.contains("unreachable") && !err.contains("super-secret-token"), "{err}");
        let bad_body = Mock::new("<html>nope</html>");
        let err = Explorer::new(&c, &bad_body).tip_height().unwrap_err().to_string();
        assert!(err.contains("unexpected explorer response") && !err.contains("super-secret-token"), "{err}");
    }

    const FEES: &str = r#"{"fastestFee":12,"halfHourFee":9,"hourFee":6,"economyFee":3,"minimumFee":1}"#;

    /// Every public method, so a refused token is shown to stop all of them before HTTP.
    fn assert_token_refused(c: &ExplorerConfig, why: &str) {
        let http = Mock::new(FEES);
        let ex = Explorer::new(c, &http);
        assert!(matches!(ex.fee_estimates(), Err(ExplorerError::TokenNotAllowed(_))), "{why}: fees");
        assert!(matches!(ex.tip_height(), Err(ExplorerError::TokenNotAllowed(_))), "{why}: tip");
        assert!(matches!(ex.lookup_address(ADDR), Err(ExplorerError::TokenNotAllowed(_))), "{why}: lookup");
        assert_eq!(http.count(), 0, "{why}: a refused token must not reach HTTP");
    }

    #[test]
    fn a_token_is_only_sent_to_the_origin_it_was_saved_for() {
        // Saved for mempool.space, URL now points elsewhere (a different preset or a typo).
        for (preset, url) in [
            (Preset::Esplora, "https://blockstream.info"),
            (Preset::Custom, "https://other.example"),
            (Preset::Mempool, "https://mempool.space:8443"), // same host, other port
            (Preset::Mempool, "http://mempool.space"),       // same host, other scheme
        ] {
            let mut c = cfg(Preset::Mempool, true, Some("tok123"));
            c.preset = preset;
            c.base_url = url.into();
            assert_token_refused(&c, url);
        }
        // BlockCypher puts the token in the URL query; it is gated the same way.
        let mut c = cfg(Preset::Mempool, true, Some("tok123"));
        c.preset = Preset::Blockcypher;
        c.base_url = Preset::Blockcypher.default_base_url().into();
        assert_token_refused(&c, "blockcypher with a mempool token");

        // Same origin, however it is spelled, still works and sends the token.
        let mut c = cfg(Preset::Mempool, true, Some("tok123"));
        c.base_url = "https://Mempool.space:443/".into();
        let http = Mock::new(FEES);
        Explorer::new(&c, &http).fee_estimates().unwrap();
        assert_eq!(http.calls.borrow()[0].1.as_deref(), Some("tok123"));
    }

    #[test]
    fn a_token_with_no_recorded_origin_is_not_sent() {
        let mut c = cfg(Preset::Mempool, true, Some("tok123"));
        c.token_origin = None; // saved by a version that did not record one
        assert_token_refused(&c, "legacy token");
        // No token at all is unaffected.
        let c = cfg(Preset::Mempool, true, None);
        let http = Mock::new(FEES);
        Explorer::new(&c, &http).fee_estimates().unwrap();
        assert_eq!(http.calls.borrow()[0].1, None);
    }

    #[test]
    fn a_token_never_goes_over_plain_http_except_to_this_computer() {
        for url in ["http://mempool.lan:3006", "http://192.168.1.50:3006", "http://10.0.0.2", "http://localhost.example", "http://127.0.0.1.example"] {
            let mut c = cfg(Preset::Custom, true, Some("tok123"));
            c.base_url = url.into();
            c.token_origin = origin_of(url); // even bound to exactly this origin
            assert_token_refused(&c, url);
        }
        for url in ["http://localhost:3006", "http://LOCALHOST", "http://127.0.0.1:8080", "http://127.5.5.5", "http://[::1]:3000", "https://mempool.example"] {
            let mut c = cfg(Preset::Custom, true, Some("tok123"));
            c.base_url = url.into();
            c.token_origin = origin_of(url);
            let http = Mock::new(FEES);
            Explorer::new(&c, &http).fee_estimates().unwrap_or_else(|e| panic!("{url}: {e}"));
            assert_eq!(http.calls.borrow()[0].1.as_deref(), Some("tok123"), "{url}");
        }
        // Without a token, plain http to a LAN server is still allowed (nothing secret is sent).
        let mut c = cfg(Preset::Custom, true, None);
        c.base_url = "http://mempool.lan:3006".into();
        let http = Mock::new(FEES);
        Explorer::new(&c, &http).fee_estimates().unwrap();
        assert_eq!(http.count(), 1);
    }

    #[test]
    fn origins_compare_by_scheme_host_and_port() {
        assert_eq!(origin_of("https://Mempool.space/"), origin_of("https://mempool.space:443/api"));
        assert_ne!(origin_of("https://mempool.space"), origin_of("https://mempool.space:8443"));
        assert_ne!(origin_of("https://mempool.space"), origin_of("http://mempool.space"));
        assert_eq!(origin_of("not a url"), None);
    }

    /// R8: `reqwest::blocking` panics when it runs, or is dropped, on a thread that belongs to an
    /// async runtime, and the explorer commands are async. The real client must work from inside
    /// a runtime task, on success and on failure, and still report the failure.
    #[test]
    fn the_real_http_client_works_inside_an_async_task_and_does_not_panic() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut s = stream.unwrap();
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n845000");
            }
        });
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = closed.local_addr().unwrap().port();
        drop(closed); // nothing listens here: connection refused
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (ok, refused) = rt.block_on(async {
            tokio::spawn(async move {
                let ok = ReqwestHttp.get(&format!("http://127.0.0.1:{port}/x"), None);
                let refused = ReqwestHttp.get(&format!("http://127.0.0.1:{closed_port}/x"), Some("tok"));
                (ok, refused)
            })
            .await
            .expect("the task panicked")
        });
        assert_eq!(ok.unwrap(), "845000");
        let err = refused.unwrap_err();
        assert!(!err.is_empty() && !err.contains("tok"), "{err}");
    }
}
