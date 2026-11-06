use crate::crypto::backup as sealed;
use crate::crypto::{self, KdfParams, MasterKey};
use zeroize::Zeroizing;
use super::{durable, file_dialog};
use crate::db;
use crate::state::AppState;
use rusqlite::params;
use serde::Serialize;
use uuid::Uuid;

const MAX_LOCAL_BACKUPS: usize = 14;
/// db_meta flag set only when the user ticked "Remember on this device" and
/// the keychain write succeeded. Until then the OS keychain is never read.
const KEYCHAIN_OPT_IN_META: &str = "keychain_opt_in";

#[derive(Debug, Serialize)]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
    /// The user opted in (read from our own database; no OS keychain call).
    pub keychain_remembered: bool,
    /// False in debug / dev builds and on Android, where the keychain is never used.
    pub keychain_supported: bool,
    /// "android", "ios" or "desktop", so the UI can say "this phone" instead of "this computer".
    pub platform: &'static str,
    /// The OS itself ("macos", "windows", "linux", ...), for the few words that differ between
    /// desktops: the shortcut key, an example path, what the keychain does.
    pub os: &'static str,
}

/// Why the keychain helper can't be turned on in this build.
pub fn keychain_off_reason(os: &str) -> &'static str {
    if crypto::has_os_keychain(os) {
        "the OS keychain isn't used in development builds"
    } else {
        "Nakos on Android doesn't save the passphrase on the phone"
    }
}

/// The platform name the UI gets, from `std::env::consts::OS`.
pub fn platform_name(os: &str) -> &'static str {
    match os {
        "android" => "android",
        "ios" => "ios",
        _ => "desktop",
    }
}

fn keychain_opted_in(conn: &rusqlite::Connection) -> bool {
    db::meta_get(conn, KEYCHAIN_OPT_IN_META).ok().flatten().as_deref() == Some("1")
}

/// Store the passphrase in the OS keychain (opt-in) and record the opt-in.
/// A no-op in builds where the keychain is disabled.
fn remember_passphrase(conn: &rusqlite::Connection, passphrase: &str) -> Result<(), String> {
    if !crypto::keychain_enabled() {
        return Ok(());
    }
    crypto::keychain_store_passphrase(passphrase).map_err(|e| e.to_string())?;
    db::meta_set(conn, KEYCHAIN_OPT_IN_META, "1").map_err(|e| e.to_string())
}

#[tauri::command]
pub fn vault_status(state: tauri::State<AppState>) -> Result<VaultStatus, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let initialized = db::meta_get(&conn, "kdf_params").map_err(|e| e.to_string())?.is_some();

    let vault = state.vault()?;
    let unlocked = vault.lock().master_key.is_some();
    // Deliberately no OS keychain call here: this runs on every app start,
    // and reading the item is what raises the macOS password prompt.
    let keychain_supported = crypto::keychain_enabled();
    let keychain_remembered = keychain_supported && keychain_opted_in(&conn);

    let platform = platform_name(std::env::consts::OS);
    let os = std::env::consts::OS;
    Ok(VaultStatus { initialized, unlocked, keychain_remembered, keychain_supported, platform, os })
}

#[tauri::command]
pub fn vault_init(state: tauri::State<AppState>, passphrase: String, remember: bool) -> Result<(), String> {
    // Wiped when this function returns, on every path.
    let passphrase = Zeroizing::new(passphrase);
    if passphrase.len() < 8 {
        return Err("passphrase must be at least 8 characters".into());
    }
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    if db::meta_get(&conn, "kdf_params").map_err(|e| e.to_string())?.is_some() {
        return Err("vault is already initialized".into());
    }

    let (params, key) = crypto::init_passphrase(&passphrase).map_err(|e| e.to_string())?;
    let params_json = serde_json::to_string(&params).map_err(|e| e.to_string())?;

    // Keychain first: if it fails, nothing has been persisted, so the user
    // can simply retry (or untick "Remember") instead of being stuck with a
    // half-created vault that reports "already initialized".
    if remember && crypto::keychain_enabled() {
        crypto::keychain_store_passphrase(&passphrase).map_err(|e| e.to_string())?;
    }
    db::meta_set(&conn, "kdf_params", &params_json).map_err(|e| e.to_string())?;
    if remember {
        remember_passphrase(&conn, &passphrase)?;
    }

    let vault = state.vault()?;
    vault.lock().master_key = Some(key);
    state.auto_lock().reset();
    Ok(())
}

#[tauri::command]
pub fn vault_unlock(state: tauri::State<AppState>, passphrase: String, remember: bool) -> Result<(), String> {
    let passphrase = Zeroizing::new(passphrase);
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let params_json = db::meta_get(&conn, "kdf_params")
        .map_err(|e| e.to_string())?
        .ok_or("vault has not been initialized yet")?;
    let params: KdfParams = serde_json::from_str(&params_json).map_err(|e| e.to_string())?;

    let key = crypto::unlock(&passphrase, &params).map_err(|e| e.to_string())?;

    // The vault is unlocked by the passphrase alone. Remembering is optional
    // and its failure must not undo or block the unlock.
    state.vault()?.lock().master_key = Some(key);
    state.auto_lock().reset();
    if remember {
        remember_passphrase(&conn, &passphrase)?;
    }
    Ok(())
}

/// Locks now. Like the automatic lock, this also forgets a passphrase remembered in the OS
/// keychain: "Remember" keeps the vault open across restarts only while the session is unlocked.
#[tauri::command]
pub fn vault_lock(state: tauri::State<AppState>) -> Result<(), String> {
    super::autolock::lock_now(&state).map(|_| ())
}

