//! Auto-lock. The vault key leaves memory after a stretch of inactivity, and on a phone shortly
//! after the app goes to the background.
//!
//! The clock runs here in Rust and not in the webview. A minimised window or a backgrounded phone
//! throttles or freezes webview timers, and the lock has to happen anyway. The webview only
//! reports activity (`vault_touch`, at most every few seconds) and visibility
//! (`vault_app_hidden`). Each of these checks the deadline first, so activity that arrives after a
//! long sleep locks the vault rather than saving it.
//!
//! Any lock, manual or automatic, also forgets the passphrase remembered in the OS keychain:
//! "Remember" lasts only while the session is unlocked (see `lock_now`).

use super::backup::forget_remembered_passphrase;
use crate::db;
use crate::state::AppState;
use parking_lot::Mutex;
use serde::Serialize;
use std::time::{Duration, Instant, SystemTime};
use tauri::{Emitter, Manager};

pub(crate) const AUTO_LOCK_PREF: &str = "auto_lock_minutes";
pub(crate) const CHOICES: [u32; 7] = [0, 1, 5, 10, 15, 30, 60];
/// How long a phone may sit in the background before the vault locks.
pub(crate) const BACKGROUND_GRACE: Duration = Duration::from_secs(30);
/// The event the webview listens for; its payload is the reason ("idle", "background", "manual").
pub const LOCKED_EVENT: &str = "vault-locked";

pub(crate) fn default_minutes(os: &str) -> u32 {
    if os == "android" || os == "ios" {
        5
    } else {
        10
    }
}

fn is_phone(os: &str) -> bool {
    os == "android" || os == "ios"
}

/// A point in time measured on both clocks. On Android the monotonic clock stops while the phone
/// sleeps, so it alone would let a phone left overnight count as only a few seconds idle. The wall
/// clock keeps running. The larger of the two counts, so changing the wall clock can only lock
/// sooner, never later.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stamp {
    mono: Instant,
    wall: SystemTime,
}

impl Stamp {
    pub(crate) fn now() -> Self {
        Stamp { mono: Instant::now(), wall: SystemTime::now() }
    }
    pub(crate) fn elapsed(&self) -> Duration {
        let wall = SystemTime::now().duration_since(self.wall).unwrap_or(Duration::ZERO);
        self.mono.elapsed().max(wall)
    }
}

#[derive(Debug)]
pub struct AutoLock {
    inner: Mutex<AutoLockInner>,
}

#[derive(Debug)]
struct AutoLockInner {
    minutes: u32,
    last_activity: Stamp,
    hidden_since: Option<Stamp>,
}

impl Default for AutoLock {
    fn default() -> Self {
        AutoLock {
            inner: Mutex::new(AutoLockInner { minutes: default_minutes(std::env::consts::OS), last_activity: Stamp::now(), hidden_since: None }),
        }
    }
}

/// Why the vault should lock now, if it should.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Due {
    Idle,
    Background,
}

impl Due {
    fn reason(self) -> &'static str {
        match self {
            Due::Idle => "idle",
            Due::Background => "background",
        }
    }
}

impl AutoLock {
    pub(crate) fn set_minutes(&self, minutes: u32) {
        self.inner.lock().minutes = minutes;
    }
    pub(crate) fn minutes(&self) -> u32 {
        self.inner.lock().minutes
    }
    /// Starts a fresh idle period (on unlock, and on real activity).
    pub(crate) fn reset(&self) {
        let mut i = self.inner.lock();
        i.last_activity = Stamp::now();
        i.hidden_since = None;
    }
    pub(crate) fn touch(&self) {
        self.inner.lock().last_activity = Stamp::now();
    }
    pub(crate) fn set_hidden(&self, hidden: bool) {
        let mut i = self.inner.lock();
        if hidden {
            i.hidden_since.get_or_insert_with(Stamp::now);
        } else {
            i.hidden_since = None;
            i.last_activity = Stamp::now();
        }
    }
    /// Pure check, given the OS and how idle/hidden the session has been.
    pub(crate) fn due(&self, os: &str) -> Option<Due> {
        let i = self.inner.lock();
        due_for(os, i.minutes, i.last_activity.elapsed(), i.hidden_since.map(|h| h.elapsed()))
    }
}

pub(crate) fn due_for(os: &str, minutes: u32, idle: Duration, hidden: Option<Duration>) -> Option<Due> {
    if is_phone(os) && hidden.is_some_and(|h| h >= BACKGROUND_GRACE) {
        return Some(Due::Background);
    }
    if minutes > 0 && idle >= Duration::from_secs(u64::from(minutes) * 60) {
        return Some(Due::Idle);
    }
    None
}

