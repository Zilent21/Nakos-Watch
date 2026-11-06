//! `tauri.windows.conf.json` is merged over `tauri.conf.json` when the target is Windows (also when
//! cross-building from a Mac: tauri-build picks it from the target triple). Windows gets no tray (the
//! icon is a macOS template image and has no menu), a per-user NSIS installer that needs no
//! administrator, and the WebView2 bootstrapper from the desktop config. No signing is configured.

use std::path::PathBuf;

fn conf(rel: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

#[test]
fn windows_drops_the_tray_and_installs_per_user_without_signing() {
    let win = conf("tauri.windows.conf.json");
    let app = win["app"].as_object().expect("app");
    assert!(app.contains_key("trayIcon") && app["trayIcon"].is_null(), "trayIcon must be null to remove it");
    assert!(!app.contains_key("windows"), "the desktop window settings apply unchanged");

    assert_eq!(win["bundle"]["publisher"], "Nakos");
    let windows = win["bundle"]["windows"].as_object().expect("bundle.windows");
    assert_eq!(windows["nsis"]["installMode"], "currentUser");
    // The installer and uninstaller show the Nakos icon, not NSIS's default one.
    assert_eq!(windows["nsis"]["installerIcon"], "icons/icon.ico");
    assert_eq!(windows["nsis"]["uninstallerIcon"], "icons/icon.ico");
    let bundle_icons = conf("tauri.conf.json")["bundle"]["icon"].as_array().unwrap().clone();
    assert!(bundle_icons.iter().any(|i| i == "icons/icon.ico"), "the app's own icon is the same file");
    let ico = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons/icon.ico")).unwrap();
    assert_eq!(&ico[..4], &[0, 0, 1, 0], "an .ico file");
    let sizes: Vec<u32> = (0..u16::from_le_bytes([ico[4], ico[5]]) as usize).map(|i| match ico[6 + 16 * i] { 0 => 256, w => w as u32 }).collect();
    for want in [16, 24, 32, 48, 64, 256] {
        assert!(sizes.contains(&want), "{want} px is missing from icon.ico: {sizes:?}");
    }
    // The uninstaller's "delete app data" box says it deletes the vault, from Nakos's own copy of
    // Tauri's English messages.
    assert_eq!(windows["nsis"]["languages"], serde_json::json!(["English"]));
    assert_eq!(windows["nsis"]["customLanguageFiles"]["English"], "windows/English.nsh");
    for key in ["certificateThumbprint", "signCommand", "timestampUrl", "webviewInstallMode"] {
        assert!(!windows.contains_key(key), "{key} must not be set here");
    }

    let desktop = conf("tauri.conf.json");
    assert_eq!(desktop["bundle"]["windows"]["webviewInstallMode"]["type"], "downloadBootstrapper");
    assert!(desktop["bundle"]["windows"].get("certificateThumbprint").is_none());
}

#[test]
fn the_uninstaller_says_the_box_deletes_the_vault() {
    let raw = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("windows/English.nsh")).unwrap();
    // Plain UTF-8 without a BOM: the bundler writes its own BOM in front, and two make makensis
    // stop at line 1 ("Invalid command"), as a build did.
    assert!(!raw.starts_with(b"\xef\xbb\xbf"), "no BOM; the bundler adds one");
    let text = String::from_utf8(raw).unwrap();
    let strings: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|l| l.strip_prefix("LangString "))
        .map(|l| {
            let (key, rest) = l.split_once(' ').unwrap();
            let value = rest.strip_prefix("${LANG_ENGLISH} \"").and_then(|v| v.strip_suffix('"')).unwrap_or_else(|| panic!("{l}"));
            (key, value)
        })
        .collect();
    // Every message Tauri 2's template uses, so none falls back to an empty string.
    let keys: Vec<&str> = strings.iter().map(|(k, _)| *k).collect();
    for want in [
        "addOrReinstall", "alreadyInstalled", "alreadyInstalledLong", "appRunning", "appRunningOkKill", "chooseMaintenanceOption",
        "choowHowToInstall", "createDesktop", "dontUninstall", "dontUninstallDowngrade", "failedToKillApp", "installingWebview2",
        "newerVersionInstalled", "older", "olderOrUnknownVersionInstalled", "silentDowngrades", "unableToUninstall", "uninstallApp",
        "uninstallBeforeInstalling", "unknown", "webview2AbortError", "webview2DownloadError", "webview2DownloadSuccess",
        "webview2Downloading", "webview2InstallError", "webview2InstallSuccess", "deleteAppData",
    ] {
        assert_eq!(keys.iter().filter(|k| **k == want).count(), 1, "{want} once");
    }
    let delete = strings.iter().find(|(k, _)| *k == "deleteAppData").unwrap().1;
    assert!(delete.contains("vault") && delete.contains("cannot be undone"), "{delete}");
    assert!(delete.len() <= 55, "the checkbox has one line of about 60 characters: {delete}");
    for (key, value) in &strings {
        assert!(!value.contains("{{"), "{key} shows a raw placeholder: {value}");
    }
    let running = strings.iter().find(|(k, _)| *k == "appRunning").unwrap().1;
    assert!(running.starts_with("${PRODUCTNAME} is running"), "{running}");
}
