pub mod migrations;
pub mod models;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("pool error: {0}")]
    Pool(#[from] r2d2::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found")]
    NotFound,
}

pub fn db_file_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("nakos.db")
}

/// Open (creating if needed) the local SQLite database, run migrations, and
/// return a connection pool. Foreign keys and WAL mode are enabled per
/// connection via the pool's init hook.
pub fn open_pool(app_data_dir: &Path) -> Result<DbPool, DbError> {
    std::fs::create_dir_all(app_data_dir)?;
    let path = db_file_path(app_data_dir);

    let manager = SqliteConnectionManager::file(&path).with_init(|conn: &mut Connection| {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
    });
    let pool = Pool::builder().max_size(8).build(manager)?;

    {
        let mut conn = pool.get()?;
        migrations::migrations().to_latest(&mut conn)?;
        split_saved_multipath_descriptors(&conn)?;
    }

    Ok(pool)
}

/// Before 0.6.0 a pasted `<0;1>` descriptor was saved as it was, and every
/// sync of that wallet failed. Split any such wallet into its receive and
/// change descriptors. Runs on every open; it does nothing once they're split.
pub fn split_saved_multipath_descriptors(conn: &Connection) -> Result<(), DbError> {
    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT id, descriptor FROM wallets WHERE descriptor LIKE '%<%;%>%'")?;
        let it = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        it.collect::<Result<_, _>>()?
    };
    for (id, d) in rows {
        // A descriptor that won't parse is left alone: its sync error still says what's wrong.
        if let Ok(Some((receive, change))) = crate::bitcoin::descriptors::split_multipath(&d) {
            conn.execute(
                "UPDATE wallets SET descriptor = ?1, change_descriptor = ?2 WHERE id = ?3",
                rusqlite::params![receive, change, id],
            )?;
        }
    }
    Ok(())
}

// -- db_meta key/value helpers (KDF params, schema flags) ----------------

pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>, DbError> {
    let mut stmt = conn.prepare("SELECT value FROM db_meta WHERE key = ?1")?;
    let mut rows = stmt.query([key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO db_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

// -- preferences -----------------------------------------------------------

pub fn pref_get(conn: &Connection, key: &str) -> Result<Option<String>, DbError> {
    let mut stmt = conn.prepare("SELECT value FROM preferences WHERE key = ?1")?;
    let mut rows = stmt.query([key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn pref_set(conn: &Connection, key: &str, value: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO preferences (key, value, updated_at) VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value],
    )?;
    Ok(())
}
