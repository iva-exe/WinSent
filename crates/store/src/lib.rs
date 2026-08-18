//! store — SQLite úložiště (SPEC kap. 8).
//!
//! v0: otevření databáze v `%ProgramData%\syswatch\`, PRAGMA (WAL,
//! synchronous=NORMAL), systém migrací schématu a retenční smyčka,
//! která zatím nemá co mazat. Datové tabulky přibudou ve v1+.

use std::path::{Path, PathBuf};
use std::time::Duration;

// Re-export: konzumenti store (svc) pracují se spojením, aniž by
// museli záviset na rusqlite napřímo.
pub use rusqlite::Connection;
pub use rusqlite::Error as SqlError;

pub mod apps;
pub mod audit;
pub mod events;
pub mod history;
pub mod migrations;
pub mod permuse;
pub mod retention;
pub mod samples;

/// Chyby úložiště.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SQLite chyba: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("nelze vytvořit datový adresář {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("proměnná prostředí ProgramData není dostupná")]
    NoProgramData,
}

/// Vrátí datový adresář nástroje: `%ProgramData%\syswatch\`.
pub fn data_dir() -> Result<PathBuf, Error> {
    let base = std::env::var_os("ProgramData").ok_or(Error::NoProgramData)?;
    Ok(PathBuf::from(base).join("syswatch"))
}

/// Otevře (a případně založí) databázi na daném místě, nastaví PRAGMA
/// a provede migrace schématu. Vrací připravené spojení.
pub fn open(db_path: &Path) -> Result<Connection, Error> {
    if let Some(dir) = db_path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| Error::CreateDir {
            path: dir.to_path_buf(),
            source,
        })?;
    }

    let conn = Connection::open(db_path)?;

    // PRAGMA dle SPEC kap. 8: WAL kvůli souběžnému čtení při zápisu,
    // synchronous=NORMAL jako kompromis trvanlivost/výkon (díru při
    // BSODu zacelí ETW autologger od v3).
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "wal_autocheckpoint", 1000)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    migrations::run(&conn)?;
    Ok(conn)
}

/// Cesta k databázi uvnitř datového adresáře.
pub fn db_path() -> Result<PathBuf, Error> {
    Ok(data_dir()?.join("syswatch.db"))
}

/// Read-only spojení pro dotazy historie z IPC handleru — WAL dovolí
/// číst souběžně se zapisovacím vláknem bez zámků.
pub fn open_readonly(db_path: &Path) -> Result<Connection, Error> {
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    Ok(conn)
}

/// Přečte hodnotu z meta tabulky (provozní údaje: clean_shutdown…).
pub fn meta_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM meta WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get(0),
    )
    .ok()
}

/// Zapíše hodnotu do meta tabulky.
pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<(), Error> {
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Interval retenční smyčky z konfigurace.
pub fn retention_interval(cfg: &core_types::config::Config) -> Duration {
    Duration::from_secs(cfg.retention_interval_s.max(1))
}
