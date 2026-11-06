use crate::db;
use crate::state::AppState;

#[tauri::command]
pub fn get_preference(state: tauri::State<AppState>, key: String) -> Result<Option<String>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    db::pref_get(&conn, &key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_preference(state: tauri::State<AppState>, key: String, value: String) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    db::pref_set(&conn, &key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_all_preferences(state: tauri::State<AppState>) -> Result<Vec<(String, String)>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT key, value FROM preferences").map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}
