//! Local encryption-at-rest for sensitive fields (inheritance instructions,
//! RPC credentials, recovery contact notes). The master key never leaves
//! memory unencrypted and is never written to disk. It is derived from a
//! user passphrase via Argon2id, or optionally cached in the OS keychain
//! if the user opts in to "remember on this device".

use aes_gcm::aead::{Aead, KeyInit, OsRng as AesOsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::password_hash::SaltString;
use argon2::{Argon2, Block, Params};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub mod backup;

pub const NONCE_LEN: usize = 12;
pub const KEY_LEN: usize = 32;
const KEYRING_SERVICE: &str = "com.nakos.app";
const KEYRING_ACCOUNT: &str = "master-key";

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("vault is locked")]
    Locked,
    #[error("incorrect passphrase")]
    WrongPassphrase,
    #[error("encryption failure")]
    Encrypt,
    #[error("decryption failure (data may be corrupt or key is wrong)")]
    Decrypt,
    #[error("keychain error: {0}")]
    Keyring(String),
    #[error("kdf error: {0}")]
    Kdf(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfParams {
    pub salt_b64: String,
    pub mem_cost_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
    /// Base64 of a value encrypted with the derived key; used to verify a
    /// passphrase attempt without ever storing the passphrase or raw key.
    pub canary_nonce_b64: String,
    pub canary_ct_b64: String,
}

#[derive(ZeroizeOnDrop)]
pub struct MasterKey(#[zeroize(drop)] pub [u8; KEY_LEN]);

impl MasterKey {
    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.0))
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        AesOsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher()
            .encrypt(nonce, plaintext)
            .map_err(|_| CryptoError::Encrypt)?;
        Ok((nonce_bytes.to_vec(), ct))
    }

    /// AES-256-GCM with associated data: `aad` is authenticated but not
    /// encrypted, so tampering with it makes decryption fail.
    pub fn encrypt_aad(&self, plaintext: &[u8], aad: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        AesOsRng.fill_bytes(&mut nonce_bytes);
        let ct = self
            .cipher()
            .encrypt(Nonce::from_slice(&nonce_bytes), Payload { msg: plaintext, aad })
            .map_err(|_| CryptoError::Encrypt)?;
        Ok((nonce_bytes.to_vec(), ct))
    }

    pub fn decrypt_aad(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if nonce.len() != NONCE_LEN {
            return Err(CryptoError::Decrypt);
        }
        self.cipher()
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ciphertext, aad })
            .map_err(|_| CryptoError::Decrypt)
    }

    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if nonce.len() != NONCE_LEN {
            return Err(CryptoError::Decrypt);
        }
        let nonce = Nonce::from_slice(nonce);
        self.cipher()
            .decrypt(nonce, ciphertext)
            .map_err(|_| CryptoError::Decrypt)
    }
}

/// Derive a master key from a passphrase and existing KDF params.
pub fn derive_key(passphrase: &str, params: &KdfParams) -> Result<MasterKey, CryptoError> {
    // The key is written straight into its final home, so there is no second copy of it on the
    // stack, and no PHC hash object that holds it. The Argon2 working memory is ours, so it can
    // be wiped (its last block is what the key is hashed from).
    let mut key = MasterKey([0u8; KEY_LEN]);
    let mut blocks: Vec<Block> = Vec::new();
    derive_into(passphrase, params, &mut key.0, &mut blocks)?;
    Ok(key)
}

/// Argon2id into `out`, using `blocks` as the working memory and wiping all of it (the whole
/// capacity) before returning, on success and on failure. Same salt decoding and parameters as the
/// PHC-string path that `derive_key` used before, so existing vaults derive the same key
/// (pinned by `derive_key_is_byte_for_byte_what_it_always_was`).
fn derive_into(passphrase: &str, params: &KdfParams, out: &mut [u8; KEY_LEN], blocks: &mut Vec<Block>) -> Result<(), CryptoError> {
    let salt = SaltString::from_b64(&params.salt_b64).map_err(|e| CryptoError::Kdf(e.to_string()))?;
    let mut salt_buf = [0u8; 64];
    let salt_bytes = salt.decode_b64(&mut salt_buf).map_err(|e| CryptoError::Kdf(e.to_string()))?;
    let argon2_params = Params::new(
        params.mem_cost_kib,
        params.time_cost,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|e| CryptoError::Kdf(e.to_string()))?;
    blocks.resize(argon2_params.block_count(), Block::default());
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, argon2_params);

    let result = argon2.hash_password_into_with_memory(passphrase.as_bytes(), salt_bytes, out, &mut blocks[..]);
    blocks.zeroize();
    result.map_err(|e| CryptoError::Kdf(e.to_string()))
}