/// Turns the keychain helper off: the opt-in flag first (so the item is never read again even if
/// removing it fails), then the item. Does nothing when the person never opted in, so a lock never
/// touches the OS keychain (and never raises a prompt) for someone who didn't ask for it.
pub(crate) fn forget_remembered_passphrase(state: &AppState) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    if !keychain_opted_in(&conn) {
        return Ok(());
    }
    db::meta_set(&conn, KEYCHAIN_OPT_IN_META, "0").map_err(|e| e.to_string())?;
    if crypto::keychain_enabled() {
        crypto::keychain_clear().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Settings toggle: "Remember helper in the OS keychain". Off by default, and a
/// no-op error in development builds (which never touch the keychain).
/// Turning it ON needs the vault passphrase again (we only hold the derived
/// key while unlocked) and verifies it before storing anything; turning it OFF
/// clears the flag first, then removes the item. Only the passphrase is ever
/// stored there; never seeds, RPC passwords or tokens.
#[tauri::command]
pub fn set_keychain_helper(state: tauri::State<AppState>, enabled: bool, passphrase: Option<String>) -> Result<(), String> {
    if !crypto::keychain_enabled() {
        return Err(keychain_off_reason(std::env::consts::OS).into());
    }
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    if enabled {
        let passphrase = passphrase.map(Zeroizing::new).filter(|p| !p.is_empty()).ok_or("enter your vault passphrase to turn this on")?;
        let params = read_kdf_params(&conn)?;
        crypto::unlock(&passphrase, &params).map_err(|_| "that isn't the vault passphrase".to_string())?;
        remember_passphrase(&conn, &passphrase)
    } else {
        db::meta_set(&conn, KEYCHAIN_OPT_IN_META, "0").map_err(|e| e.to_string())?;
        crypto::keychain_clear().map_err(|e| e.to_string())
    }
}

/// Attempt to silently unlock from a passphrase cached in the OS keychain.
/// Only ever touches the keychain when the user opted in on a build that
/// allows it. Every failure (denied prompt, new-binary ACL, missing or stale
/// item) returns `Ok(false)` so the normal passphrase screen takes over.
#[tauri::command]
pub fn vault_try_keychain_unlock(state: tauri::State<AppState>) -> Result<bool, String> {
    if !crypto::keychain_enabled() {
        return Ok(false);
    }
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    if !keychain_opted_in(&conn) {
        return Ok(false);
    }
    let Ok(Some(passphrase)) = crypto::keychain_load_passphrase() else {
        return Ok(false);
    };
    let Some(params_json) = db::meta_get(&conn, "kdf_params").map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let params: KdfParams = serde_json::from_str(&params_json).map_err(|e| e.to_string())?;
    let Ok(key) = crypto::unlock(&passphrase, &params) else {
        return Ok(false);
    };
    state.vault()?.lock().master_key = Some(key);
    state.auto_lock().reset();
    Ok(true)
}

/// An independent copy of the unlocked key for a long operation, so the vault lock is not held
/// for its duration. Copied straight from the vault's key into a heap box (no temporary array
/// holding the key on the stack) and wiped when the box is dropped.
fn require_key(state: &AppState) -> Result<Box<MasterKey>, String> {
    let vault = state.vault()?;
    let guard = vault.lock();
    let key = guard.master_key.as_ref().ok_or("vault is locked")?;
    let mut copy = Box::new(MasterKey([0u8; crypto::KEY_LEN]));
    copy.0.copy_from_slice(&key.0);
    Ok(copy)
}

const MAX_RESTORE_BYTES: u64 = 512 * 1024 * 1024;

fn read_kdf_params(conn: &rusqlite::Connection) -> Result<KdfParams, String> {
    let json = db::meta_get(conn, "kdf_params").map_err(|e| e.to_string())?.ok_or("vault has not been initialized yet")?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

/// Build an encrypted vault backup (format 2, see `crypto::backup`) from the
/// live database. The vault passphrase is the only secret involved.
pub fn create_backup_file(pool: &db::DbPool, app_data_dir: &std::path::Path, key: &MasterKey) -> Result<Vec<u8>, String> {
    let kdf = {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA wal_checkpoint(FULL);").map_err(|e| e.to_string())?;
        read_kdf_params(&conn)?
    };
    let seal_file = || -> Result<Result<Vec<u8>, sealed::BackupError>, String> {
        let raw = std::fs::read(db::db_file_path(app_data_dir)).map_err(|e| e.to_string())?;
        Ok(sealed::seal(&raw, key, &kdf, &chrono::Utc::now().to_rfc3339(), env!("CARGO_PKG_VERSION")))
    };
    match seal_file()? {
        Err(sealed::BackupError::ContainsKeyMaterial) => {
            // SQLite does not erase text that was edited away: it can sit in the file's free space and
            // still trip the guard. Rebuild the file once and look again before refusing.
            rebuild_database_file(pool);
            match seal_file()? {
                Err(sealed::BackupError::ContainsKeyMaterial) => Err(key_text_refusal(pool)),
                other => other.map_err(|e| e.to_string()),
            }
        }
        other => other.map_err(|e| e.to_string()),
    }
}

/// Drop free pages (and with them any stale copy of edited-away text) by rebuilding the file.
/// Best effort: if another connection is busy this simply does nothing.
fn rebuild_database_file(pool: &db::DbPool) {
    if let Ok(conn) = pool.get() {
        let _ = conn.execute_batch("VACUUM;");
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
}

/// Plain-text columns that can hold user, imported or remote text, and whether the app lets the
/// owner edit them afterwards. Used only to say where key-like text is, never to show it.
const TEXT_COLUMNS: &[(&str, &str, bool)] = &[
    ("wallets", "name", true),
    ("wallets", "notes", true),
    ("wallets", "color", true),
    ("wallets", "master_fingerprint", false),
    ("wallets", "derivation_path", false),
    ("wallets", "descriptor", false),
    ("wallets", "change_descriptor", false),
    ("wallets", "xpub", false),
    ("multisig_cosigners", "label", false),
    ("multisig_cosigners", "hardware_model", false),
    ("multisig_cosigners", "xpub", false),
    ("multisig_cosigners", "fingerprint", false),
    ("multisig_cosigners", "derivation_path", false),
    ("recovery_contacts", "name", false),
    ("recovery_contacts", "relationship", false),
    ("recovery_contacts", "contact_method", false),
    ("utxos", "label", true),
    ("utxos", "acquisition_note", true),
    ("security_config", "hardware_wallet_model", true),
    ("node_configs", "label", true),
    ("node_configs", "host", true),
    ("node_configs", "rpc_user", true),
    ("node_configs", "cookie_path", true),
    ("wallet_snapshots", "as_of", false),
    ("health_score_snapshots", "details_json", false),
    ("backup_log", "file_path", false),
    ("db_meta", "value", true),
];

/// `table.column` for every listed column that holds text looking like an extended private key.
fn locate_key_like_text(conn: &rusqlite::Connection) -> Vec<String> {
    let mut found = Vec::new();
    for (table, column, editable) in TEXT_COLUMNS {
        // Table and column names are the constants above, never user input.
        let Ok(mut stmt) = conn.prepare(&format!("SELECT {column} FROM {table} WHERE {column} IS NOT NULL")) else { continue };
        let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else { continue };
        if rows.flatten().any(|v| sealed::contains_extended_private_key(v.as_bytes())) {
            found.push(if *editable { format!("{table}.{column}") } else { format!("{table}.{column} (can't be edited in Nakos)") });
        }
    }
    found
}

fn key_text_refusal(pool: &db::DbPool) -> String {
    let places = pool.get().map(|c| locate_key_like_text(&c)).unwrap_or_default();
    if places.is_empty() {
        "the backup was refused: the database file holds text that looks like an extended private key, and Nakos could not tell which field it is in".to_string()
    } else {
        format!("the backup was refused: text that looks like an extended private key is stored in {}. Nakos never stores private keys; edit or remove that text and try again", places.join(", "))
    }
}

/// Replace `nakos.db` with `db_bytes`. The caller must have closed its
/// connection pool first. The current file is kept as `nakos.db.pre-restore-bak`,
/// and stale `-wal` / `-shm` files are removed so they can't be replayed onto
/// the new database. Every step is synced to the disk, so a power cut leaves
/// either the old database or the new one, never a half-written file.
pub fn swap_in_database(app_data_dir: &std::path::Path, db_bytes: &[u8]) -> Result<(), String> {
    let db_path = db::db_file_path(app_data_dir);
    let tmp = app_data_dir.join("nakos.db.restore-tmp");
    durable::write_synced(&tmp, db_bytes)?;
    if db_path.exists() {
        durable::copy_synced(&db_path, &db_path.with_extension("db.pre-restore-bak"))?;
    }
    for suffix in ["-wal", "-shm"] {
        let mut name = db_path.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(name));
    }
    // The removals are kept before the rename, so an old -wal can't come back next to the new file.
    durable::sync_dir(app_data_dir)?;
    std::fs::rename(&tmp, &db_path).map_err(|e| e.to_string())?;
    durable::sync_dir(app_data_dir).map_err(|e| format!("the backup was restored, but Nakos could not confirm it is saved to disk ({e}); restart Nakos and check your wallets"))
}

/// Remove a database file Nakos made and the `-wal` / `-shm` files SQLite keeps next to it.
/// Missing files are fine.
fn remove_with_companions(path: &std::path::Path) -> Result<(), String> {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        remove_if_present(std::path::Path::new(&name))?;
    }
    Ok(())
}

fn remove_if_present(path: &std::path::Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not delete {}: {e}", path.display())),
    }
}

/// Open a format-2 backup with its passphrase, prove the vault inside is
/// usable (its own KDF settings match the header and the passphrase unlocks
/// its canary), then swap it in. Nothing is replaced unless all of that holds.
/// Returns the vault key for the restored database.
pub fn restore_backup_file(app_data_dir: &std::path::Path, file: &[u8], passphrase: &str) -> Result<MasterKey, String> {
    let opened = sealed::open(file, passphrase).map_err(|e| e.to_string())?;

    // Verify on a scratch copy, never on the live file. It is read once and deleted, so it is not
    // synced; a stale -wal from an interrupted earlier restore is cleared so it can't be replayed
    // onto this copy.
    std::fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
    let scratch = app_data_dir.join("nakos.db.verify-tmp");
    remove_with_companions(&scratch)?;
    std::fs::write(&scratch, &opened.db_bytes).map_err(|e| e.to_string())?;
    let verdict = (|| -> Result<(), String> {
        let conn = rusqlite::Connection::open_with_flags(&scratch, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| sealed::BackupError::NotADatabase.to_string())?;
        let params = read_kdf_params(&conn).map_err(|_| sealed::BackupError::InconsistentVault.to_string())?;
        if params.salt_b64 != opened.header.kdf.salt_b64 {
            return Err(sealed::BackupError::InconsistentVault.to_string());
        }
        crypto::unlock(passphrase, &params).map_err(|_| sealed::BackupError::InconsistentVault.to_string())?;
        Ok(())
        // The connection is closed here. The copy is a WAL-mode database opened read-only, and a
        // read-only connection leaves the -wal / -shm it opened behind on close, so those go too.
    })();
    remove_with_companions(&scratch).ok();
    verdict?;

    swap_in_database(app_data_dir, &opened.db_bytes)?;
    Ok(opened.key)
}

