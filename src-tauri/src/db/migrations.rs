use rusqlite_migration::{Migrations, M};

/// All schema migrations, in order. Add new `M::up(...)` entries here —
/// never edit a migration that has already shipped.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../../migrations/0001_init.sql")),
        M::up(include_str!("../../migrations/0002_node_cookie_auth.sql")),
        M::up(include_str!("../../migrations/0003_signing_origin.sql")),
        M::up(include_str!("../../migrations/0004_inheritance_review.sql")),
        M::up(include_str!("../../migrations/0005_wallet_snapshots.sql")),
        M::up(include_str!("../../migrations/0006_utxo_basis_note.sql")),
        M::up(include_str!("../../migrations/0007_core_rpc_no_tls.sql")),
        M::up(include_str!("../../migrations/0008_utxo_unseen_since.sql")),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bitcoin Core rows saved with "TLS" on (a flag the transport ignored) are cleared when the
    /// database is migrated, including a restored older backup; Electrum's real TLS flag stays.
    #[test]
    fn migration_0007_clears_the_tls_flag_on_existing_core_rows_only() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_version(&mut conn, 6).unwrap();
        for (id, kind) in [("core", "bitcoin_core_rpc"), ("electrum", "electrum")] {
            conn.execute("INSERT INTO node_configs (id, kind, label, host, port, use_tls) VALUES (?1, ?2, ?1, 'node.example', 8332, 1)", rusqlite::params![id, kind]).unwrap();
        }
        migrations().to_latest(&mut conn).unwrap();
        let tls = |id: &str| -> i64 { conn.query_row("SELECT use_tls FROM node_configs WHERE id = ?1", rusqlite::params![id], |r| r.get(0)).unwrap() };
        assert_eq!(tls("core"), 0, "a Core row never keeps a TLS flag");
        assert_eq!(tls("electrum"), 1, "Electrum TLS is untouched");
    }

    /// 0008 only adds `utxos.unseen_since`: an existing database keeps every coin and every
    /// user-entered field, and the new column starts empty.
    #[test]
    fn migration_0008_adds_unseen_since_and_keeps_existing_coin_data() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_version(&mut conn, 7).unwrap();
        conn.execute("INSERT INTO wallets (id,name,custody_type,source_type) VALUES ('w','t','cold_storage','xpub')", []).unwrap();
        conn.execute(
            "INSERT INTO utxos (id,wallet_id,txid,vout,amount_sats,is_frozen,label,acquisition_cost_fiat,acquisition_date,acquisition_note,spent)
             VALUES ('u','w','tx',0,5000,1,'cold',12.5,'2024-01-02','gift',0)",
            [],
        )
        .unwrap();
        assert!(conn.prepare("SELECT unseen_since FROM utxos").is_err(), "column must not exist before 0008");
        migrations().to_latest(&mut conn).unwrap();
        let row: (i64, i64, Option<String>, Option<f64>, Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT amount_sats, is_frozen, label, acquisition_cost_fiat, acquisition_date, acquisition_note, unseen_since FROM utxos WHERE id = 'u'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .unwrap();
        assert_eq!(row, (5000, 1, Some("cold".into()), Some(12.5), Some("2024-01-02".into()), Some("gift".into()), None));
    }
}