/// Create fresh KDF params + canary for a brand-new passphrase (first run).
pub fn init_passphrase(passphrase: &str) -> Result<(KdfParams, MasterKey), CryptoError> {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    // KDF: Argon2id v1.3, 32-byte output used directly as the AES-256-GCM
    // vault key. Cost: 19 MiB memory, 2 passes, 1 lane, which is the OWASP
    // minimum-tier Argon2id configuration. The 16-byte random salt and these
    // three numbers are stored per vault (db_meta `kdf_params`), so a vault
    // always opens with the parameters it was created with; raising the cost
    // for new vaults later does not affect existing ones. Documented in
    // docs/THREAT_MODEL.md.
    let mem_cost_kib: u32 = 19 * 1024;
    let time_cost: u32 = 2;
    let parallelism: u32 = 1;

    let mut params = KdfParams {
        salt_b64: salt.as_str().to_string(),
        mem_cost_kib,
        time_cost,
        parallelism,
        canary_nonce_b64: String::new(),
        canary_ct_b64: String::new(),
    };

    let key = derive_key(passphrase, &params)?;
    let (nonce, ct) = key.encrypt(b"nakos-vault-canary")?;
    params.canary_nonce_b64 = base64_encode(&nonce);
    params.canary_ct_b64 = base64_encode(&ct);

    Ok((params, key))
}

/// Verify a passphrase attempt against stored KDF params + canary, returning
/// the derived master key on success.
pub fn unlock(passphrase: &str, params: &KdfParams) -> Result<MasterKey, CryptoError> {
    let key = derive_key(passphrase, params)?;
    let nonce = base64_decode(&params.canary_nonce_b64)?;
    let ct = base64_decode(&params.canary_ct_b64)?;
    match key.decrypt(&nonce, &ct) {
        Ok(pt) if pt == b"nakos-vault-canary" => Ok(key),
        _ => Err(CryptoError::WrongPassphrase),
    }
}

// -- Optional OS keychain caching (opt-in "remember on this device") -----
//
// The keychain item holds the vault passphrase, purely as a convenience
// cache: the vault key is always derived from the passphrase, so a missing,
// denied or stale keychain can never block unlock. Nothing here is touched
// unless the user opted in (see `KEYCHAIN_OPT_IN_META` in commands::backup),
// and debug / `tauri dev` builds skip it entirely so every rebuild (new binary,
// new keychain ACL) doesn't raise an OS password prompt.

/// Whether this build may use the OS keychain at all. Release builds: yes.
/// Debug builds: no, unless `NAKOS_KEYCHAIN=1` is set for deliberate testing.
/// Android: never (see `has_os_keychain`).
pub fn keychain_enabled() -> bool {
    keychain_enabled_for(
        cfg!(debug_assertions),
        std::env::var("NAKOS_KEYCHAIN").ok().as_deref(),
        has_os_keychain(std::env::consts::OS),
    )
}

pub fn keychain_enabled_for(debug_build: bool, env_override: Option<&str>, os_keychain: bool) -> bool {
    os_keychain && (!debug_build || env_override == Some("1"))
}

/// The keyring crate has no Android backend and silently falls back to an in-memory mock, so a
/// "remembered" passphrase would be gone at the next start. Android gets no keychain helper.
pub fn has_os_keychain(os: &str) -> bool {
    os != "android"
}


pub fn keychain_store_passphrase(passphrase: &str) -> Result<(), CryptoError> {
    keychain_store_in(KEYRING_SERVICE, passphrase)
}

pub fn keychain_load_passphrase() -> Result<Option<zeroize::Zeroizing<String>>, CryptoError> {
    keychain_load_from(KEYRING_SERVICE)
}

pub fn keychain_clear() -> Result<(), CryptoError> {
    keychain_clear_in(KEYRING_SERVICE)
}

// The item is `service` / `KEYRING_ACCOUNT`; tests pass a throwaway service so they never touch the
// real one. On Windows it is a generic credential in Credential Manager named
// "master-key.<service>" (keyring's naming), holding the passphrase as UTF-16.
fn keychain_entry(service: &str) -> Result<keyring::Entry, CryptoError> {
    keyring::Entry::new(service, KEYRING_ACCOUNT).map_err(|e| CryptoError::Keyring(e.to_string()))
}

