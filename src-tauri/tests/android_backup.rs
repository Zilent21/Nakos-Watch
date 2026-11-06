//! The Android project under `gen/android` is edited by hand after `tauri android init`. These tests
//! read its files so a regenerated or edited project cannot quietly send the vault to a backup.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn identifier(conf: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(&read(conf)).unwrap();
    v["identifier"].as_str().unwrap_or_default().to_string()
}

/// Every app data domain Android's backup rules know about.
const DOMAINS: [&str; 9] = [
    "root",
    "file",
    "database",
    "sharedpref",
    "external",
    "device_root",
    "device_file",
    "device_database",
    "device_sharedpref",
];

#[test]
fn android_is_org_nakos_app_and_desktop_keeps_com_nakos_app() {
    assert_eq!(identifier("tauri.android.conf.json"), "org.nakos.app");
    // Changing the desktop identifier would orphan existing vaults and keychain items.
    assert_eq!(identifier("tauri.conf.json"), "com.nakos.app");
    let gradle = read("gen/android/app/build.gradle.kts");
    assert!(gradle.contains(r#"applicationId = "org.nakos.app""#), "applicationId");
}

#[test]
fn the_manifest_turns_android_backup_off() {
    let manifest = read("gen/android/app/src/main/AndroidManifest.xml");
    let start = manifest.find("<application").expect("<application");
    let tag = &manifest[start..start + manifest[start..].find('>').unwrap()];
    for attr in [
        r#"android:allowBackup="false""#,
        r#"android:fullBackupContent="false""#,
        r#"android:dataExtractionRules="@xml/data_extraction_rules""#,
    ] {
        assert!(tag.contains(attr), "missing {attr} on <application>");
    }
}

#[test]
fn every_data_domain_is_kept_out_of_cloud_backup_and_device_transfer() {
    let rules = read("gen/android/app/src/main/res/xml/data_extraction_rules.xml");
    assert!(!rules.contains("<include"), "an <include> would let data back in");
    for section in ["cloud-backup", "device-transfer"] {
        let open = format!("<{section}>");
        let close = format!("</{section}>");
        let start = rules.find(&open).unwrap_or_else(|| panic!("missing {open}"));
        let end = start + rules[start..].find(&close).unwrap_or_else(|| panic!("missing {close}"));
        let body = &rules[start..end];
        for domain in DOMAINS {
            let exclude = format!(r#"<exclude domain="{domain}" path="." />"#);
            assert!(body.contains(&exclude), "{section} does not exclude {domain}");
        }
    }
}
