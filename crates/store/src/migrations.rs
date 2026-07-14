//! Migrace schématu — lineární, číslované, idempotentní systém.
//!
//! Verze schématu se drží v SQLite `PRAGMA user_version`. Každá migrace
//! je SQL blok, který se aplikuje v transakci; po úspěchu se verze
//! zvýší. Migrace se nikdy nemění zpětně — jen se přidávají na konec.

use rusqlite::Connection;

/// Seznam migrací v pořadí aplikace. Index 0 = přechod na verzi 1.
/// v0 zakládá jen tabulku `meta`; datové tabulky (app, proc_instance,
/// sample_*…) přibudou s kolektory ve v1+.
const MIGRATIONS: &[&str] = &[
    // → verze 1: meta tabulka (klíč/hodnota) pro provozní údaje.
    "CREATE TABLE meta (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) WITHOUT ROWID;
    INSERT INTO meta (key, value) VALUES ('created_ts', strftime('%s','now'));",
];

/// Aplikuje všechny dosud neaplikované migrace. Bezpečné volat při
/// každém startu — už aplikované se přeskočí podle `user_version`.
pub fn run(conn: &Connection) -> Result<(), rusqlite::Error> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let target = (idx + 1) as u32;
        if target <= current {
            continue;
        }
        tracing::info!(from = current, to = target, "aplikuji migraci schématu");
        conn.execute_batch(&format!(
            "BEGIN;\n{sql}\nPRAGMA user_version = {target};\nCOMMIT;"
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Migrace musí být idempotentní — druhý běh nesmí nic změnit ani selhat.
    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        run(&conn).unwrap();
        let v: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
    }
}