/// What a restore or an erase answers when another task still has the database open.
pub(crate) const DB_STILL_IN_USE: &str = "Nakos is still using its database (a sync or another task may be running). Nothing was changed; wait a moment and try again.";

/// How `with_pool_closed` decides the database file is free to replace or delete, and how long it
/// waits for that. Tests pass a stand-in.
pub(crate) struct Release {
    pub in_use: fn(&std::path::Path) -> bool,
    pub wait: std::time::Duration,
}

pub(crate) const RELEASE: Release = Release { in_use: database_in_use, wait: std::time::Duration::from_secs(2) };

/// Windows will not delete, replace or rename a file another handle has open, and SQLite opens it
/// without letting others delete it. Opening it with no sharing at all fails with
/// ERROR_SHARING_VIOLATION exactly while some connection still holds it.
#[cfg(windows)]
pub(crate) fn database_in_use(dir: &std::path::Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    match std::fs::OpenOptions::new().read(true).share_mode(0).open(db::db_file_path(dir)) {
        Ok(_) => false,
        Err(e) => e.raw_os_error() == Some(ERROR_SHARING_VIOLATION),
    }
}

/// Elsewhere an open file can be removed or renamed over; a connection still open keeps the old,
/// unlinked file, which cannot reach the new one (the erase test checks).
#[cfg(not(windows))]
pub(crate) fn database_in_use(_dir: &std::path::Path) -> bool {
    false
}

