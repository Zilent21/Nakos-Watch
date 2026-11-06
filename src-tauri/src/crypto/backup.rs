//! Encrypted vault backup: one self-contained file the owner can restore on a
//! fresh install with only the vault passphrase. Not the family letter and
//! not the advanced packet; those are watch-only exports meant for other
//! people. See docs/BACKUP.md.
//!
//! File layout (all integers big-endian):
//!
//!   "NAKOSBK2"            8 bytes   magic / format version
//!   header_len            4 bytes   length of the JSON header
//!   header                JSON      { format, created_at, app_version, kdf{...} }
//!   nonce                12 bytes   random, per file
//!   ciphertext + tag      rest      AES-256-GCM of the raw SQLite file
//!
//! The AEAD key is the vault master key: Argon2id(passphrase, salt) using the
//! KDF parameters stored in the header. There is no separate backup key. The
//! magic, length and header are bound as associated data, so editing any of
//! them makes the file fail to open. The database inside already holds RPC
//! passwords and the explorer token sealed under that same key, so none of
//! them appears in plaintext even after the outer layer is removed.

use super::{derive_key, CryptoError, KdfParams, MasterKey, NONCE_LEN};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAGIC_V2: &[u8; 8] = b"NAKOSBK2";
/// Older export format: nonce + ciphertext under the *current vault's* key,
/// with no KDF parameters in the file.
pub const MAGIC_V1: &[u8; 8] = b"NAKOSBK1";

