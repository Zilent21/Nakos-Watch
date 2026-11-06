//! Normalizes user-supplied wallet sources (raw descriptors, xpub/ypub/zpub,
//! single addresses, multisig cosigner sets) into BDK-compatible descriptor
//! strings. Nakos never accepts or derives from private key material —
//! every path here is public-key-only (watch-only).

use bitcoin::bip32::{ChildNumber, Xpub};
use bitcoin::base58;
use bitcoin::Network;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DescriptorError {
    #[error("unrecognized extended public key prefix")]
    UnknownPrefix,
    #[error("base58check decode failed: {0}")]
    Base58(String),
    #[error("invalid extended public key: {0}")]
    InvalidXpub(String),
    #[error("invalid descriptor: {0}")]
    InvalidDescriptor(String),
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    #[error("multisig requires at least 2 cosigners")]
    MultisigTooFewCosigners,
    #[error("invalid signing origin: {0}")]
    InvalidOrigin(String),
    #[error("cosigner '{label}' is missing a master fingerprint and/or derivation path — both are required so Nakos can build a signable PSBT")]
    CosignerMissingOrigin { label: String },
}

/// Validate and normalize a `[fingerprint/path]` origin pair into the
/// descriptor-syntax prefix, e.g. `[73c5da0a/84'/0'/0']`. Never invents
/// either value — both must come from the caller (ultimately the user).
fn format_origin(fingerprint: &str, path: &str) -> Result<String, DescriptorError> {
    let fp = fingerprint.trim().to_lowercase();
    if fp.len() != 8 || !fp.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(DescriptorError::InvalidOrigin(format!(
            "fingerprint '{fingerprint}' must be exactly 8 hex characters"
        )));
    }
    let parsed: bitcoin::bip32::DerivationPath = path
        .trim()
        .parse()
        .map_err(|e| DescriptorError::InvalidOrigin(format!("derivation path '{path}' is invalid: {e}")))?;
    Ok(format!("[{fp}/{parsed}]"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    /// legacy P2PKH — xpub/tpub
    Legacy,
    /// P2SH-wrapped segwit — ypub/upub
    ShWpkh,
    /// native segwit — zpub/vpub
    Wpkh,
    /// P2SH-wrapped multisig — Ypub/Upub
    ShWshMulti,
    /// native segwit multisig — Zpub/Vpub
    WshMulti,
}

/// Standard version bytes rust-bitcoin's `Xpub` parser understands.
const MAINNET_XPUB_VERSION: [u8; 4] = [0x04, 0x88, 0xB2, 0x1E];
const TESTNET_TPUB_VERSION: [u8; 4] = [0x04, 0x35, 0x87, 0xCF];

fn version_bytes_and_kind(prefix4: &[u8; 4], mainnet: bool) -> Option<ScriptKind> {
    // SLIP-132 version byte table.
    let table_mainnet: &[([u8; 4], ScriptKind)] = &[
        ([0x04, 0x88, 0xB2, 0x1E], ScriptKind::Legacy),     // xpub
        ([0x04, 0x9D, 0x7C, 0xB2], ScriptKind::ShWpkh),     // ypub
        ([0x04, 0xB2, 0x47, 0x46], ScriptKind::Wpkh),       // zpub
        ([0x02, 0x95, 0xb4, 0x3f], ScriptKind::ShWshMulti), // Ypub
        ([0x02, 0xaa, 0x7e, 0xd3], ScriptKind::WshMulti),   // Zpub
    ];
    let table_testnet: &[([u8; 4], ScriptKind)] = &[
        ([0x04, 0x35, 0x87, 0xCF], ScriptKind::Legacy),     // tpub
        ([0x04, 0x4a, 0x52, 0x62], ScriptKind::ShWpkh),     // upub
        ([0x04, 0x5f, 0x1c, 0xf6], ScriptKind::Wpkh),       // vpub
        ([0x02, 0x42, 0x89, 0xef], ScriptKind::ShWshMulti), // Upub
        ([0x02, 0x57, 0x54, 0x83], ScriptKind::WshMulti),   // Vpub
    ];
    let table = if mainnet { table_mainnet } else { table_testnet };
    table.iter().find(|(v, _)| v == prefix4).map(|(_, k)| *k)
}

/// Decode any SLIP-132 extended public key (xpub/ypub/zpub/tpub/upub/vpub
/// and their multisig `Y/Z/U/V` counterparts) into a standard rust-bitcoin
/// `Xpub` plus the script kind it signals.
pub fn parse_extended_key(key: &str, network: Network) -> Result<(Xpub, ScriptKind), DescriptorError> {
    let mainnet = matches!(network, Network::Bitcoin);
    let mut data = base58::decode_check(key).map_err(|e| DescriptorError::Base58(e.to_string()))?;
    if data.len() < 4 {
        return Err(DescriptorError::UnknownPrefix);
    }
    let prefix: [u8; 4] = data[0..4].try_into().unwrap();
    let kind = version_bytes_and_kind(&prefix, mainnet).ok_or(DescriptorError::UnknownPrefix)?;

    let standard_version = if mainnet { MAINNET_XPUB_VERSION } else { TESTNET_TPUB_VERSION };
    data[0..4].copy_from_slice(&standard_version);
    let normalized = base58::encode_check(&data);

    let xpub = Xpub::from_str_std(&normalized).map_err(|e| DescriptorError::InvalidXpub(e.to_string()))?;
    Ok((xpub, kind))
}

trait XpubFromStdStr: Sized {
    fn from_str_std(s: &str) -> Result<Self, bitcoin::bip32::Error>;
}
impl XpubFromStdStr for Xpub {
    fn from_str_std(s: &str) -> Result<Self, bitcoin::bip32::Error> {
        use std::str::FromStr;
        Xpub::from_str(s)
    }
}

/// For a user-pasted full output descriptor: derive the change (`/1/*`)
/// sibling of a receive (`/0/*`) descriptor. The pasted string itself is
/// stored untouched — it already carries its own `[fp/path]`, so nothing is
/// wrapped or re-derived. Returns `None` if it isn't a plain `/0/*` receive
/// descriptor (e.g. multipath or already a change branch).
pub fn change_from_receive(descriptor: &str) -> Option<String> {
    let body = descriptor.split('#').next()?.trim();
    if body.matches("/0/*").count() == 0 || body.contains("<") {
        return None;
    }
    Some(body.replace("/0/*", "/1/*"))
}

fn purpose_kind(path: &str) -> Option<ScriptKind> {
    let first = path.trim().trim_start_matches("m/").split('/').next()?;
    match first.trim_end_matches(['\'', 'h', 'H']) {
        "44" => Some(ScriptKind::Legacy),
        "49" => Some(ScriptKind::ShWpkh),
        "84" => Some(ScriptKind::Wpkh),
        _ => None,
    }
}

/// External (receive) and change descriptor strings for a single-sig source.
#[derive(Debug)]
pub struct DescriptorPair {
    pub external: String,
    pub internal: String,
    pub script_kind: ScriptKind,
}

/// Build watch-only external/internal descriptors from a raw xpub/ypub/zpub.
///
/// `origin` is an optional `(master_fingerprint, derivation_path)` pair,
/// user-supplied and never invented. When present it is embedded into the
/// descriptor as a BIP-380 key origin (e.g. `[73c5da0a/84'/0'/0']xpub...`)
/// so a PSBT built against this wallet can carry `bip32_derivation` and be
/// signed by a real hardware wallet. When absent, the descriptor is still
/// valid for watch-only tracking, but `bitcoin::psbt::build_unsigned_psbt`
/// will refuse to build a spend from it.
pub fn descriptors_from_extended_key(
    key: &str,
    network: Network,
    origin: Option<(&str, &str)>,
) -> Result<DescriptorPair, DescriptorError> {
    let (xpub, prefix_kind) = parse_extended_key(key, network)?;
    let xpub_str = xpub.to_string();
    let origin_prefix = match origin {
        Some((fp, path)) => format_origin(fp, path)?,
        None => String::new(),
    };

    // A plain `xpub`/`tpub` prefix says nothing about script type (SLIP-132
    // only encodes it for y/z/u/v keys), and Sparrow & co. routinely hand out
    // BIP-84 account keys as `xpub`. When the user supplied an origin, its
    // BIP-43 purpose (44'/49'/84') is the authoritative script type; without
    // it a bare `xpub` remains legacy P2PKH. A prefix that contradicts the
    // purpose is rejected rather than silently scanning the wrong addresses.
    let kind = match origin.and_then(|(_, path)| purpose_kind(path)) {
        Some(purpose) if prefix_kind == ScriptKind::Legacy => purpose,
        Some(purpose) if purpose != prefix_kind => {
            return Err(DescriptorError::InvalidOrigin(format!(
                "key prefix implies {prefix_kind:?} but derivation path purpose implies {purpose:?}"
            )))
        }
        _ => prefix_kind,
    };

    let (external, internal) = match kind {
        ScriptKind::Legacy => (
            format!("pkh({origin_prefix}{xpub_str}/0/*)"),
            format!("pkh({origin_prefix}{xpub_str}/1/*)"),
        ),
        ScriptKind::ShWpkh => (
            format!("sh(wpkh({origin_prefix}{xpub_str}/0/*))"),
            format!("sh(wpkh({origin_prefix}{xpub_str}/1/*))"),
        ),
        ScriptKind::Wpkh => (
            format!("wpkh({origin_prefix}{xpub_str}/0/*)"),
            format!("wpkh({origin_prefix}{xpub_str}/1/*)"),
        ),
        ScriptKind::ShWshMulti | ScriptKind::WshMulti => {
            return Err(DescriptorError::InvalidDescriptor(
                "multisig extended keys must be supplied via the multisig wallet flow".into(),
            ))
        }
    };

    Ok(DescriptorPair {
        external,
        internal,
        script_kind: kind,
    })
}

/// Build a sorted-multisig descriptor pair from cosigner keys.
/// `script_type` selects p2wsh (native segwit, recommended), p2sh-p2wsh, or p2tr (musig-style not supported watch-only, falls back to wsh).
///
/// Every cosigner **must** carry a master fingerprint and derivation path —
/// this is enforced here (not just at the UI layer) because a multisig
/// wallet's entire purpose in Nakos is producing a signable PSBT later, and
/// a cosigner key with no recorded origin can never appear correctly in
/// `bip32_derivation`. Nothing here invents a fingerprint or path.
pub fn descriptors_from_multisig(
    threshold: u8,
    cosigners: &[crate::db::models::Cosigner],
    network: Network,
    script_type: &str,
) -> Result<DescriptorPair, DescriptorError> {
    if cosigners.len() < 2 {
        return Err(DescriptorError::MultisigTooFewCosigners);
    }

    let mut ext_keys = Vec::new();
    let mut int_keys = Vec::new();
    for c in cosigners {
        let fingerprint = c.fingerprint.as_deref().filter(|s| !s.trim().is_empty());
        let path = c.derivation_path.as_deref().filter(|s| !s.trim().is_empty());
        let (fingerprint, path) = match (fingerprint, path) {
            (Some(fp), Some(p)) => (fp, p),
            _ => return Err(DescriptorError::CosignerMissingOrigin { label: c.label.clone() }),
        };
        let origin_prefix = format_origin(fingerprint, path)?;

        let (xpub, _kind) = parse_extended_key(&c.xpub, network)?;
        let s = xpub.to_string();
        ext_keys.push(format!("{origin_prefix}{s}/0/*"));
        int_keys.push(format!("{origin_prefix}{s}/1/*"));
    }

    let ext_inner = format!("sortedmulti({threshold},{})", ext_keys.join(","));
    let int_inner = format!("sortedmulti({threshold},{})", int_keys.join(","));

    let (external, internal, kind) = match script_type {
        "p2sh-p2wsh" => (
            format!("sh(wsh({ext_inner}))"),
            format!("sh(wsh({int_inner}))"),
            ScriptKind::ShWshMulti,
        ),
        _ => (
            format!("wsh({ext_inner})"),
            format!("wsh({int_inner})"),
            ScriptKind::WshMulti,
        ),
    };

    Ok(DescriptorPair { external, internal, script_kind: kind })
}

/// Validate a raw address string against the target network.
pub fn validate_address(addr: &str, network: Network) -> Result<bitcoin::Address, DescriptorError> {
    let unchecked: bitcoin::Address<bitcoin::address::NetworkUnchecked> =
        addr.parse().map_err(|e: bitcoin::address::ParseError| DescriptorError::InvalidAddress(e.to_string()))?;
    unchecked
        .require_network(network)
        .map_err(|e| DescriptorError::InvalidAddress(e.to_string()))
}

/// Best-effort detection of the source kind from a raw pasted string, used
/// by the "Add Wallet" UI to pick the right form before the user confirms.
pub fn sniff_source_type(input: &str) -> &'static str {
    let trimmed = input.trim();
    if trimmed.starts_with("[") || trimmed.contains('(') {
        return "descriptor";
    }
    // `get` returns None when byte 4 falls inside a multibyte character (or the text is shorter
    // than 4 bytes); indexing with `[..4]` would panic there and abort the app.
    match trimmed.get(..4).unwrap_or("") {
        "xpub" | "tpub" => "xpub",
        "ypub" | "upub" => "ypub",
        "zpub" | "vpub" => "zpub",
        _ => {
            if trimmed.starts_with('1')
                || trimmed.starts_with('3')
                || trimmed.starts_with("bc1")
                || trimmed.starts_with("tb1")
                || trimmed.starts_with('m')
                || trimmed.starts_with('n')
                || trimmed.starts_with('2')
            {
                "address"
            } else {
                "unknown"
            }
        }
    }
}