fn keychain_store_in(service: &str, passphrase: &str) -> Result<(), CryptoError> {
    keychain_entry(service)?
        .set_password(passphrase)
        .map_err(|e| CryptoError::Keyring(e.to_string()))
}

fn keychain_load_from(service: &str) -> Result<Option<zeroize::Zeroizing<String>>, CryptoError> {
    match keychain_entry(service)?.get_password() {
        Ok(p) => Ok(Some(zeroize::Zeroizing::new(p))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CryptoError::Keyring(e.to_string())),
    }
}

fn keychain_clear_in(service: &str) -> Result<(), CryptoError> {
    match keychain_entry(service)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(CryptoError::Keyring(e.to_string())),
    }
}

// -- tiny base64 helpers (avoid pulling a whole extra crate surface) -----

fn base64_encode(bytes: &[u8]) -> String {
    use base64_lite::Engine;
    base64_lite::STANDARD.encode(bytes)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, CryptoError> {
    use base64_lite::Engine;
    base64_lite::STANDARD
        .decode(s)
        .map_err(|_| CryptoError::Decrypt)
}

/// Minimal base64 shim over the `bitcoin` crate's re-exported base64,
/// keeping our dependency surface small and explicit.
mod base64_lite {
    pub use bitcoin::base64::engine::general_purpose::STANDARD;
    pub use bitcoin::base64::Engine;
}

#[cfg(test)]
mod keychain_gate_tests {
    use super::{has_os_keychain, keychain_enabled_for};

    #[test]
    fn debug_builds_skip_the_keychain_unless_explicitly_enabled() {
        assert!(!keychain_enabled_for(true, None, true));
        assert!(!keychain_enabled_for(true, Some("0"), true));
        assert!(keychain_enabled_for(true, Some("1"), true));
    }

    #[test]
    fn release_builds_allow_the_keychain() {
        assert!(keychain_enabled_for(false, None, true));
    }

    #[test]
    fn android_never_uses_the_keychain_even_with_the_override() {
        assert!(!has_os_keychain("android"));
        for os in ["macos", "ios", "windows", "linux"] {
            assert!(has_os_keychain(os), "{os}");
        }
        assert!(!keychain_enabled_for(false, None, false));
        assert!(!keychain_enabled_for(true, Some("1"), false));
    }

    /// The real Credential Manager, under a throwaway name. Windows lets only a logged-in session
    /// save credentials ("Credentials cannot be saved from this logon session" over an SSH key
    /// login), so this is ignored in the normal run; run it in the session with `--ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore = "needs a logged-in Windows session; run with --ignored there"]
    fn the_passphrase_is_kept_in_windows_credential_manager_and_removed() {
        use super::{keychain_clear_in, keychain_load_from, keychain_store_in, KEYRING_ACCOUNT};
        let service = format!("com.nakos.app.test-{}", uuid::Uuid::new_v4().simple());
        let target = format!("{KEYRING_ACCOUNT}.{service}");
        // Removed again even if an assertion fails, so a failed run leaves nothing behind.
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = keychain_clear_in(&self.0);
            }
        }
        let _cleanup = Cleanup(service.clone());
        let listed = || {
            let out = std::process::Command::new("cmdkey").arg(format!("/list:{target}")).output().unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        let passphrase = "throwaway test passphrase ü 1";
        assert!(keychain_load_from(&service).unwrap().is_none(), "nothing there before");
        keychain_store_in(&service, passphrase).unwrap();
        // A fresh entry each call: keyring's in-memory stand-in would have forgotten it.
        assert_eq!(keychain_load_from(&service).unwrap().as_deref().map(String::as_str), Some(passphrase));
        let shown = listed();
        assert!(shown.contains(&format!("Target: {target}")) && shown.contains("Type: Generic") && shown.contains(&format!("User: {KEYRING_ACCOUNT}")), "{shown}");
        assert!(!shown.contains(passphrase), "cmdkey never shows the secret");
        keychain_clear_in(&service).unwrap();
        assert!(keychain_load_from(&service).unwrap().is_none(), "cleared");
        assert!(listed().contains("* NONE *"), "Credential Manager no longer has it");
        keychain_clear_in(&service).unwrap();
    }
}