/// Locks the vault: the key is dropped and a remembered passphrase is removed from the OS
/// keychain. Returns whether the vault was unlocked before.
pub(crate) fn lock_now(state: &AppState) -> Result<bool, String> {
    let vault = state.vault()?;
    let was_unlocked = vault.lock().master_key.take().is_some();
    // Even when the key was already gone: "Remember" never outlives a lock.
    forget_remembered_passphrase(state)?;
    Ok(was_unlocked)
}

/// Locks if a deadline has passed, and tells the webview. Used by the watchdog and by every
/// activity report, so late activity cannot rescue an expired session.
pub(crate) fn enforce(app: &tauri::AppHandle, state: &AppState) -> Result<bool, String> {
    let unlocked = state.vault()?.lock().master_key.is_some();
    if !unlocked {
        return Ok(false);
    }
    let Some(due) = state.auto_lock().due(std::env::consts::OS) else {
        return Ok(false);
    };
    // `lock_now` drops the key before it touches the OS keychain, so the vault is locked even when
    // removing a remembered passphrase fails (no Secret Service on Linux, a refused prompt). The
    // webview is told either way, or it would keep showing balances for a locked vault.
    let locked = lock_now(state);
    let _ = app.emit(LOCKED_EVENT, due.reason());
    locked.map(|_| true)
}

/// Reads the saved timeout into memory. Called once the database is open.
pub(crate) fn load_setting(state: &AppState) {
    let minutes = state
        .pool()
        .ok()
        .and_then(|pool| pool.get().ok())
        .and_then(|conn| db::pref_get(&conn, AUTO_LOCK_PREF).ok().flatten())
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|m| CHOICES.contains(m))
        .unwrap_or_else(|| default_minutes(std::env::consts::OS));
    state.auto_lock().set_minutes(minutes);
}

/// The watchdog: checks every few seconds for the life of the app.
pub fn spawn_watchdog(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("nakos-autolock".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            let state = app.state::<AppState>();
            let _ = enforce(&app, &state);
        })
        .expect("start the auto-lock watchdog");
}

#[derive(Debug, Serialize)]
pub struct AutoLockSettings {
    pub minutes: u32,
    pub default_minutes: u32,
    pub choices: Vec<u32>,
    pub background_lock_seconds: Option<u64>,
}

fn settings_of(minutes: u32) -> AutoLockSettings {
    let os = std::env::consts::OS;
    AutoLockSettings {
        minutes,
        default_minutes: default_minutes(os),
        choices: CHOICES.to_vec(),
        background_lock_seconds: is_phone(os).then_some(BACKGROUND_GRACE.as_secs()),
    }
}

/// Activity in the window. Locks instead if the deadline already passed.
#[tauri::command]
pub fn vault_touch(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    if !enforce(&app, &state)? {
        state.auto_lock().touch();
    }
    Ok(())
}

/// The app went to the background (`hidden = true`) or came back. Coming back after the grace
/// period locks before anything else happens.
#[tauri::command]
pub fn vault_app_hidden(app: tauri::AppHandle, state: tauri::State<AppState>, hidden: bool) -> Result<(), String> {
    if hidden {
        state.auto_lock().set_hidden(true);
        return Ok(());
    }
    enforce(&app, &state)?;
    state.auto_lock().set_hidden(false);
    Ok(())
}

#[tauri::command]
pub fn get_auto_lock(state: tauri::State<AppState>) -> Result<AutoLockSettings, String> {
    Ok(settings_of(state.auto_lock().minutes()))
}

