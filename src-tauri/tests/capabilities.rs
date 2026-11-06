//! The webview's permissions. tauri-plugin-fs is registered on phones so Rust can open the file a
//! dialog returns; the webview itself must not get any fs command.

#[test]
fn the_webview_gets_no_file_system_permission() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/default.json");
    let caps: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let perms: Vec<String> = caps["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .map(|p| p.as_str().map(str::to_string).unwrap_or_else(|| p["identifier"].as_str().unwrap_or_default().to_string()))
        .collect();
    assert!(!perms.is_empty());
    assert!(perms.iter().all(|p| !p.starts_with("fs:")), "fs permission granted: {perms:?}");
    assert_eq!(perms, ["core:default", "dialog:allow-ask", "dialog:allow-open"]);
}
