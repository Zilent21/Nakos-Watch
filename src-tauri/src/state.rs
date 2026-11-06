use crate::db::DbPool;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{App, Manager};
use tauri_plugin_dialog::FilePath;

pub struct VaultState {
    pub master_key: Option<crate::crypto::MasterKey>,
}

#[derive(Default)]
pub struct AppState {
    pub inner: Mutex<AppStateInner>,
}

#[derive(Default)]
pub struct AppStateInner {
    pub pool: Option<DbPool>,
    pub app_data_dir: Option<PathBuf>,
    pub vault: Option<Arc<Mutex<VaultState>>>,
    /// The backup file the person chose in the native dialog (`pick_backup_to_restore`). A restore
    /// reads this and nothing the webview names. A path on desktop, a `content://` URI on Android.
    pub restore_grant: Option<FilePath>,
    /// Taken by each wallet sync on its background thread, so syncs run one at a time as they
    /// did when sync ran on the main thread (their write transactions would otherwise collide).
    pub sync_lock: Arc<Mutex<()>>,
    /// Inactivity and background deadlines (`commands::autolock`).
    pub auto_lock: Arc<crate::commands::AutoLock>,
}

impl AppState {
    pub fn pool(&self) -> Result<DbPool, String> {
        self.inner
            .lock()
            .pool
            .clone()
            .ok_or_else(|| "database not initialized".to_string())
    }

    pub fn vault(&self) -> Result<Arc<Mutex<VaultState>>, String> {
        self.inner
            .lock()
            .vault
            .clone()
            .ok_or_else(|| "vault not initialized".to_string())
    }

    pub fn auto_lock(&self) -> Arc<crate::commands::AutoLock> {
        self.inner.lock().auto_lock.clone()
    }

    pub fn sync_lock(&self) -> Arc<Mutex<()>> {
        self.inner.lock().sync_lock.clone()
    }

    pub fn grant_restore_file(&self, file: impl Into<FilePath>) {
        self.inner.lock().restore_grant = Some(file.into());
    }

    /// The chosen backup file, if one was chosen. It stays until a restore succeeds, so a wrong
    /// passphrase can be retried without choosing the file again.
    pub fn restore_grant(&self) -> Result<FilePath, String> {
        self.inner.lock().restore_grant.clone().ok_or_else(|| "choose a backup file first".to_string())
    }

    pub fn clear_restore_grant(&self) {
        self.inner.lock().restore_grant = None;
    }

    pub fn app_data_dir(&self) -> Result<PathBuf, String> {
        self.inner
            .lock()
            .app_data_dir
            .clone()
            .ok_or_else(|| "app data dir not set".to_string())
    }
}

/// Whether the vault lives in the local app data folder instead of the usual one. Windows only:
/// there the usual folder is Roaming AppData, which a company network with roaming profiles copies
/// to its servers and to every PC the person signs into, and the database holds plaintext xpubs
/// and addresses. Local AppData stays on this PC. Elsewhere the folder is unchanged.
pub(crate) fn uses_local_app_data(os: &str) -> bool {
    os == "windows"
}

pub fn init(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let app_data_dir = if uses_local_app_data(std::env::consts::OS) {
        app.path().app_local_data_dir()?
    } else {
        app.path().app_data_dir()?
    };
    let pool = crate::db::open_pool(&app_data_dir)?;

    let state = app.state::<AppState>();
    let mut inner = state.inner.lock();
    inner.app_data_dir = Some(app_data_dir);
    inner.pool = Some(pool);
    inner.vault = Some(Arc::new(Mutex::new(VaultState { master_key: None })));

    // Best-effort automatic backup on every launch (rotated, capped locally).
    // Failure here (e.g. very first run with an empty db) must never block startup.
    if let Some(pool) = &inner.pool {
        let _ = crate::commands::perform_local_backup(inner.app_data_dir.as_ref().unwrap(), pool);
    }

    Ok(())
}

#[cfg(test)]
mod data_folder_tests {
    use super::uses_local_app_data;

    #[test]
    fn only_windows_keeps_the_vault_in_local_app_data() {
        assert!(uses_local_app_data("windows"));
        for os in ["macos", "linux", "android", "ios"] {
            assert!(!uses_local_app_data(os), "{os} keeps its folder");
        }
    }

    /// `init` needs a running app, so the host checks it asks for the real OS and picks the local
    /// folder only when told to.
    #[test]
    fn init_picks_the_local_folder_from_the_real_os() {
        let src = include_str!("state.rs");
        let start = src.find("pub fn init(").unwrap();
        let init = &src[start..start + src[start..].find("\n}\n").unwrap()];
        let choice = init.find("if uses_local_app_data(std::env::consts::OS) {").expect("init asks for the real OS");
        let local = init.find("app.path().app_local_data_dir()?").expect("the local folder");
        let other = init.find("} else {\n        app.path().app_data_dir()?").expect("the usual folder otherwise");
        assert!(choice < local && local < other, "{init}");
        assert_eq!(init.matches("app_data_dir()?").count() + init.matches("app_local_data_dir()?").count(), 2, "{init}");
    }
}