fn _child(n: u32) -> ChildNumber {
    ChildNumber::from_normal_idx(n).expect("valid index")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bdk_wallet::miniscript::{Descriptor, DescriptorPublicKey};
    use bitcoin::secp256k1::Secp256k1;

    // BIP-84 published test vector (mnemonic "abandon ... about"), account 0.
    const BIP84_ACCOUNT_ZPUB: &str = "zpub6rFR7y4Q2AijBEqTUquhVz398htDFrtymD9xYYfG1m4wAcvPhXNfE3EfH1r1ADqtfSdVCToUG868RvUUkgDKf31mGDtKsAYz2oz2AGutZYs";
    const BIP84_FINGERPRINT: &str = "73c5da0a";
    const BIP84_PATH: &str = "84'/0'/0'";
    const BIP84_RECEIVE_0: &str = "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu";
    const BIP84_CHANGE_0: &str = "bc1q8c6fshw2dlwun7ekn9qwf37cu2rn755upcp6el";

    /// Same key material, re-encoded with the plain `xpub` version bytes —
    /// this is how Sparrow / many tools hand out a BIP-84 account key.
    fn as_xpub(zpub: &str) -> String {
        let mut data = base58::decode_check(zpub).unwrap();
        data[0..4].copy_from_slice(&MAINNET_XPUB_VERSION);
        base58::encode_check(&data)
    }

    fn address_at(desc: &str, index: u32) -> String {
        let secp = Secp256k1::new();
        let (d, _) = Descriptor::<DescriptorPublicKey>::parse_descriptor(&secp, desc).unwrap();
        d.at_derivation_index(index).unwrap().address(Network::Bitcoin).unwrap().to_string()
    }

    #[test]
    fn zpub_with_origin_matches_bip84_vector_and_path_is_not_doubled() {
        let pair =
            descriptors_from_extended_key(BIP84_ACCOUNT_ZPUB, Network::Bitcoin, Some((BIP84_FINGERPRINT, BIP84_PATH)))
                .unwrap();
        println!("zpub external: {}\nzpub internal: {}", pair.external, pair.internal);
        assert_eq!(address_at(&pair.external, 0), BIP84_RECEIVE_0);
        assert_eq!(address_at(&pair.internal, 0), BIP84_CHANGE_0);
        assert_eq!(pair.external.matches("84'").count(), 1, "path must appear exactly once");
        assert!(pair.external.starts_with("wpkh([73c5da0a/84'/0'/0']"));
        assert!(pair.external.ends_with("/0/*)") && pair.internal.ends_with("/1/*)"));
    }

    #[test]
    fn xpub_prefixed_bip84_account_with_origin_is_wpkh_not_pkh() {
        let xpub = as_xpub(BIP84_ACCOUNT_ZPUB);
        assert!(xpub.starts_with("xpub"));
        let pair =
            descriptors_from_extended_key(&xpub, Network::Bitcoin, Some((BIP84_FINGERPRINT, BIP84_PATH))).unwrap();
        println!("xpub external: {}\nxpub internal: {}", pair.external, pair.internal);
        assert_eq!(address_at(&pair.external, 0), BIP84_RECEIVE_0);
        assert_eq!(address_at(&pair.internal, 0), BIP84_CHANGE_0);
    }

    #[test]
    fn pasted_full_descriptor_is_not_rewrapped_and_gets_change_sibling() {
        let pair = descriptors_from_extended_key(BIP84_ACCOUNT_ZPUB, Network::Bitcoin, Some((BIP84_FINGERPRINT, BIP84_PATH))).unwrap();
        let pasted = format!("{}#abcdefgh", pair.external);
        let change = change_from_receive(&pasted).unwrap();
        assert_eq!(change, pair.internal, "change must equal the generated /1/* descriptor, origin untouched");
        assert_eq!(change.matches('[').count(), 1, "origin must not be wrapped a second time");
        assert_eq!(address_at(&change, 0), BIP84_CHANGE_0);
        assert!(change_from_receive("wpkh([aa/84h/0h/0h]xpub/<0;1>/*)").is_none());
    }

    /// Pasted text can be anything: a character that straddles byte 4 must not panic (it used
    /// to, and a panic in this command aborts the whole app).
    #[test]
    fn sniffing_never_panics_on_multibyte_text_and_still_detects_real_keys() {
        for text in ["a😀bc", "日本語日", "€€€€", "é", "ééé", "aé", "aaé€", "😀😀", "  日本語  ", "\u{200b}\u{200b}x", "xpu\u{e9}", ""] {
            assert!(["unknown", "address", "descriptor", "xpub", "ypub", "zpub"].contains(&sniff_source_type(text)), "{text:?}");
        }
        assert_eq!(sniff_source_type("日本語日"), "unknown");
        assert_eq!(sniff_source_type("a😀bc"), "unknown");
        // Real input is unaffected.
        assert_eq!(sniff_source_type("xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8"), "xpub");
        assert_eq!(sniff_source_type("  zpub6abc "), "zpub");
        assert_eq!(sniff_source_type("ypub6abc"), "ypub");
        assert_eq!(sniff_source_type("tpubD6abc"), "xpub");
        assert_eq!(sniff_source_type("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"), "address");
        assert_eq!(sniff_source_type("[73c5da0a/84'/0'/0']xpub6abc/0/*"), "descriptor");
        assert_eq!(sniff_source_type("wpkh(xpub6abc/0/*)"), "descriptor");
        assert_eq!(sniff_source_type("xp"), "unknown");
    }
}
