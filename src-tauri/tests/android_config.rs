//! `tauri.android.conf.json` is merged over `tauri.conf.json` (JSON merge patch: an array replaces
//! the desktop one, `null` removes a key). Android gets no tray and none of the desktop window keys.

use std::path::PathBuf;

fn conf(rel: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

#[test]
fn android_drops_the_tray_and_the_desktop_window_keys() {
    let android = conf("tauri.android.conf.json");
    let app = android["app"].as_object().expect("app");
    assert!(app.contains_key("trayIcon") && app["trayIcon"].is_null(), "trayIcon must be null to remove it");
    let windows = app["windows"].as_array().expect("windows replaces the desktop array");
    assert_eq!(windows.len(), 1);
    let window = windows[0].as_object().unwrap();
    for key in ["width", "height", "minWidth", "minHeight", "titleBarStyle", "hiddenTitle", "resizable", "fullscreen"] {
        assert!(!window.contains_key(key), "{key} is desktop-only");
    }
    assert_eq!(window["title"], "Nakos");
    assert_eq!(window["theme"], "Dark");
}

/// The APK's versionName comes from `version`; pointing it at package.json keeps one version for
/// both builds (package.json is held equal to Cargo.toml by `package_json_version_matches_cargo`).
/// Desktop has no `version` in its config and takes Cargo.toml's.
#[test]
fn android_takes_its_version_from_package_json() {
    assert_eq!(conf("tauri.android.conf.json")["version"], "../package.json");
    assert!(conf("tauri.conf.json").get("version").is_none());
    let pkg = conf("../package.json");
    let v = pkg["version"].as_str().unwrap();
    assert_eq!(v, env!("CARGO_PKG_VERSION"));
}

#[test]
fn signing_keys_are_ignored_by_git() {
    let ignore = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.gitignore")).unwrap();
    for pattern in ["*.jks", "*.keystore"] {
        assert!(ignore.lines().any(|l| l.trim() == pattern), "{pattern} not in .gitignore");
    }
}

#[test]
fn desktop_keeps_its_tray_and_window() {
    let desktop = conf("tauri.conf.json");
    assert!(desktop["app"]["trayIcon"].is_object());
    assert_eq!(desktop["app"]["windows"][0]["minWidth"], 1024);
    assert_eq!(desktop["app"]["windows"][0]["titleBarStyle"], "Overlay");
}