#[tauri::command]
pub fn set_auto_lock(state: tauri::State<AppState>, minutes: u32) -> Result<AutoLockSettings, String> {
    if !CHOICES.contains(&minutes) {
        return Err(format!("choose one of {CHOICES:?} minutes"));
    }
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    db::pref_set(&conn, AUTO_LOCK_PREF, &minutes.to_string()).map_err(|e| e.to_string())?;
    state.auto_lock().set_minutes(minutes);
    state.auto_lock().touch();
    Ok(settings_of(minutes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    /// Found on the first Linux build: with no Secret Service, removing a remembered passphrase
    /// fails. The key is already gone by then, and the webview must still hear about the lock.
    #[test]
    fn the_webview_hears_about_a_lock_even_when_the_keychain_fails() {
        let src = include_str!("autolock.rs");
        let start = src.find("pub(crate) fn enforce(").unwrap();
        let body = &src[start..start + src[start..].find("\n}\n").unwrap()];
        let lock = body.find("let locked = lock_now(state);").expect("the lock result is kept, not `?`-returned");
        let emit = body.find("app.emit(LOCKED_EVENT").expect("emits");
        assert!(lock < emit && !body.contains("lock_now(state)?"), "{body}");
        let lock_now_body = &src[src.find("pub(crate) fn lock_now(").unwrap()..];
        let take = lock_now_body.find("master_key.take()").unwrap();
        let forget = lock_now_body.find("forget_remembered_passphrase(state)").unwrap();
        assert!(take < forget, "the key goes before the keychain is touched");
    }

    #[test]
    fn desktop_locks_after_the_chosen_idle_time_and_never_when_off() {
        assert_eq!(due_for("macos", 10, MIN * 9, None), None);
        assert_eq!(due_for("macos", 10, MIN * 10, None), Some(Due::Idle));
        assert_eq!(due_for("windows", 1, MIN, None), Some(Due::Idle));
        assert_eq!(due_for("linux", 0, MIN * 10_000, None), None, "off means off");
    }

    #[test]
    fn a_hidden_desktop_window_keeps_only_the_idle_rule() {
        // Minimised on a desktop: no background lock, the idle timer still runs.
        assert_eq!(due_for("macos", 10, MIN, Some(MIN * 60)), None);
        assert_eq!(due_for("macos", 10, MIN * 10, Some(MIN)), Some(Due::Idle));
    }

    #[test]
    fn a_phone_locks_thirty_seconds_after_going_to_the_background_even_with_idle_off() {
        assert_eq!(due_for("android", 0, Duration::ZERO, Some(Duration::from_secs(29))), None);
        assert_eq!(due_for("android", 0, Duration::ZERO, Some(BACKGROUND_GRACE)), Some(Due::Background));
        assert_eq!(due_for("android", 5, MIN * 5, None), Some(Due::Idle));
    }

    #[test]
    fn defaults_are_ten_minutes_on_desktop_and_five_on_a_phone() {
        assert_eq!(default_minutes("macos"), 10);
        assert_eq!(default_minutes("windows"), 10);
        assert_eq!(default_minutes("linux"), 10);
        assert_eq!(default_minutes("android"), 5);
        assert!(CHOICES.contains(&default_minutes("macos")) && CHOICES.contains(&default_minutes("android")));
    }

    #[test]
    fn coming_back_resets_the_background_clock_but_hiding_twice_keeps_the_first_time() {
        let a = AutoLock::default();
        a.set_minutes(0);
        a.set_hidden(true);
        let first = a.inner.lock().hidden_since.unwrap().mono;
        a.set_hidden(true);
        assert_eq!(a.inner.lock().hidden_since.unwrap().mono, first);
        a.set_hidden(false);
        assert!(a.inner.lock().hidden_since.is_none());
    }

    #[test]
    fn the_wall_clock_counts_when_the_monotonic_clock_slept() {
        // A phone asleep overnight: the monotonic clock barely moved, the wall clock did.
        let s = Stamp { mono: Instant::now(), wall: SystemTime::now() - MIN * 60 * 8 };
        assert!(s.elapsed() >= MIN * 60 * 8);
    }

    #[test]
    fn lock_now_drops_the_key_and_forgets_the_remembered_passphrase() {
        let src = include_str!("autolock.rs");
        let body = &src[src.find("pub(crate) fn lock_now(").unwrap()..];
        let body = &body[..body.find("\n}\n").unwrap()];
        assert!(body.contains(".master_key.take()"), "{body}");
        assert!(body.contains("forget_remembered_passphrase(state)?"), "{body}");
    }

    #[test]
    fn every_activity_report_enforces_the_deadline_first() {
        let src = include_str!("autolock.rs");
        for f in ["pub fn vault_touch(", "pub fn vault_app_hidden("] {
            let body = &src[src.find(f).unwrap()..];
            let body = &body[..body.find("\n}\n").unwrap()];
            let enforce_at = body.find("enforce(&app, &state)").unwrap_or_else(|| panic!("{f} enforces"));
            let reset_at = body.find("touch()").or_else(|| body.find("set_hidden(false)")).unwrap();
            assert!(enforce_at < reset_at, "{f}: the deadline is checked before activity counts");
        }
    }
}
