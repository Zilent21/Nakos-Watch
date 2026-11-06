mod autolock;
mod about;
mod backup;
mod durable;
mod explorer_cmd;
pub(crate) mod file_dialog;
mod inheritance;
mod inheritance_packet;
mod node;
mod pdf;
mod prefs;
mod scoring_cmd;
mod utxos;
mod wallets;

pub use autolock::{spawn_watchdog, AutoLock, AutoLockSettings, LOCKED_EVENT};
pub(crate) use autolock::load_setting as load_auto_lock_setting;
pub use backup::*;
pub use explorer_cmd::*;
pub use inheritance::*;
pub use inheritance_packet::*;
pub use node::*;
pub use prefs::*;
pub use scoring_cmd::*;
pub use utxos::*;
pub use wallets::*;

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // wallets
        wallets::list_wallets,
        wallets::add_wallet,
        wallets::detect_source_type,
        wallets::update_wallet,
        wallets::archive_wallet,
        wallets::sync_wallet,
        wallets::add_multisig_wallet,
        wallets::get_multisig_config,
        wallets::portfolio_summary,
        wallets::list_wallet_balances,
        wallets::snapshot_wallets,
        // utxos
        utxos::list_utxos,
        utxos::freeze_utxo,
        utxos::label_utxo,
        utxos::utxo_distribution,
        utxos::plan_consolidation,
        utxos::plan_spend,
        utxos::set_utxo_basis,
        utxos::get_tax_readiness,
        // scoring
        scoring_cmd::get_security_score,
        scoring_cmd::get_privacy_score,
        scoring_cmd::get_privacy_dashboard,
        scoring_cmd::get_health_score,
        scoring_cmd::get_health_history,
        scoring_cmd::record_health_snapshot,
        scoring_cmd::run_daily_health_snapshot,
        scoring_cmd::update_security_config,
        // node
        node::list_node_configs,
        node::acknowledge_public_electrum,
        node::add_node_config,
        node::update_node_config,
        node::set_primary_node,
        node::archive_node_config,
        node::test_node_connection,
        node::get_node_status,
        node::get_fee_estimates,
        explorer_cmd::get_explorer_settings,
        explorer_cmd::save_explorer_settings,
        explorer_cmd::get_explorer_height,
        explorer_cmd::explorer_lookup_address,
        // inheritance
        inheritance::get_inheritance_config,
        inheritance::save_inheritance_instructions,
        inheritance::get_inheritance_instructions,
        inheritance::add_recovery_contact,
        inheritance::mark_recovery_tested,
        inheritance_packet::get_inheritance_packet,
        inheritance_packet::export_inheritance_packet,
        inheritance_packet::import_inheritance_packet,
        inheritance_packet::get_review_reminder,
        inheritance_packet::save_review_reminder,
        // backup / vault
        backup::vault_status,
        backup::vault_init,
        backup::vault_unlock,
        backup::vault_lock,
        backup::vault_try_keychain_unlock,
        backup::set_keychain_helper,
        autolock::vault_touch,
        autolock::vault_app_hidden,
        autolock::get_auto_lock,
        autolock::set_auto_lock,
        // about
        about::open_project_link,
        backup::export_encrypted_backup,
        backup::pick_backup_to_restore,
        backup::restore_encrypted_backup,
        backup::erase_all_data,
        backup::run_local_backup,
        backup::list_backups,
        // preferences
        prefs::get_preference,
        prefs::set_preference,
        prefs::get_all_preferences,
    ]
}

#[cfg(test)]
mod no_path_from_the_webview {
    //! R11: no Tauri command may take a file path from the frontend. Files the app reads or
    //! writes for the user are chosen in a dialog that Rust opens (`file_dialog`), so a compromised
    //! webview cannot name `~/.ssh/id_ed25519` or a file to overwrite.

    /// Names of `#[tauri::command]` functions in `src` that have a parameter whose name contains
    /// "path" or "uri" (on Android a dialog returns a `content://` URI where desktop has a path).
    fn commands_taking_a_path(src: &str) -> Vec<String> {
        let marker = concat!("#[tauri::", "command]");
        let mut found = Vec::new();
        for (at, _) in src.match_indices(marker) {
            let rest = &src[at + marker.len()..];
            let Some(fn_at) = rest.find("fn ") else { continue };
            let after_fn = &rest[fn_at + 3..];
            let name: String = after_fn.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let Some(open) = after_fn.find('(') else { continue };
            let mut depth = 0usize;
            let mut end = open;
            for (i, ch) in after_fn[open..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let params = &after_fn[open + 1..end];
            // Parameter names are what precedes a ':' at the top level of each `name: Type`.
            let takes_path = params.split(',').any(|p| {
                let n = p.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
                n.contains("path") || n.contains("uri")
            });
            if takes_path {
                found.push(name);
            }
        }
        found
    }

    #[test]
    fn the_scanner_finds_a_command_with_a_path_parameter() {
        let bad = format!("{}\npub fn export_thing(state: tauri::State<AppState>, dest_path: String) -> Result<(), String> {{ Ok(()) }}\n", concat!("#[tauri::", "command]"));
        assert_eq!(commands_taking_a_path(&bad), vec!["export_thing"]);
        let multi = format!("{}\npub async fn read_thing(\n    app: tauri::AppHandle,\n    src_path: String,\n) -> Result<(), String> {{ Ok(()) }}\n", concat!("#[tauri::", "command]"));
        assert_eq!(commands_taking_a_path(&multi), vec!["read_thing"]);
        let uri = format!("{}\npub fn open_doc(state: tauri::State<AppState>, content_uri: String) -> Result<(), String> {{ Ok(()) }}\n", concat!("#[tauri::", "command]"));
        assert_eq!(commands_taking_a_path(&uri), vec!["open_doc"]);
        let fine = format!("{}\npub fn ok_thing(state: tauri::State<AppState>, label: String) -> Result<(), String> {{ Ok(()) }}\n", concat!("#[tauri::", "command]"));
        assert!(commands_taking_a_path(&fine).is_empty());
    }

    #[test]
    fn no_tauri_command_takes_a_file_path_from_the_frontend() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands");
        let mut offenders = Vec::new();
        let mut scanned_commands = 0usize;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            scanned_commands += src.matches(concat!("#[tauri::", "command]")).count();
            for name in commands_taking_a_path(&src) {
                offenders.push(format!("{}: {name}", path.file_name().unwrap().to_string_lossy()));
            }
        }
        assert!(scanned_commands > 50, "the scan did not find the commands ({scanned_commands})");
        assert!(offenders.is_empty(), "commands that take a path from the webview: {offenders:?}");
    }
}
