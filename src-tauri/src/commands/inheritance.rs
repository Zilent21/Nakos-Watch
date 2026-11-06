use crate::db::models::{InheritanceConfig, RecoveryContact};
use crate::state::AppState;
use rusqlite::params;
use uuid::Uuid;

fn row_to_contact(row: &rusqlite::Row) -> rusqlite::Result<RecoveryContact> {
    Ok(RecoveryContact {
        id: row.get("id")?,
        name: row.get("name")?,
        relationship: row.get("relationship")?,
        contact_method: row.get("contact_method")?,
        has_key_share: row.get::<_, i64>("has_key_share")? != 0,
        has_notes: row.get::<_, Option<Vec<u8>>>("notes_encrypted")?.is_some(),
        created_at: row.get("created_at")?,
    })
}

pub(crate) fn get_or_create_config(conn: &rusqlite::Connection, wallet_id: &Option<String>) -> Result<String, String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM inheritance_configs WHERE wallet_id IS ?1",
            params![wallet_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO inheritance_configs (id, wallet_id) VALUES (?1, ?2)",
        params![id, wallet_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub fn get_inheritance_config(state: tauri::State<AppState>, wallet_id: Option<String>) -> Result<InheritanceConfig, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let id = get_or_create_config(&conn, &wallet_id)?;

    let (has_instructions, last_tested_at, test_status, updated_at): (bool, Option<String>, String, String) = conn
        .query_row(
            "SELECT instructions_encrypted IS NOT NULL, last_tested_at, test_status, updated_at FROM inheritance_configs WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, Option<Vec<u8>>>(0)?.is_some(), r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT rc.* FROM recovery_contacts rc
             JOIN inheritance_contact_links l ON l.contact_id = rc.id
             WHERE l.inheritance_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let contacts = stmt
        .query_map(params![id], row_to_contact)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(InheritanceConfig { id, wallet_id, has_instructions, last_tested_at, test_status, updated_at, contacts })
}

#[tauri::command]
pub fn save_inheritance_instructions(
    state: tauri::State<AppState>,
    wallet_id: Option<String>,
    instructions: String,
) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let id = get_or_create_config(&conn, &wallet_id)?;

    let vault = state.vault()?;
    let guard = vault.lock();
    let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it before saving inheritance instructions")?;
    let (nonce, ct) = key.encrypt(instructions.as_bytes()).map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE inheritance_configs SET instructions_encrypted = ?1, instructions_nonce = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?3",
        params![ct, nonce, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_inheritance_instructions(state: tauri::State<AppState>, wallet_id: Option<String>) -> Result<Option<String>, String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let id = get_or_create_config(&conn, &wallet_id)?;

    let row: Option<(Vec<u8>, Vec<u8>)> = conn
        .query_row(
            "SELECT instructions_encrypted, instructions_nonce FROM inheritance_configs WHERE id = ?1",
            params![id],
            |r| {
                let ct: Option<Vec<u8>> = r.get(0)?;
                let nonce: Option<Vec<u8>> = r.get(1)?;
                Ok(ct.zip(nonce))
            },
        )
        .map_err(|e| e.to_string())?;
    let Some((ct, nonce)) = row else { return Ok(None) };

    let vault = state.vault()?;
    let guard = vault.lock();
    let key = guard.master_key.as_ref().ok_or("vault is locked")?;
    let pt = key.decrypt(&nonce, &ct).map_err(|e| e.to_string())?;
    Ok(Some(String::from_utf8_lossy(&pt).to_string()))
}

#[derive(Debug, serde::Deserialize)]
pub struct NewRecoveryContact {
    pub name: String,
    pub relationship: Option<String>,
    pub contact_method: Option<String>,
    pub has_key_share: bool,
    pub notes: Option<String>,
    pub link_to_wallet_id: Option<String>,
}

#[tauri::command]
pub fn add_recovery_contact(state: tauri::State<AppState>, input: NewRecoveryContact) -> Result<RecoveryContact, String> {
    add_recovery_contact_inner(&state, input)
}

fn add_recovery_contact_inner(state: &AppState, input: NewRecoveryContact) -> Result<RecoveryContact, String> {
    // Contacts can never be edited or deleted afterwards, so a key-like name, relationship or contact
    // method would block encrypted backups for good. (The notes are sealed, not plain text.)
    crate::commands::wallets::refuse_private_key_material([Some(input.name.as_str()), input.relationship.as_deref(), input.contact_method.as_deref()])?;
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();

    let (notes_ct, notes_nonce) = if let Some(notes) = &input.notes {
        let vault = state.vault()?;
        let guard = vault.lock();
        let key = guard.master_key.as_ref().ok_or("vault is locked — unlock it before saving contact notes")?;
        let (nonce, ct) = key.encrypt(notes.as_bytes()).map_err(|e| e.to_string())?;
        (Some(ct), Some(nonce))
    } else {
        (None, None)
    };

    conn.execute(
        "INSERT INTO recovery_contacts (id, name, relationship, contact_method, has_key_share, notes_encrypted, notes_nonce)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![id, input.name, input.relationship, input.contact_method, input.has_key_share as i64, notes_ct, notes_nonce],
    )
    .map_err(|e| e.to_string())?;

    let inheritance_id = get_or_create_config(&conn, &input.link_to_wallet_id)?;
    conn.execute(
        "INSERT INTO inheritance_contact_links (id, inheritance_id, contact_id) VALUES (?1,?2,?3)",
        params![Uuid::new_v4().to_string(), inheritance_id, id],
    )
    .map_err(|e| e.to_string())?;

    conn.query_row("SELECT * FROM recovery_contacts WHERE id = ?1", params![id], row_to_contact)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mark_recovery_tested(state: tauri::State<AppState>, wallet_id: Option<String>, passed: bool) -> Result<(), String> {
    let pool = state.pool()?;
    let conn = pool.get().map_err(|e| e.to_string())?;
    let id = get_or_create_config(&conn, &wallet_id)?;
    let status = if passed { "tested_pass" } else { "tested_fail" };
    conn.execute(
        "UPDATE inheritance_configs SET test_status = ?1, last_tested_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        params![status, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contacts can never be edited or deleted, so key-like text is refused up front (it would
    /// block encrypted backups for good); an ordinary contact still saves.
    #[test]
    fn contact_text_that_looks_like_a_key_is_refused() {
        const KEY_LIKE: &str = "xprvAAAAAAAAAAAAAAAAAAAAAAAA";
        let dir = crate::test_dir::TestDir::new("contact");
        let pool = crate::db::open_pool(&dir).unwrap();
        let state = AppState::default();
        state.inner.lock().pool = Some(pool.clone());
        let contact = |name: &str, relationship: Option<&str>, method: Option<&str>| NewRecoveryContact {
            name: name.into(),
            relationship: relationship.map(String::from),
            contact_method: method.map(String::from),
            has_key_share: false,
            notes: None,
            link_to_wallet_id: None,
        };
        let conn = pool.get().unwrap();
        let rows = || -> i64 { conn.query_row("SELECT COUNT(*) FROM recovery_contacts", [], |r| r.get(0)).unwrap() };

        for bad in [contact(KEY_LIKE, None, None), contact("Anna", Some(KEY_LIKE), None), contact("Anna", None, Some(KEY_LIKE))] {
            assert_eq!(add_recovery_contact_inner(&state, bad).unwrap_err(), "private key material is not stored");
        }
        assert_eq!(rows(), 0, "nothing was written");
        add_recovery_contact_inner(&state, contact("Anna", Some("sister"), Some("anna@example.com"))).unwrap();
        assert_eq!(rows(), 1);

        state.inner.lock().pool = None;
        drop(conn);
        drop(pool);
    }
}