const MAX_HEADER_LEN: usize = 4096;
// A restore is user-initiated, but the KDF cost comes from the file, so cap it.
const MAX_MEM_KIB: u32 = 512 * 1024;
const MAX_TIME_COST: u32 = 10;
const MAX_PARALLELISM: u32 = 8;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("this looks like a family letter or packet, not an encrypted backup")]
    LooksLikeLetter,
    #[error("not a Nakos encrypted backup")]
    NotABackup,
    #[error("this backup uses an older format; unlock the vault it came from, then restore it from Settings")]
    OlderFormat,
    #[error("unsupported backup settings in this file")]
    UnsupportedKdf,
    #[error("wrong passphrase, or the backup file is damaged")]
    WrongPassphraseOrDamaged,
    #[error("the backup contains extended private key material and was refused")]
    ContainsKeyMaterial,
    #[error("the backup doesn't contain a valid Nakos database")]
    NotADatabase,
    #[error("the backup's vault settings don't match its contents")]
    InconsistentVault,
    #[error("{0}")]
    Crypto(#[from] CryptoError),
    #[error("{0}")]
    Json(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupKdf {
    pub alg: String,
    pub version: u32,
    pub salt_b64: String,
    pub mem_cost_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupHeader {
    pub format: u32,
    pub created_at: String,
    pub app_version: String,
    pub kdf: BackupKdf,
}

/// Seal `db_bytes` (a raw SQLite file) under `key`, recording the vault's KDF
/// parameters so the file can be opened without the original machine.
pub fn seal(db_bytes: &[u8], key: &MasterKey, kdf: &KdfParams, created_at: &str, app_version: &str) -> Result<Vec<u8>, BackupError> {
    reject_key_material(db_bytes)?;
    let header = BackupHeader {
        format: 2,
        created_at: created_at.to_string(),
        app_version: app_version.to_string(),
        kdf: BackupKdf {
            alg: "argon2id".into(),
            version: 19,
            salt_b64: kdf.salt_b64.clone(),
            mem_cost_kib: kdf.mem_cost_kib,
            time_cost: kdf.time_cost,
            parallelism: kdf.parallelism,
        },
    };
    let header_json = serde_json::to_vec(&header).map_err(|e| BackupError::Json(e.to_string()))?;
    let mut prefix = Vec::with_capacity(12 + header_json.len());
    prefix.extend_from_slice(MAGIC_V2);
    prefix.extend_from_slice(&(header_json.len() as u32).to_be_bytes());
    prefix.extend_from_slice(&header_json);

    let (nonce, ct) = key.encrypt_aad(db_bytes, &prefix)?;
    let mut out = prefix;
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Quick, content-based check for the exports meant for other people.
pub fn looks_like_letter(bytes: &[u8]) -> bool {
    if bytes.starts_with(MAGIC_V2) || bytes.starts_with(MAGIC_V1) {
        return false;
    }
    let head = &bytes[..bytes.len().min(64 * 1024)];
    let Ok(text) = std::str::from_utf8(head) else { return false };
    let t = text.trim_start();
    t.starts_with('#') || t.starts_with('{') || text.contains("nakos-inherit-json") || text.contains("watch_only_notice")
}

/// What the backup needs from a restore: the raw database and the derived vault key.
pub struct Opened {
    pub db_bytes: Vec<u8>,
    pub key: MasterKey,
    pub header: BackupHeader,
}

/// Parse and open a V2 backup with the vault passphrase.
pub fn open(file: &[u8], passphrase: &str) -> Result<Opened, BackupError> {
    if looks_like_letter(file) {
        return Err(BackupError::LooksLikeLetter);
    }
    if file.starts_with(MAGIC_V1) {
        return Err(BackupError::OlderFormat);
    }
    if file.len() < 12 + NONCE_LEN || !file.starts_with(MAGIC_V2) {
        return Err(BackupError::NotABackup);
    }
    let header_len = u32::from_be_bytes(file[8..12].try_into().unwrap()) as usize;
    if header_len == 0 || header_len > MAX_HEADER_LEN || file.len() < 12 + header_len + NONCE_LEN {
        return Err(BackupError::NotABackup);
    }
    let prefix = &file[..12 + header_len];
    let header: BackupHeader = serde_json::from_slice(&file[12..12 + header_len]).map_err(|e| BackupError::Json(e.to_string()))?;
    let k = &header.kdf;
    if header.format != 2
        || k.alg != "argon2id"
        || k.version != 19
        || k.mem_cost_kib == 0
        || k.mem_cost_kib > MAX_MEM_KIB
        || k.time_cost == 0
        || k.time_cost > MAX_TIME_COST
        || k.parallelism == 0
        || k.parallelism > MAX_PARALLELISM
    {
        return Err(BackupError::UnsupportedKdf);
    }
    let params = KdfParams {
        salt_b64: k.salt_b64.clone(),
        mem_cost_kib: k.mem_cost_kib,
        time_cost: k.time_cost,
        parallelism: k.parallelism,
        canary_nonce_b64: String::new(),
        canary_ct_b64: String::new(),
    };
    let key = derive_key(passphrase, &params)?;
    let nonce = &file[12 + header_len..12 + header_len + NONCE_LEN];
    let ct = &file[12 + header_len + NONCE_LEN..];
    let db_bytes = key.decrypt_aad(nonce, ct, prefix).map_err(|_| BackupError::WrongPassphraseOrDamaged)?;
    if db_bytes.len() < 16 || &db_bytes[..16] != b"SQLite format 3\0" {
        return Err(BackupError::NotADatabase);
    }
    reject_key_material(&db_bytes)?;
    Ok(Opened { db_bytes, key, header })
}

/// True if `bytes` carry an extended private key followed by base58 text: the SLIP-132
/// private versions, mainnet `xprv`/`yprv`/`zprv` and multisig `Yprv`/`Zprv`, testnet
/// `tprv`/`uprv`/`vprv` and multisig `Uprv`/`Vprv` (the same ten kinds `bitcoin::descriptors`
/// accepts as public keys). The one prefix list and scan: used by backup sealing/opening and by
/// the wallet write paths.
pub fn contains_extended_private_key(bytes: &[u8]) -> bool {
    const PREFIXES: [&[u8]; 10] = [
        b"xprv", b"yprv", b"zprv", b"Yprv", b"Zprv", b"tprv", b"uprv", b"vprv", b"Uprv", b"Vprv",
    ];
    let is_b58 = |b: u8| b.is_ascii_alphanumeric() && !matches!(b, b'0' | b'O' | b'I' | b'l');
    for w in 0..bytes.len().saturating_sub(24) {
        let head = &bytes[w..w + 4];
        if PREFIXES.contains(&head) && bytes[w + 4..w + 24].iter().all(|b| is_b58(*b)) {
            return true;
        }
    }
    false
}

/// Refuse any database that carries an extended private key. Nakos never
/// stores one, so finding one means the file didn't come from Nakos or
/// something went badly wrong.
pub fn reject_key_material(db_bytes: &[u8]) -> Result<(), BackupError> {
    if contains_extended_private_key(db_bytes) {
        return Err(BackupError::ContainsKeyMaterial);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::init_passphrase;

    fn fake_db(extra: &[u8]) -> Vec<u8> {
        let mut v = b"SQLite format 3\0".to_vec();
        v.extend_from_slice(&[0u8; 100]);
        v.extend_from_slice(extra);
        v
    }

    #[test]
    fn round_trip_needs_only_the_passphrase() {
        let (kdf, key) = init_passphrase("correct horse battery").unwrap();
        let db = fake_db(b"wallet rows");
        let file = seal(&db, &key, &kdf, "2026-09-30T00:00:00Z", "0.1.0").unwrap();
        assert!(file.starts_with(MAGIC_V2));
        assert!(!file.windows(11).any(|w| w == b"wallet rows"), "database content must not appear in the file");
        let opened = open(&file, "correct horse battery").unwrap();
        assert_eq!(opened.db_bytes, db);
        assert_eq!(opened.header.kdf.salt_b64, kdf.salt_b64);
    }

    #[test]
    fn wrong_passphrase_and_tampering_fail_with_one_message() {
        let (kdf, key) = init_passphrase("correct horse battery").unwrap();
        let file = seal(&fake_db(b"x"), &key, &kdf, "t", "0.1.0").unwrap();
        assert!(matches!(open(&file, "wrong passphrase"), Err(BackupError::WrongPassphraseOrDamaged)));
        // Flip one bit of the header: the AAD binding must reject it.
        let mut tampered = file.clone();
        tampered[20] ^= 1;
        assert!(open(&tampered, "correct horse battery").is_err());
        // Flip one bit of the ciphertext.
        let mut damaged = file.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        assert!(matches!(open(&damaged, "correct horse battery"), Err(BackupError::WrongPassphraseOrDamaged)));
    }

    #[test]
    fn family_letters_and_packets_are_refused() {
        let letter = b"# A letter about my bitcoin\n\nSome words.\n\n```nakos-inherit-json\n{}\n```\n";
        assert!(matches!(open(letter, "x"), Err(BackupError::LooksLikeLetter)));
        let packet = br#"{"format_version":2,"watch_only_notice":"x","wallets":[]}"#;
        assert!(matches!(open(packet, "x"), Err(BackupError::LooksLikeLetter)));
        assert!(matches!(open(b"random bytes that are not a backup at all", "x"), Err(BackupError::NotABackup)));
    }

    /// A real 111-character extended private key for each SLIP-132 private version, built by
    /// re-encoding the BIP-32 test-vector key with that version, plus the matching public
    /// version of the public test-vector key. The version bytes are the SLIP-132 values.
    fn slip132_keys() -> Vec<(&'static str, String, String)> {
        const XPRV: &str = "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi";
        const XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
        let with_version = |key: &str, version: [u8; 4]| {
            let mut data = bitcoin::base58::decode_check(key).unwrap();
            data[..4].copy_from_slice(&version);
            bitcoin::base58::encode_check(&data)
        };
        [
            ("xprv", [0x04, 0x88, 0xAD, 0xE4], [0x04, 0x88, 0xB2, 0x1E]),
            ("yprv", [0x04, 0x9D, 0x78, 0x78], [0x04, 0x9D, 0x7C, 0xB2]),
            ("zprv", [0x04, 0xB2, 0x43, 0x0C], [0x04, 0xB2, 0x47, 0x46]),
            ("Yprv", [0x02, 0x95, 0xB0, 0x05], [0x02, 0x95, 0xB4, 0x3F]),
            ("Zprv", [0x02, 0xAA, 0x7A, 0x99], [0x02, 0xAA, 0x7E, 0xD3]),
            ("tprv", [0x04, 0x35, 0x83, 0x94], [0x04, 0x35, 0x87, 0xCF]),
            ("uprv", [0x04, 0x4A, 0x4E, 0x28], [0x04, 0x4A, 0x52, 0x62]),
            ("vprv", [0x04, 0x5F, 0x18, 0xBC], [0x04, 0x5F, 0x1C, 0xF6]),
            ("Uprv", [0x02, 0x42, 0x85, 0xB5], [0x02, 0x42, 0x89, 0xEF]),
            ("Vprv", [0x02, 0x57, 0x50, 0x48], [0x02, 0x57, 0x54, 0x83]),
        ]
        .into_iter()
        .map(|(name, prv, pubv)| (name, with_version(XPRV, prv), with_version(XPUB, pubv)))
        .collect()
    }

    #[test]
    fn every_slip132_private_version_is_recognized_and_public_ones_are_not() {
        for (name, prv, public) in slip132_keys() {
            assert!(prv.starts_with(name) && prv.len() == 111, "{name}: test key is {prv}");
            assert!(contains_extended_private_key(prv.as_bytes()), "{name} private key not recognized");
            let in_descriptor = format!("wsh(sortedmulti(2,[aabbccdd/48'/0'/0'/2']{prv}/0/*))");
            assert!(contains_extended_private_key(in_descriptor.as_bytes()), "{name} inside a descriptor");
            assert!(!contains_extended_private_key(public.as_bytes()), "public key for {name} was flagged: {public}");
        }
    }

    #[test]
    fn a_database_holding_a_capitalized_private_key_is_not_sealed() {
        let (kdf, key) = init_passphrase("correct horse battery").unwrap();
        for (name, prv, _) in slip132_keys().into_iter().filter(|(n, _, _)| n.starts_with(char::is_uppercase)) {
            let db = fake_db(format!("wsh(sortedmulti(2,{prv}/0/*))").as_bytes());
            assert!(matches!(seal(&db, &key, &kdf, "t", "v"), Err(BackupError::ContainsKeyMaterial)), "{name}");
        }
    }

    #[test]
    fn databases_with_extended_private_keys_are_refused_both_ways() {
        let (kdf, key) = init_passphrase("correct horse battery").unwrap();
        let bad = fake_db(b"xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi");
        assert!(matches!(seal(&bad, &key, &kdf, "t", "v"), Err(BackupError::ContainsKeyMaterial)));
        // A hand-built file that bypasses `seal` is refused on open as well.
        let header = BackupHeader { format: 2, created_at: "t".into(), app_version: "v".into(), kdf: BackupKdf { alg: "argon2id".into(), version: 19, salt_b64: kdf.salt_b64.clone(), mem_cost_kib: kdf.mem_cost_kib, time_cost: kdf.time_cost, parallelism: kdf.parallelism } };
        let hj = serde_json::to_vec(&header).unwrap();
        let mut prefix = MAGIC_V2.to_vec();
        prefix.extend_from_slice(&(hj.len() as u32).to_be_bytes());
        prefix.extend_from_slice(&hj);
        let (nonce, ct) = key.encrypt_aad(&bad, &prefix).unwrap();
        let mut file = prefix;
        file.extend_from_slice(&nonce);
        file.extend_from_slice(&ct);
        assert!(matches!(open(&file, "correct horse battery"), Err(BackupError::ContainsKeyMaterial)));
        // Ordinary xpubs are fine.
        assert!(reject_key_material(&fake_db(b"xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8")).is_ok());
    }

    #[test]
    fn absurd_kdf_costs_in_a_file_are_rejected_before_any_work() {
        let (kdf, key) = init_passphrase("correct horse battery").unwrap();
        let mut bytes = seal(&fake_db(b"x"), &key, &kdf, "t", "v").unwrap();
        // Rewrite the header with an enormous memory cost (same length class of JSON).
        let header_len = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let mut header: serde_json::Value = serde_json::from_slice(&bytes[12..12 + header_len]).unwrap();
        header["kdf"]["mem_cost_kib"] = serde_json::json!(4_000_000_000u64);
        let hj = serde_json::to_vec(&header).unwrap();
        let mut forged = MAGIC_V2.to_vec();
        forged.extend_from_slice(&(hj.len() as u32).to_be_bytes());
        forged.extend_from_slice(&hj);
        forged.extend_from_slice(&bytes.split_off(12 + header_len));
        assert!(matches!(open(&forged, "correct horse battery"), Err(BackupError::UnsupportedKdf)));
    }
}