/// Polls `busy` until it is false or `wait` has passed. True when it became free.
fn wait_until_free(mut busy: impl FnMut() -> bool, wait: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + wait;
    loop {
        if !busy() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Close the pool so no connection holds the old file, run `f`, then always
/// reopen a pool (on the restored file if `f` succeeded, else the untouched one).
/// Dropping the pool closes its idle connections; one another command still has checked out
/// stays open until that command finishes. If the file is still in use when `release.wait` is up,
/// `f` is not run and the answer is `DB_STILL_IN_USE`.
fn with_pool_closed<T>(state: &AppState, release: &Release, f: impl FnOnce(&std::path::Path) -> Result<T, String>) -> Result<T, String> {
    let dir = state.app_data_dir()?;
    drop(state.inner.lock().pool.take());
    let result = if wait_until_free(|| (release.in_use)(&dir), release.wait) {
        f(&dir)
    } else {
        Err(DB_STILL_IN_USE.to_string())
    };
    let pool = db::open_pool(&dir).map_err(|e| e.to_string())?;
    state.inner.lock().pool = Some(pool);
    result
}

/// Export an encrypted vault backup. Rust opens the save dialog, so the webview never names
/// the path; returns the path written, or `None` when the person cancelled. This is the whole
/// vault, meant for the owner; it is not the family letter or the advanced packet.
#[tauri::command]
pub async fn export_encrypted_backup(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    // Fail before any dialog if the vault is locked.
    drop(require_key(&state)?);
    let Some(dest) = file_dialog::pick_save_file(&app, "Save encrypted vault backup", "nakos-vault-backup.nakosbak", "Nakos encrypted backup", &["nakosbak"]).await? else {
        return Ok(None);
    };
    write_encrypted_backup(&state, &file_dialog::shown(&dest), |bytes| file_dialog::write_picked(&app, &dest, bytes)).map(Some)
}

/// Seal the vault and hand the bytes to `write`; `shown` names the destination in the log and
/// the result.
fn write_encrypted_backup(state: &AppState, shown: &str, write: impl FnOnce(&[u8]) -> Result<(), String>) -> Result<String, String> {
    let key = require_key(state)?;
    let app_data_dir = state.app_data_dir()?;
    let pool = state.pool()?;
    let bytes = create_backup_file(&pool, &app_data_dir, &key)?;
    write(&bytes)?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO backup_log (id, file_path, size_bytes, kind) VALUES (?1,?2,?3,'export')",
        params![Uuid::new_v4().to_string(), shown, bytes.len() as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(shown.to_string())
}

#[derive(Debug, Serialize)]
pub struct RestoreSummary {
    pub wallets: i64,
}

/// Step one of a restore: Rust opens the "choose a backup" dialog and remembers the file the
/// person picked. Returns that path for display, or `None` when cancelled. The webview never
/// supplies a path: `restore_encrypted_backup` reads only this remembered file.
#[tauri::command]
pub async fn pick_backup_to_restore(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let Some(file) = file_dialog::pick_open_file(&app, "Choose an encrypted Nakos backup", "Nakos encrypted backup", &["nakosbak"]).await? else {
        return Ok(None);
    };
    let shown = file_dialog::shown(&file);
    state.grant_restore_file(file);
    Ok(Some(shown))
}

/// Step two: restore the file chosen by `pick_backup_to_restore`. This REPLACES the current vault
/// with the one in the file, and the backup's passphrase becomes the vault passphrase. Works
/// before any vault exists (first run) and while unlocked. Older (format 1) files can only be
/// restored into the vault they came from.
#[tauri::command]
pub fn restore_encrypted_backup(app: tauri::AppHandle, state: tauri::State<AppState>, passphrase: String) -> Result<RestoreSummary, String> {
    let passphrase = Zeroizing::new(passphrase);
    let chosen = state.restore_grant()?;
    let summary = restore_from(&state, || file_dialog::read_picked(&app, &chosen, MAX_RESTORE_BYTES, TOO_LARGE_FOR_A_BACKUP), &passphrase)?;
    state.clear_restore_grant();
    Ok(summary)
}

const TOO_LARGE_FOR_A_BACKUP: &str = "that file is too large to be a Nakos backup";

/// Restore from the bytes `read` returns (the chosen file, read with the size cap).
fn restore_from(state: &AppState, read: impl FnOnce() -> Result<Vec<u8>, String>, passphrase: &str) -> Result<RestoreSummary, String> {
    restore_from_with(state, read, passphrase, &RELEASE)
}

fn restore_from_with(state: &AppState, read: impl FnOnce() -> Result<Vec<u8>, String>, passphrase: &str, release: &Release) -> Result<RestoreSummary, String> {
    let file = read()?;

    if file.starts_with(sealed::MAGIC_V1) {
        let key = require_key(state).map_err(|_| sealed::BackupError::OlderFormat.to_string())?;
        let nonce = &file[8..8 + crypto::NONCE_LEN.min(file.len().saturating_sub(8))];
        let ct = file.get(8 + crypto::NONCE_LEN..).ok_or("not a valid Nakos export file")?;
        let db_bytes = key
            .decrypt(nonce, ct)
            .map_err(|_| "decryption failed: this older backup belongs to a different vault passphrase, or the file is damaged".to_string())?;
        if db_bytes.len() < 16 || &db_bytes[..16] != b"SQLite format 3\0" {
            return Err(sealed::BackupError::NotADatabase.to_string());
        }
        sealed::reject_key_material(&db_bytes).map_err(|e| e.to_string())?;
        with_pool_closed(state, release, |dir| swap_in_database(dir, &db_bytes))?;
    } else {
        let key = with_pool_closed(state, release, |dir| restore_backup_file(dir, &file, passphrase))?;
        state.vault()?.lock().master_key = Some(key);
    }

    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let wallets: i64 = conn.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    Ok(RestoreSummary { wallets })
}

/// The word the person types to confirm an erase. The command refuses anything else, so a stray
/// call from the webview cannot wipe the vault.
const ERASE_CONFIRMATION: &str = "ERASE";

#[derive(Debug, Serialize)]
pub struct EraseSummary {
    /// False when the OS keychain item could not be removed (it is never read again: the opt-in
    /// flag that guards it was in the erased database).
    pub keychain_cleared: bool,
}

/// Delete exactly the files Nakos writes in its data folder: the database, the restore
/// leftovers, and the automatic local copies. Nothing else in the folder is touched, so a wrong
/// path cannot take unrelated files with it. Missing files are fine. The database's `-wal` /
/// `-shm` files are left to SQLite: closing the pool folds them in and removes them, and a fresh
/// database resets any that a still-open connection kept (the erase test checks no old data
/// survives). The restore scratch files' companions have no open connection, so they go here.
fn erase_data_files(dir: &std::path::Path) -> Result<(), String> {
    let db_path = db::db_file_path(dir);
    remove_if_present(&db_path)?;
    remove_if_present(&db_path.with_extension("db.pre-restore-bak"))?;
    for f in [dir.join("nakos.db.restore-tmp"), dir.join("nakos.db.verify-tmp")] {
        remove_with_companions(&f)?;
    }
    match std::fs::remove_dir_all(dir.join("backups")) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not delete the local copies: {e}")),
    }
}

fn erase_all_data_inner(state: &AppState, confirm: &str, clear_keychain: impl FnOnce() -> Result<(), String>) -> Result<EraseSummary, String> {
    erase_all_data_with(state, confirm, &RELEASE, clear_keychain)
}

fn erase_all_data_with(state: &AppState, confirm: &str, release: &Release, clear_keychain: impl FnOnce() -> Result<(), String>) -> Result<EraseSummary, String> {
    if confirm != ERASE_CONFIRMATION {
        return Err(format!("type {ERASE_CONFIRMATION} to confirm"));
    }
    // The pool is reopened on a new, empty database, so the app is back at "Create your vault".
    let erased = with_pool_closed(state, release, erase_data_files);
    // Refused because the database is still in use: nothing was deleted, so the vault stays as it was.
    if erased.as_ref().err().map(String::as_str) == Some(DB_STILL_IN_USE) {
        return Err(DB_STILL_IN_USE.to_string());
    }
    state.vault()?.lock().master_key = None;
    state.clear_restore_grant();
    erased?;
    Ok(EraseSummary { keychain_cleared: clear_keychain().is_ok() })
}

/// Settings, Erase all data: delete this computer's vault, its local copies and the remembered
/// passphrase, and start again from "Create your vault". Files exported elsewhere (encrypted
/// backups, letters, packets) are not touched.
#[tauri::command]
pub fn erase_all_data(state: tauri::State<AppState>, confirm: String) -> Result<EraseSummary, String> {
    erase_all_data_inner(&state, &confirm, || {
        if crypto::keychain_enabled() {
            crypto::keychain_clear().map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    })
}

#[tauri::command]
pub fn run_local_backup(state: tauri::State<AppState>) -> Result<String, String> {
    let app_data_dir = state.app_data_dir()?;
    let pool = state.pool()?;
    perform_local_backup(&app_data_dir, &pool)
}

/// Shared by the manual `run_local_backup` command and the automatic
/// startup backup fired from `state::init`.
pub fn perform_local_backup(app_data_dir: &std::path::Path, pool: &db::DbPool) -> Result<String, String> {
    let backups_dir = app_data_dir.join("backups");
    std::fs::create_dir_all(&backups_dir).map_err(|e| e.to_string())?;

    {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA wal_checkpoint(FULL);").map_err(|e| e.to_string())?;
    }

    let db_path = db::db_file_path(app_data_dir);
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup_path = backups_dir.join(format!("nakos-{ts}.db"));
    std::fs::copy(&db_path, &backup_path).map_err(|e| e.to_string())?;

    let size = std::fs::metadata(&backup_path).map(|m| m.len() as i64).unwrap_or(0);
    {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO backup_log (id, file_path, size_bytes, kind) VALUES (?1,?2,?3,'automatic')",
            params![Uuid::new_v4().to_string(), backup_path.to_string_lossy(), size],
        )
        .map_err(|e| e.to_string())?;
    }

    rotate_backups(&backups_dir)?;
    Ok(backup_path.to_string_lossy().to_string())
}

fn rotate_backups(backups_dir: &std::path::Path) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(backups_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "db").unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    if entries.len() > MAX_LOCAL_BACKUPS {
        for old in &entries[..entries.len() - MAX_LOCAL_BACKUPS] {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct BackupEntry {
    pub id: String,
    pub file_path: String,
    pub size_bytes: i64,
    pub kind: String,
    pub created_at: String,
}

#[tauri::command]
pub fn list_backups(state: tauri::State<AppState>) -> Result<Vec<BackupEntry>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, file_path, size_bytes, kind, created_at FROM backup_log ORDER BY created_at DESC LIMIT 100")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(BackupEntry {
                id: r.get(0)?,
                file_path: r.get(1)?,
                size_bytes: r.get(2)?,
                kind: r.get(3)?,
                created_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[cfg(test)]
mod backup_tests {
    use super::*;
    use bitcoin::base64::{engine::general_purpose::STANDARD, Engine};
    use std::path::PathBuf;

    const XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
    const RPC_SECRET: &str = "RPCPASSWORD-SECRET-7f3a91";
    const TOKEN_SECRET: &str = "EXPLORER-TOKEN-SECRET-c41d";
    const PASS: &str = "correct horse battery staple";

    /// Fresh temp directory under the system temp dir; never the real app data dir. Removed when
    /// it is dropped, so keep it until the test is done with the folder.
    fn temp_dir(tag: &str) -> crate::test_dir::TestDir {
        crate::test_dir::TestDir::new(tag)
    }

    /// A vault with a wallet, a node whose password is sealed, and a sealed explorer token.
    /// R9: the key copy handed to a long operation is independent of the vault's own key, equal
    /// to it, and wiped when dropped.
    #[test]
    fn require_key_is_an_independent_copy_that_is_wiped_on_drop() {
        use crate::state::VaultState;
        let (_, original) = crypto::init_passphrase("correct horse battery").unwrap();
        let want = original.0;
        let state = AppState::default();
        state.inner.lock().vault = Some(std::sync::Arc::new(parking_lot::Mutex::new(VaultState { master_key: Some(original) })));
        let copy = require_key(&state).unwrap();
        assert_eq!(copy.0, want);
        let raw = Box::into_raw(copy);
        // SAFETY: `raw` came from a Box and is freed exactly once below without a second drop.
        unsafe {
            assert_ne!(raw as usize, 0);
            std::ptr::drop_in_place(raw);
            assert_eq!(std::ptr::read_volatile(&(*raw).0), [0u8; crypto::KEY_LEN], "the copy is wiped on drop");
            std::alloc::dealloc(raw as *mut u8, std::alloc::Layout::new::<MasterKey>());
        }
        // The vault's own key is untouched, and locking clears it.
        assert_eq!(state.vault().unwrap().lock().master_key.as_ref().unwrap().0, want);
        state.vault().unwrap().lock().master_key = None;
        assert!(require_key(&state).is_err());
    }

    fn state_for(dir: &std::path::Path, key: Option<MasterKey>) -> AppState {
        let state = AppState::default();
        {
            let mut inner = state.inner.lock();
            inner.pool = Some(db::open_pool(dir).unwrap());
            inner.app_data_dir = Some(dir.to_path_buf());
            inner.vault = Some(std::sync::Arc::new(parking_lot::Mutex::new(crate::state::VaultState { master_key: key })));
        }
        state
    }

    /// Any lock drops the key and turns "Remember" off, so a remembered passphrase never outlives
    /// the unlocked session. Someone who never opted in is left alone (no flag written). Debug
    /// builds never call the OS keychain, so this touches only the test database.
    #[test]
    fn locking_drops_the_key_and_turns_remember_off() {
        let dir = temp_dir("lock-forgets");
        let (_, key) = crypto::init_passphrase(PASS).unwrap();
        let state = state_for(dir.path(), Some(key));
        let conn = state.pool().unwrap().get().unwrap();
        assert!(super::super::autolock::lock_now(&state).unwrap(), "it was unlocked");
        assert!(state.vault().unwrap().lock().master_key.is_none());
        assert_eq!(db::meta_get(&conn, KEYCHAIN_OPT_IN_META).unwrap(), None, "never opted in: nothing written");

        db::meta_set(&conn, KEYCHAIN_OPT_IN_META, "1").unwrap();
        // A release-mode test run uses the real OS keychain; on a headless Linux box (no Secret
        // Service) removing the item fails. The lock and the flag must not depend on it.
        match super::super::autolock::lock_now(&state) {
            Ok(was_unlocked) => assert!(!was_unlocked, "already locked"),
            Err(e) => assert!(e.contains("keychain"), "only the keychain step may fail: {e}"),
        }
        assert!(state.vault().unwrap().lock().master_key.is_none());
        assert!(!keychain_opted_in(&conn), "a lock turns Remember off even when already locked");
    }

    /// R11: the file steps behind the commands. Rust opens the dialogs, so these take the path Rust
    /// chose: an export writes a file that opens with the passphrase and is logged, a restore from
    /// that file brings the vault into another database, and a restore needs a remembered file.
    #[test]
    fn export_writes_the_file_rust_chose_and_a_restore_reads_only_the_remembered_one() {
        let src = temp_dir("r11-src");
        let key = make_vault(&src, PASS);
        let state = state_for(&src, Some(MasterKey(key.0)));
        let dest = src.join("out").join("backup.nakosbak");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let write_dest = |b: &[u8]| file_dialog::write_path(&dest, b);
        let shown = write_encrypted_backup(&state, dest.to_str().unwrap(), write_dest).unwrap();
        assert_eq!(shown, dest.to_str().unwrap());
        let file = std::fs::read(&dest).unwrap();
        assert!(sealed::open(&file, PASS).is_ok());
        let logged: String = state.pool().unwrap().get().unwrap().query_row("SELECT file_path FROM backup_log WHERE kind = 'export'", [], |r| r.get(0)).unwrap();
        assert_eq!(logged, shown);
        // A locked vault cannot export, and an unwritable destination is an error, not a panic.
        let locked = state_for(&src, None);
        assert!(write_encrypted_backup(&locked, dest.to_str().unwrap(), write_dest).unwrap_err().contains("locked"));
        let nowhere = src.join("no-such-dir").join("x");
        assert!(write_encrypted_backup(&state, nowhere.to_str().unwrap(), |b| file_dialog::write_path(&nowhere, b)).unwrap_err().contains("could not write"));

        // Restore needs a file chosen in the dialog; nothing else names one.
        let dst = temp_dir("r11-dst");
        make_fresh_other_vault(&dst);
        let heir = state_for(&dst, None);
        assert_eq!(heir.restore_grant().unwrap_err(), "choose a backup file first");
        heir.grant_restore_file(dest.clone());
        let chosen = match heir.restore_grant().unwrap() {
            tauri_plugin_dialog::FilePath::Path(p) => p,
            other => panic!("{other:?}"),
        };
        assert_eq!(chosen, dest);
        let summary = restore_from(&heir, || file_dialog::read_path(&chosen, MAX_RESTORE_BYTES, TOO_LARGE_FOR_A_BACKUP), PASS).unwrap();
        assert_eq!(summary.wallets, 1);
        assert!(heir.vault().unwrap().lock().master_key.is_some());
        heir.clear_restore_grant();
        assert!(heir.restore_grant().is_err());
        // A file that is not a backup, or a path that does not exist, is refused with an error.
        let junk = src.join("junk.bin");
        std::fs::write(&junk, b"not a backup at all").unwrap();
        assert!(restore_from(&heir, || file_dialog::read_path(&junk, MAX_RESTORE_BYTES, TOO_LARGE_FOR_A_BACKUP), PASS).is_err());
        let missing = src.join("missing");
        assert!(restore_from(&heir, || file_dialog::read_path(&missing, MAX_RESTORE_BYTES, TOO_LARGE_FOR_A_BACKUP), PASS).unwrap_err().contains("could not open"));
        // The cap is enforced while reading: one byte over is refused before anything is decrypted.
        assert_eq!(restore_from(&heir, || file_dialog::read_path(&junk, 4, TOO_LARGE_FOR_A_BACKUP), PASS).unwrap_err(), TOO_LARGE_FOR_A_BACKUP);
    }

    /// Every file under `dir`, recursively, as bytes.
    fn all_file_bytes(dir: &std::path::Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                out.extend(all_file_bytes(&p));
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
        out
    }

    /// Everything a restore can leave in the data folder, the scratch files' SQLite companions included.
    const RESTORE_LEFTOVERS: [&str; 7] = [
        "nakos.db.pre-restore-bak",
        "nakos.db.restore-tmp",
        "nakos.db.restore-tmp-wal",
        "nakos.db.restore-tmp-shm",
        "nakos.db.verify-tmp",
        "nakos.db.verify-tmp-wal",
        "nakos.db.verify-tmp-shm",
    ];

    /// Erase all data: refuses without the exact word; then forgets the key and the chosen
    /// restore file, deletes the database (WAL included), restore leftovers and local copies,
    /// leaves unrelated files alone, reopens an empty vault, and reports the keychain result.
    #[test]
    fn erase_needs_the_word_then_leaves_an_empty_vault_and_no_copies() {
        let dir = temp_dir("erase");
        let key = make_vault(&dir, PASS);
        let state = state_for(&dir, Some(MasterKey(key.0)));
        perform_local_backup(&dir, &state.pool().unwrap()).unwrap();
        for leftover in RESTORE_LEFTOVERS {
            std::fs::write(dir.join(leftover), XPUB).unwrap();
        }
        std::fs::write(dir.join("unrelated.txt"), b"keep me").unwrap();
        state.grant_restore_file(dir.join("chosen.nakosbak"));
        // A later write that sits in the write-ahead log, not yet in the main file.
        {
            // Enough later writes that the log is far larger than anything the fresh vault writes.
            let mut conn = state.pool().unwrap().get().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute("CREATE TABLE filler (t TEXT)", []).unwrap();
            for i in 0..3000 {
                tx.execute("INSERT INTO filler (t) VALUES (?1)", params![format!("{i} {XPUB}")]).unwrap();
            }
            tx.commit().unwrap();
        }
        assert!(all_file_bytes(&dir).iter().any(|(_, b)| contains(b, XPUB)));

        let mut keychain_calls = 0;
        for wrong in ["", "erase", "ERASE ", " ERASE", "yes"] {
            let err = erase_all_data_inner(&state, wrong, || { keychain_calls += 1; Ok(()) }).unwrap_err();
            assert!(err.contains("type ERASE"), "{err}");
        }
        assert_eq!(keychain_calls, 0);
        {
            let conn = state.pool().unwrap().get().unwrap();
            assert!(db::meta_get(&conn, "kdf_params").unwrap().is_some(), "a refused erase changes nothing");
        }
        assert!(state.vault().unwrap().lock().master_key.is_some());
        assert!(dir.join("backups").exists());

        // Another command (a health or fee check) still holds a connection, so SQLite cannot fold
        // the write-ahead log back in and delete it on close: the erase has to remove it itself.
        let busy_pool = state.pool().unwrap();
        let busy = busy_pool.get().unwrap();
        assert!(dir.join("nakos.db-wal").exists());
        assert!(contains(&std::fs::read(dir.join("nakos.db-wal")).unwrap(), XPUB), "premise: the WAL holds the later write");
        // Windows will not delete a file that connection still has open: the erase waits, then
        // refuses before touching anything, and works once the other command is done.
        #[cfg(windows)]
        {
            assert!(database_in_use(&dir), "premise: the open connection holds the file");
            let err = erase_all_data_inner(&state, "ERASE", || { keychain_calls += 1; Ok(()) }).unwrap_err();
            assert_eq!(err, DB_STILL_IN_USE);
            assert_eq!(keychain_calls, 0);
            assert!(state.vault().unwrap().lock().master_key.is_some(), "a refused erase keeps the vault unlocked");
            assert!(state.restore_grant().is_ok(), "and keeps the chosen restore file");
            assert!(dir.join("backups").exists());
            for leftover in RESTORE_LEFTOVERS {
                assert!(dir.join(leftover).exists(), "{leftover} is still there");
            }
            let conn = state.pool().unwrap().get().unwrap();
            assert!(db::meta_get(&conn, "kdf_params").unwrap().is_some(), "the pool is back on the untouched vault");
            drop(conn);
            drop(busy);
            drop(busy_pool);
        }
        let summary = erase_all_data_inner(&state, "ERASE", || { keychain_calls += 1; Ok(()) }).unwrap();
        for (path, bytes) in all_file_bytes(&dir) {
            assert!(!contains(&bytes, XPUB), "{} still holds wallet data right after the erase", path.display());
        }
        // Elsewhere the erase goes ahead; the old connection writing to the old, deleted file must
        // not touch the new vault.
        #[cfg(not(windows))]
        {
            let _ = busy.execute("INSERT INTO filler (t) VALUES ('late write')", []);
            {
                let conn = state.pool().unwrap().get().unwrap();
                let ok: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
                assert_eq!(ok, "ok");
                let filler: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'filler'", [], |r| r.get(0)).unwrap();
                assert_eq!(filler, 0, "the old database's tables are not in the new one");
            }
            drop(busy);
            drop(busy_pool);
        }
        assert_eq!(keychain_calls, 1);
        assert!(summary.keychain_cleared);
        assert!(state.vault().unwrap().lock().master_key.is_none(), "the key is forgotten");
        assert!(state.restore_grant().is_err(), "the chosen restore file is forgotten");
        {
            let conn = state.pool().unwrap().get().unwrap();
            assert!(db::meta_get(&conn, "kdf_params").unwrap().is_none(), "the app is back at Create your vault");
            let wallets: i64 = conn.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).unwrap();
            let nodes: i64 = conn.query_row("SELECT COUNT(*) FROM node_configs", [], |r| r.get(0)).unwrap();
            assert_eq!((wallets, nodes), (0, 0));
            assert!(db::meta_get(&conn, "explorer_token_ct").unwrap().is_none());
        }
        assert!(!dir.join("backups").exists(), "local copies are deleted");
        for leftover in RESTORE_LEFTOVERS {
            assert!(!dir.join(leftover).exists(), "{leftover} is deleted");
        }
        for (path, bytes) in all_file_bytes(&dir) {
            assert!(!contains(&bytes, XPUB), "{} still holds wallet data", path.display());
        }
        assert_eq!(std::fs::read(dir.join("unrelated.txt")).unwrap(), b"keep me");

        // A keychain that will not let go is reported, not fatal.
        let again = erase_all_data_inner(&state, "ERASE", || Err("denied".into())).unwrap();
        assert!(!again.keychain_cleared);
    }

    fn make_vault(dir: &std::path::Path, passphrase: &str) -> MasterKey {
        let pool = db::open_pool(dir).unwrap();
        let conn = pool.get().unwrap();
        let (params, key) = crypto::init_passphrase(passphrase).unwrap();
        db::meta_set(&conn, "kdf_params", &serde_json::to_string(&params).unwrap()).unwrap();
        conn.execute(
            "INSERT INTO wallets (id, name, custody_type, source_type, xpub) VALUES ('w1', 'Savings', 'cold_storage', 'xpub', ?1)",
            params![XPUB],
        )
        .unwrap();
        let (nonce, ct) = key.encrypt(RPC_SECRET.as_bytes()).unwrap();
        conn.execute(
            "INSERT INTO node_configs (id, kind, label, host, port, rpc_user, rpc_password_encrypted, rpc_password_nonce)
             VALUES ('n1', 'bitcoin_core_rpc', 'core', 'node.example', 8332, 'user', ?1, ?2)",
            params![ct, nonce],
        )
        .unwrap();
        let (tnonce, tct) = key.encrypt(TOKEN_SECRET.as_bytes()).unwrap();
        db::meta_set(&conn, "explorer_token_ct", &STANDARD.encode(tct)).unwrap();
        db::meta_set(&conn, "explorer_token_nonce", &STANDARD.encode(tnonce)).unwrap();
        key
    }

    fn contains(hay: &[u8], needle: &str) -> bool {
        hay.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    /// A refused backup says which field holds the key-like text (never the text itself). Text that was
    /// edited away, or whose row was deleted (sync prunes spent coins with their labels), stops blocking:
    /// SQLite leaves a deleted row's text in the file's free space until the file is rebuilt.
    #[test]
    fn a_refused_backup_names_the_field_and_edited_away_text_stops_blocking() {
        const KEY_LIKE: &str = "xprvAAAAAAAAAAAAAAAAAAAAAAAA";
        let dir = temp_dir("keytext");
        let key = make_vault(&dir, PASS);
        let pool = db::open_pool(&dir).unwrap();
        let conn = pool.get().unwrap();
        conn.execute("UPDATE wallets SET notes = ?1 WHERE id = 'w1'", params![KEY_LIKE]).unwrap();

        let err = create_backup_file(&pool, &dir, &key).unwrap_err();
        assert!(err.contains("wallets.notes") && !err.contains(KEY_LIKE), "{err}");

        // The owner edits the note away: the backup works again.
        conn.execute("UPDATE wallets SET notes = 'clean' WHERE id = 'w1'", []).unwrap();
        create_backup_file(&pool, &dir, &key).expect("a backup works once the text is gone");

        // A coin label that looked like a key is deleted with its row (what a sync prune does). No live
        // field holds the text any more, but the old bytes stay in the file until it is rebuilt.
        conn.execute("INSERT INTO utxos (id, wallet_id, txid, vout, amount_sats, label) VALUES ('u1', 'w1', 'tx', 0, 1000, ?1)", params![KEY_LIKE]).unwrap();
        assert!(create_backup_file(&pool, &dir, &key).unwrap_err().contains("utxos.label"));
        conn.execute("DELETE FROM utxos WHERE id = 'u1'", []).unwrap();
        create_backup_file(&pool, &dir, &key).expect("a deleted row's leftover text must not block backups");

        // A field the app cannot edit says so.
        conn.execute("INSERT INTO multisig_configs (id, wallet_id, threshold_m, total_n) VALUES ('m1', 'w1', 2, 3)", []).unwrap();
        conn.execute("INSERT INTO multisig_cosigners (id, multisig_id, label, xpub) VALUES ('c1', 'm1', ?1, ?2)", params![KEY_LIKE, XPUB]).unwrap();
        let err = create_backup_file(&pool, &dir, &key).unwrap_err();
        assert!(err.contains("multisig_cosigners.label (can't be edited in Nakos)") && !err.contains(KEY_LIKE), "{err}");

        drop(conn);
        drop(pool);
    }

    #[test]
    fn backup_hides_secrets_and_restores_into_a_fresh_vault() {
        // --- source vault -> backup file
        let src = temp_dir("src");
        let key = make_vault(&src, PASS);
        let pool = db::open_pool(&src).unwrap();
        let file = create_backup_file(&pool, &src, &key).unwrap();
        drop(pool);

        // The backup bytes never contain the raw secrets, nor any plaintext wallet data.
        assert!(!contains(&file, RPC_SECRET), "backup contains the raw rpcpassword");
        assert!(!contains(&file, TOKEN_SECRET), "backup contains the raw explorer token");
        assert!(!contains(&file, "xpub661"), "backup leaks plaintext wallet data");
        assert!(!contains(&file, "SQLite format 3"), "backup is not sealed");
        // Even with the outer layer removed, the secrets are still sealed inside the database
        // while ordinary wallet data is plaintext at rest (documented in docs/THREAT_MODEL.md).
        let inner = crate::crypto::backup::open(&file, PASS).unwrap().db_bytes;
        assert!(!contains(&inner, RPC_SECRET) && !contains(&inner, TOKEN_SECRET), "secrets must be sealed at rest");
        assert!(contains(&inner, "xpub661MyMwAqRbc"), "wallet xpubs are plaintext at rest");

        // --- fresh destination vault (its own, different passphrase) -> restore
        let dst = temp_dir("dst");
        make_fresh_other_vault(&dst);
        let wrong = restore_backup_file(&dst, &file, "not the passphrase");
        assert!(wrong.is_err(), "wrong passphrase must fail");
        // A failed restore leaves the destination vault untouched and still unlockable.
        assert_still_other_vault(&dst);

        let restored_key = restore_backup_file(&dst, &file, PASS).unwrap();
        let pool = db::open_pool(&dst).unwrap();
        let conn = pool.get().unwrap();
        let names: Vec<String> = conn.prepare("SELECT name FROM wallets").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(names, vec!["Savings".to_string()], "wallet list matches");
        // Sealed fields decrypt after unlocking with the backup's passphrase.
        let params = read_kdf_params(&conn).unwrap();
        let unlocked = crypto::unlock(PASS, &params).expect("backup passphrase unlocks the restored vault");
        let (ct, nonce): (Vec<u8>, Vec<u8>) =
            conn.query_row("SELECT rpc_password_encrypted, rpc_password_nonce FROM node_configs WHERE id = 'n1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(unlocked.decrypt(&nonce, &ct).unwrap(), RPC_SECRET.as_bytes());
        assert_eq!(restored_key.decrypt(&nonce, &ct).unwrap(), RPC_SECRET.as_bytes(), "returned key is the working vault key");
        let tct = STANDARD.decode(db::meta_get(&conn, "explorer_token_ct").unwrap().unwrap()).unwrap();
        let tn = STANDARD.decode(db::meta_get(&conn, "explorer_token_nonce").unwrap().unwrap()).unwrap();
        assert_eq!(unlocked.decrypt(&tn, &tct).unwrap(), TOKEN_SECRET.as_bytes());
        // The replaced database was kept, and no stale WAL sidecars were left behind to replay.
        assert!(dst.join("nakos.db.pre-restore-bak").exists());
        // The scratch copy is a WAL-mode file opened read-only, which leaves -wal / -shm behind on
        // close (seen on Android on 2026-10-14); a restore must not leave any scratch file.
        let scratch: Vec<_> = std::fs::read_dir(&dst).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains("-tmp")).collect();
        assert!(scratch.is_empty(), "restore left {scratch:?}");
        drop(conn);
        drop(pool);
    }

    fn make_fresh_other_vault(dir: &std::path::Path) {
        let pool = db::open_pool(dir).unwrap();
        let conn = pool.get().unwrap();
        let (params, _key) = crypto::init_passphrase("another passphrase entirely").unwrap();
        db::meta_set(&conn, "kdf_params", &serde_json::to_string(&params).unwrap()).unwrap();
    }

    fn assert_still_other_vault(dir: &std::path::Path) {
        let pool = db::open_pool(dir).unwrap();
        let conn = pool.get().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        crypto::unlock("another passphrase entirely", &read_kdf_params(&conn).unwrap()).unwrap();
    }

    #[test]
    fn restore_refuses_family_letters_and_tampered_files_without_touching_the_vault() {
        let dst = temp_dir("refuse");
        make_fresh_other_vault(&dst);
        let letter = b"# A letter about my bitcoin\n\n```nakos-inherit-json\n{\"format_version\":2,\"wallets\":[]}\n```\n";
        let err = restore_backup_file(&dst, letter, PASS).err().expect("letters are refused");
        assert!(err.contains("family letter"), "{err}");

        let src = temp_dir("tamper");
        let key = make_vault(&src, PASS);
        let pool = db::open_pool(&src).unwrap();
        let mut file = create_backup_file(&pool, &src, &key).unwrap();
        drop(pool);
        file[20] ^= 1; // header byte: bound as associated data
        assert!(restore_backup_file(&dst, &file, PASS).is_err());
        assert_still_other_vault(&dst);
    }

    #[test]
    fn waiting_for_the_database_stops_when_it_is_free_or_the_time_is_up() {
        use std::time::{Duration, Instant};
        let mut polls = 0;
        assert!(wait_until_free(|| { polls += 1; false }, Duration::from_secs(5)));
        assert_eq!(polls, 1, "a free database is not waited for");
        let mut polls = 0;
        assert!(wait_until_free(|| { polls += 1; polls < 3 }, Duration::from_secs(5)));
        assert_eq!(polls, 3, "it asks again until the database is free");
        let mut polls = 0;
        let start = Instant::now();
        assert!(!wait_until_free(|| { polls += 1; true }, Duration::from_millis(200)));
        let took = start.elapsed();
        assert!(took >= Duration::from_millis(200) && took < Duration::from_secs(2), "{took:?}");
        assert!(polls > 1, "it asked more than once before giving up");
    }

    fn always_in_use(_: &std::path::Path) -> bool {
        true
    }

    /// A backup of a second vault (passphrase OTHER_PASS), as bytes.
    const OTHER_PASS: &str = "another passphrase entirely";
    fn other_vault_backup(tag: &str) -> (Vec<u8>, MasterKey) {
        let other = temp_dir(tag);
        let key = make_vault(&other, OTHER_PASS);
        let pool = db::open_pool(&other).unwrap();
        let file = create_backup_file(&pool, &other, &key).unwrap();
        drop(pool);
        (file, key)
    }

    /// While another task still has the database open (on Windows), a restore and an erase refuse
    /// before touching anything. A stand-in check plays that part here, on every platform.
    #[test]
    fn while_the_database_is_in_use_restore_and_erase_change_nothing() {
        let busy = Release { in_use: always_in_use, wait: std::time::Duration::from_millis(100) };
        // "Free" is the real check: always free off Windows; on Windows it also waits out the
        // connections a just-reopened pool is still opening in the background.
        let free = Release { in_use: database_in_use, wait: RELEASE.wait };
        let dir = temp_dir("in-use");
        let key = make_vault(&dir, PASS);
        let state = state_for(&dir, Some(MasterKey(key.0)));
        perform_local_backup(&dir, &state.pool().unwrap()).unwrap();
        state.grant_restore_file(dir.join("chosen.nakosbak"));
        let (file, other_key) = other_vault_backup("in-use-other");
        let unchanged = |state: &AppState| {
            assert_eq!(state.vault().unwrap().lock().master_key.as_ref().unwrap().0, key.0, "the vault stays unlocked with its own key");
            assert!(state.restore_grant().is_ok(), "the chosen restore file is kept");
            let conn = state.pool().unwrap().get().unwrap();
            crypto::unlock(PASS, &read_kdf_params(&conn).unwrap()).unwrap();
            let wallets: i64 = conn.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).unwrap();
            assert_eq!(wallets, 1);
            assert!(dir.join("backups").read_dir().unwrap().next().is_some(), "the local copies are still there");
            for scratch in ["nakos.db.pre-restore-bak", "nakos.db.restore-tmp", "nakos.db.verify-tmp"] {
                assert!(!dir.join(scratch).exists(), "{scratch} was written");
            }
        };

        let err = restore_from_with(&state, || Ok(file.clone()), OTHER_PASS, &busy).unwrap_err();
        assert_eq!(err, DB_STILL_IN_USE);
        unchanged(&state);
        // An older (format 1) file, sealed with this vault's own key, takes the other swap path.
        let v1 = {
            let src = temp_dir("in-use-v1");
            make_vault(&src, PASS);
            let db_bytes = std::fs::read(db::db_file_path(&src)).unwrap();
            let (nonce, ct) = key.encrypt(&db_bytes).unwrap();
            [sealed::MAGIC_V1, &nonce[..], &ct[..]].concat()
        };
        let err = restore_from_with(&state, || Ok(v1.clone()), PASS, &busy).unwrap_err();
        assert_eq!(err, DB_STILL_IN_USE);
        unchanged(&state);

        let mut keychain_calls = 0;
        let err = erase_all_data_with(&state, "ERASE", &busy, || { keychain_calls += 1; Ok(()) }).unwrap_err();
        assert_eq!(err, DB_STILL_IN_USE);
        assert_eq!(keychain_calls, 0, "the keychain is left alone too");
        unchanged(&state);

        // Once the database is free, both go ahead.
        assert_eq!(restore_from_with(&state, || Ok(v1.clone()), PASS, &free).unwrap().wallets, 1, "premise: the format 1 file restores once free");
        assert_eq!(state.vault().unwrap().lock().master_key.as_ref().unwrap().0, key.0, "a format 1 restore keeps the vault key");
        assert_eq!(restore_from_with(&state, || Ok(file.clone()), OTHER_PASS, &free).unwrap().wallets, 1);
        assert_eq!(state.vault().unwrap().lock().master_key.as_ref().unwrap().0, other_key.0, "the backup's key is now the vault key");
        erase_all_data_with(&state, "ERASE", &free, || { keychain_calls += 1; Ok(()) }).unwrap();
        assert_eq!(keychain_calls, 1);
        assert!(state.vault().unwrap().lock().master_key.is_none());
        assert!(db::meta_get(&state.pool().unwrap().get().unwrap(), "kdf_params").unwrap().is_none());
    }

    /// The real check on Windows: a connection another task still holds keeps a restore off the
    /// database, with nothing written, until it is closed.
    #[cfg(windows)]
    #[test]
    fn on_windows_an_open_connection_holds_off_a_restore_until_it_closes() {
        let dir = temp_dir("win-restore");
        let key = make_vault(&dir, PASS);
        let state = state_for(&dir, Some(MasterKey(key.0)));
        let (file, other_key) = other_vault_backup("win-restore-other");
        let busy_pool = state.pool().unwrap();
        let busy = busy_pool.get().unwrap();
        assert!(database_in_use(&dir));
        let err = restore_from(&state, || Ok(file.clone()), OTHER_PASS).unwrap_err();
        assert_eq!(err, DB_STILL_IN_USE);
        assert_eq!(state.vault().unwrap().lock().master_key.as_ref().unwrap().0, key.0);
        for scratch in ["nakos.db.pre-restore-bak", "nakos.db.restore-tmp", "nakos.db.verify-tmp"] {
            assert!(!dir.join(scratch).exists(), "{scratch} was written");
        }
        let wallets: i64 = busy.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).unwrap();
        assert_eq!(wallets, 1, "the open connection still sees the untouched vault");
        drop(busy);
        drop(busy_pool);
        assert_eq!(restore_from(&state, || Ok(file.clone()), OTHER_PASS).unwrap().wallets, 1);
        assert_eq!(state.vault().unwrap().lock().master_key.as_ref().unwrap().0, other_key.0);
        drop(state);
        // The pool's background threads may still be opening connections for a moment, which is
        // why the app waits instead of asking once.
        assert!(wait_until_free(|| database_in_use(&dir), RELEASE.wait), "with every connection closed the file is free");
    }
}

#[cfg(test)]
mod platform_tests {
    use super::{keychain_off_reason, platform_name};

    #[test]
    fn the_keychain_refusal_names_android_or_development_builds() {
        assert_eq!(keychain_off_reason("android"), "Nakos on Android doesn't save the passphrase on the phone");
        assert_eq!(keychain_off_reason("macos"), "the OS keychain isn't used in development builds");
    }

    /// The OS-dependent lines differ only on a device, so the host checks they read the real OS.
    #[test]
    fn the_real_os_is_what_the_gate_and_the_ui_are_given() {
        let fn_body = |src: &'static str, sig: &str| -> &'static str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig}"));
            let end = start + src[start..].find("\n}\n").unwrap();
            &src[start..end]
        };
        let crypto = include_str!("../crypto/mod.rs");
        let backup = include_str!("backup.rs");
        assert!(fn_body(crypto, "pub fn keychain_enabled()").contains("has_os_keychain(std::env::consts::OS)"));
        assert!(fn_body(backup, "pub fn vault_status(").contains("platform_name(std::env::consts::OS)"));
        assert!(fn_body(backup, "pub fn vault_status(").contains("let os = std::env::consts::OS;"));
        assert!(fn_body(backup, "pub fn set_keychain_helper(").contains("keychain_off_reason(std::env::consts::OS)"));
    }

    /// Turning the helper off, any lock, and Erase all data each remove the remembered
    /// passphrase from the OS keychain. The commands need a running app, so the host checks the
    /// calls in the source (the erase's own test checks the clear runs, after the files are gone).
    #[test]
    fn turning_the_helper_off_forgetting_it_and_erasing_clear_the_keychain() {
        let fn_body = |src: &'static str, sig: &str| -> &'static str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig}"));
            let end = start + src[start..].find("\n}\n").unwrap();
            &src[start..end]
        };
        let backup = include_str!("backup.rs");
        let helper = fn_body(backup, "pub fn set_keychain_helper(");
        let off = &helper[helper.find("} else {").expect("an off branch")..];
        let flag = off.find("db::meta_set(&conn, KEYCHAIN_OPT_IN_META, \"0\")").expect("off clears the opt-in flag");
        let cleared = off.find("crypto::keychain_clear()").expect("off clears the keychain");
        assert!(flag < cleared, "{off}");
        // Every lock forgets: the button goes through the same `lock_now` as the auto-lock.
        assert!(fn_body(backup, "pub fn vault_lock(").contains("super::autolock::lock_now(&state)"));
        let forget = fn_body(backup, "pub(crate) fn forget_remembered_passphrase(");
        let asked = forget.find("if !keychain_opted_in(&conn)").expect("only someone who opted in is touched");
        let flag = forget.find("db::meta_set(&conn, KEYCHAIN_OPT_IN_META, \"0\")").expect("the flag goes first");
        let cleared = forget.find("crypto::keychain_clear()").expect("then the item");
        assert!(asked < flag && flag < cleared, "{forget}");
        assert!(fn_body(backup, "pub fn erase_all_data(").contains("crypto::keychain_clear()"));
        let crypto = include_str!("../crypto/mod.rs");
        assert!(fn_body(crypto, "pub fn keychain_clear()").contains("keychain_clear_in(KEYRING_SERVICE)"));
        assert!(fn_body(crypto, "pub fn keychain_store_passphrase(").contains("keychain_store_in(KEYRING_SERVICE, passphrase)"));
        assert!(fn_body(crypto, "pub fn keychain_load_passphrase(").contains("keychain_load_from(KEYRING_SERVICE)"));
    }

    /// A power cut can't be staged in a test, so the host checks the restore swap goes through the
    /// syncing helpers: the new file and the kept copy are on disk before the rename, the folder
    /// is synced before it (so removed -wal / -shm files can't come back and replay onto the new
    /// database) and after it (so the rename itself is kept). The export does the same for the
    /// file the person chose.
    #[test]
    fn the_restore_swap_and_the_export_are_synced_to_disk() {
        let fn_body = |src: &'static str, sig: &str| -> &'static str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig}"));
            let end = start + src[start..].find("\n}\n").unwrap();
            &src[start..end]
        };
        let swap = fn_body(include_str!("backup.rs"), "pub fn swap_in_database(");
        let at = |needle: &str| swap.find(needle).unwrap_or_else(|| panic!("swap_in_database lacks {needle}"));
        let write = at("durable::write_synced(&tmp, db_bytes)");
        let copy = at("durable::copy_synced(&db_path, &db_path.with_extension(\"db.pre-restore-bak\"))");
        let wal = at("for suffix in [\"-wal\", \"-shm\"]");
        let rename = at("std::fs::rename(&tmp, &db_path)");
        let before = at("durable::sync_dir(app_data_dir)");
        let after = rename + swap[rename..].find("durable::sync_dir(app_data_dir)").expect("the folder is synced after the rename");
        assert!(write < rename && copy < rename && wal < before && before < rename && rename < after, "{swap}");
        assert!(!swap.contains("std::fs::write(") && !swap.contains("std::fs::copy("), "unsynced write in {swap}");
        let export = fn_body(include_str!("file_dialog.rs"), "fn write_all(");
        assert!(export.contains("durable::sync_file(&f)"), "{export}");
    }

    /// `pickBackupToRestore` can fail (a provider that won't hand over the file, a closed dialog);
    /// the dialog has to say so instead of doing nothing.
    #[test]
    fn a_failed_file_choice_is_shown_in_the_restore_dialog() {
        let tsx = include_str!("../../../src/components/restore-backup-dialog.tsx");
        let start = tsx.find("const choose = async () => {").expect("choose()");
        let choose = &tsx[start..start + tsx[start..].find("\n  };\n").unwrap()];
        let tried = choose.find("await api.pickBackupToRestore()").expect("picks through Rust");
        let caught = choose.find("} catch (e) {").expect("choose() catches a failed pick");
        assert!(tried < caught && choose[caught..].contains("setError(String(e));"), "{choose}");
        assert!(tsx.contains("{error && (") && tsx.contains("{error}"), "the error line is still rendered");
    }

    #[test]
    fn the_ui_is_told_android_ios_or_desktop() {
        assert_eq!(platform_name("android"), "android");
        assert_eq!(platform_name("ios"), "ios");
        for os in ["macos", "windows", "linux"] {
            assert_eq!(platform_name(os), "desktop", "{os}");
        }
        assert_eq!(platform_name(std::env::consts::OS), "desktop", "this test runs on a desktop host");
    }
}

#[cfg(test)]
mod version_tests {
    /// Cargo.toml is the single source of truth for the app version (tauri.conf.json
    /// has no `version`, so Tauri uses it). package.json must not drift from it.
    #[test]
    fn package_json_version_matches_cargo() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../package.json");
        let pkg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(pkg["version"].as_str().unwrap(), env!("CARGO_PKG_VERSION"), "bump package.json to match Cargo.toml");
        let conf = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json")).unwrap();
        let conf: serde_json::Value = serde_json::from_str(&conf).unwrap();
        assert!(conf.get("version").is_none(), "tauri.conf.json must not carry its own version");
    }
}
