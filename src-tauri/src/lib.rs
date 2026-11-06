pub mod bitcoin;
pub mod commands;
pub mod crypto;
pub mod db;
pub mod rpc;
pub mod scoring;
pub mod state;
#[cfg(test)]
pub(crate) mod test_dir;
pub mod window_fit;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    rpc::ensure_tls_provider();
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    // Rust uses it to open the `content://` file a dialog returns on a phone; no fs permission is
    // granted to the webview.
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_fs::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(commands::file_dialog::android_fd::init());
    builder
        .manage(state::AppState::default())
        .setup(|app| {
            state::init(app)?;
            commands::load_auto_lock_setting(&app.state::<state::AppState>());
            commands::spawn_watchdog(app.handle().clone());
            // A window taller than the screen hid Settings behind the taskbar; never fatal.
            #[cfg(desktop)]
            let _ = window_fit::fit_main_window(app);
            Ok(())
        })
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("error while running Nakos");
}