/// R9: what can be checked about how key material is handled. Heap and stack residue cannot be
/// observed directly; these tests pin the pieces that can: the derived bytes are unchanged, the key
/// struct and the Argon2 working memory are wiped, and the cipher's key schedule is wipe-on-drop.
#[cfg(test)]
mod key_hygiene_tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn params(mem: u32, time: u32, lanes: u32) -> KdfParams {
        KdfParams {
            salt_b64: "Zml4ZWRzYWx0MTIzNDU2Nw".into(), // the 16 bytes "fixedsalt1234567"
            mem_cost_kib: mem,
            time_cost: time,
            parallelism: lanes,
            canary_nonce_b64: String::new(),
            canary_ct_b64: String::new(),
        }
    }

    /// Vectors recorded from the PHC-string implementation this code replaced (the production
    /// cost, a multibyte passphrase, and several lanes). A vault created before the change must
    /// derive the same key, or it can no longer be opened.
    #[test]
    fn derive_key_is_byte_for_byte_what_it_always_was() {
        for (pass, mem, t, p, want) in [
            ("correct horse battery", 19 * 1024, 2, 1, "07cad6b44f4fcc09c5b34361296badef975096e0c885708cb3041229ba06e82a"),
            ("pässwörd 日本語 ✓", 64, 1, 1, "5ff4c23b6ede2b423bc688735f9dadf43f0da3ab9df10767f57df87d6a8eb99b"),
            ("lanes", 256, 3, 4, "260f8899ba71bf2ae323e384ecb5497552a18925eb4cf0eafdcbb64352d10df1"),
        ] {
            let key = derive_key(pass, &params(mem, t, p)).unwrap();
            assert_eq!(hex(&key.0), want, "{pass:?} ({mem} KiB, t={t}, p={p})");
        }
    }

    #[test]
    fn a_bad_salt_or_cost_is_an_error_and_the_vault_still_round_trips() {
        let mut bad = params(64, 1, 1);
        bad.salt_b64 = "c2hvcnQ".into(); // 5 bytes: too short for Argon2
        assert!(matches!(derive_key("whatever", &bad), Err(CryptoError::Kdf(_))));
        assert!(matches!(derive_key("whatever", &params(0, 1, 1)), Err(CryptoError::Kdf(_))));
        let (kdf, key) = init_passphrase("correct horse battery").unwrap();
        assert_eq!(unlock("correct horse battery", &kdf).unwrap().0, key.0);
        assert!(matches!(unlock("wrong passphrase", &kdf), Err(CryptoError::WrongPassphrase)));
    }

    #[test]
    fn the_master_key_is_wiped_when_it_is_dropped() {
        let mut key = std::mem::ManuallyDrop::new(MasterKey([0xAB; KEY_LEN]));
        // SAFETY: the storage stays valid inside ManuallyDrop; this is the drop that would run at
        // the end of the key's life, and the read is of plain bytes afterwards.
        unsafe { std::mem::ManuallyDrop::drop(&mut key) };
        let after = unsafe { std::ptr::read_volatile(&key.0) };
        assert_eq!(after, [0u8; KEY_LEN]);
    }

    /// Argon2's working memory ends in the block the key is hashed from, so it is as sensitive as
    /// the key. It must be all zero (its whole capacity) once the derivation returns.
    #[test]
    fn the_argon2_working_memory_is_wiped_after_the_derivation() {
        let mut blocks: Vec<Block> = Vec::new();
        let mut out = [0u8; KEY_LEN];
        derive_into("correct horse battery", &params(256, 2, 2), &mut out, &mut blocks).unwrap();
        assert_ne!(out, [0u8; KEY_LEN]);
        let bytes = blocks.capacity() * std::mem::size_of::<Block>();
        assert!(bytes >= 256 * 1024, "the memory was really allocated: {bytes} bytes");
        // SAFETY: the whole capacity was initialised (resize) and then overwritten with zeros.
        let memory = unsafe { std::slice::from_raw_parts(blocks.as_ptr() as *const u8, bytes) };
        assert!(memory.iter().all(|b| *b == 0), "Argon2 working memory still holds data");
    }

    /// Compile-time check: with the `aes` crate's `zeroize` feature on, the cipher that AES-GCM
    /// builds for every call wipes its key schedule when dropped. Without the feature this stops
    /// compiling.
    #[test]
    fn the_aes_key_schedule_is_wiped_on_drop() {
        fn wiped_on_drop<T: ZeroizeOnDrop>() {}
        wiped_on_drop::<aes::Aes256>();
    }
}
