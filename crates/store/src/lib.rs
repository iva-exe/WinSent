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
    #[error(
        "na cílovém místě už databáze je ({path}) — přesuň nebo smaž ji ručně, \
         Winsent sám nerozhoduje, která z nich je ta pravá"
    )]
    MoveBlocked { path: PathBuf },
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

/// Jméno souboru databáze. Přípony WAL a shm k němu patří.
pub const DB_FILE: &str = "syswatch.db";

/// Cesta k databázi podle konfigurace. Prázdný `dir` = výchozí místo.
pub fn db_path_in(dir: &str) -> Result<PathBuf, Error> {
    let d = dir.trim();
    if d.is_empty() {
        return db_path();
    }
    Ok(PathBuf::from(d).join(DB_FILE))
}

/// Soubor se stopou, KDE databáze právě leží.
///
/// Bez něj by služba neuměla stěhovat zpátky: config říká, kam se
/// databáze má dostat, ale ne odkud. Když si uživatel po přesunu na
/// jiný disk zvolil zase výchozí umístění, cíl se shodoval s výchozím
/// místem, nikdo nic nepřesunul a služba si na výchozím místě založila
/// PRÁZDNOU databázi — celá historie zůstala ležet na starém disku.
/// Naměřeno při ověřování: 119,7 MB na D: a 0 MB na C:.
///
/// Stopa žije vždy ve výchozím adresáři, ať databáze leží kdekoli.
fn db_marker() -> Result<PathBuf, Error> {
    Ok(data_dir()?.join("db_location.txt"))
}

/// Kde databáze leží podle stopy. Bez stopy se předpokládá výchozí
/// místo — tak to bylo, než se stěhování vůbec zavedlo.
pub fn db_current_dir() -> Result<PathBuf, Error> {
    let marker = db_marker()?;
    match std::fs::read_to_string(&marker) {
        Ok(s) if !s.trim().is_empty() => Ok(PathBuf::from(s.trim())),
        _ => data_dir(),
    }
}

/// Zapíše stopu. Selhání se nepovažuje za fatální — jen se příště
/// bude vycházet z výchozího místa.
pub fn set_db_current_dir(dir: &Path) {
    if let Ok(marker) = db_marker() {
        if let Some(parent) = marker.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&marker, dir.to_string_lossy().as_bytes());
    }
}

/// Přestěhuje databázi, když si uživatel přál jiné místo.
///
/// Volá se JEDINĚ při startu služby, tedy ve chvíli, kdy databázi nikdo
/// nedrží otevřenou. Stěhovat ji za běhu by znamenalo přijít o rozepsaný
/// WAL, ve kterém sedí poslední vzorky.
///
/// Když se přesun nepovede, vrátí se chyba a volající zůstane u starého
/// místa — data jsou přednější než přání.
pub fn move_db(from: &Path, to: &Path) -> Result<(), Error> {
    if from == to || !from.exists() {
        return Ok(());
    }
    // Na cíli něco leží — dál se nejde.
    //
    // Rozhodovat podle velikosti, která z těch dvou databází je „ta
    // pravá", by znamenalo hádat s cizí historií v ruce. Za normálního
    // provozu tenhle stav nenastane: přesun soubor STĚHUJE, takže na
    // původním místě nic nezůstává. Když k němu přesto dojde, řekne se
    // to uživateli a rozhodne on.
    if to.exists() {
        return Err(Error::MoveBlocked {
            path: to.to_path_buf(),
        });
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|source| Error::CreateDir {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    // Nejdřív samotná databáze; WAL a shm jsou odvozené soubory, které
    // SQLite umí dopočítat znovu, takže na jejich selhání se nepadá.
    std::fs::rename(from, to)
        .or_else(|_| {
            // Přes hranici svazku `rename` nefunguje — pak kopie a smazání.
            std::fs::copy(from, to).and_then(|_| std::fs::remove_file(from))
        })
        .map_err(|source| Error::CreateDir {
            path: to.to_path_buf(),
            source,
        })?;
    for pripona in ["-wal", "-shm"] {
        let a = PathBuf::from(format!("{}{pripona}", from.display()));
        let b = PathBuf::from(format!("{}{pripona}", to.display()));
        if a.exists() {
            let _ = std::fs::rename(&a, &b).or_else(|_| {
                std::fs::copy(&a, &b).and_then(|_| std::fs::remove_file(&a))
            });
        }
    }
    Ok(())
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

#[cfg(test)]
mod stehovani {
    use super::*;

    fn temp(jmeno: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("winsent-test-{jmeno}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dočasná složka");
        d
    }

    fn naplnit(p: &Path, bajtu: usize) {
        std::fs::write(p, vec![7u8; bajtu]).expect("zápis");
    }

    // Přesun vezme databázi i její WAL.
    #[test]
    fn presun_vezme_i_wal() {
        let a = temp("presun-z");
        let b = temp("presun-na");
        let z = a.join(DB_FILE);
        naplnit(&z, 200_000);
        naplnit(&PathBuf::from(format!("{}-wal", z.display())), 1024);
        let na = b.join(DB_FILE);
        move_db(&z, &na).expect("přesun");
        assert!(na.exists(), "databáze na cíli chybí");
        assert!(!z.exists(), "databáze zůstala na původním místě");
        assert!(
            PathBuf::from(format!("{}-wal", na.display())).exists(),
            "WAL se nepřestěhoval"
        );
    }

    // Plnou databázi na cíli nesmí nic přepsat.
    #[test]
    fn plnou_databazi_na_cili_neprepiseme() {
        let a = temp("kolize-z");
        let b = temp("kolize-na");
        let z = a.join(DB_FILE);
        let na = b.join(DB_FILE);
        naplnit(&z, 200_000);
        naplnit(&na, 300_000);
        assert!(move_db(&z, &na).is_err(), "přesun přes plnou databázi prošel");
        assert_eq!(
            std::fs::metadata(&na).expect("cíl").len(),
            300_000,
            "cíl se přepsal"
        );
    }

    // Ani malá databáze na cíli se nepřepisuje.
    //
    // Rozhodovat podle velikosti, která z těch dvou je „ta pravá",
    // znamená hádat s cizí historií v ruce. Radši se to řekne uživateli.
    #[test]
    fn ani_mala_databaze_na_cili_neustoupi() {
        let a = temp("zbytek-z");
        let b = temp("zbytek-na");
        let z = a.join(DB_FILE);
        let na = b.join(DB_FILE);
        naplnit(&z, 200_000);
        naplnit(&na, 4096);
        assert!(move_db(&z, &na).is_err(), "malá databáze na cíli se přepsala");
        assert!(z.exists(), "zdroj zmizel, přestože se nepřesunul");
    }

    // Původní místo po přesunu zůstane prázdné, takže cesta zpátky je
    // volná. Právě tohle drží pravidlo „na cíli nesmí nic být" v chodu.
    #[test]
    fn cesta_zpatky_je_po_presunu_volna() {
        let a = temp("tam-z");
        let b = temp("tam-na");
        let z = a.join(DB_FILE);
        let na = b.join(DB_FILE);
        naplnit(&z, 200_000);
        move_db(&z, &na).expect("tam");
        move_db(&na, &z).expect("zpátky");
        assert!(z.exists(), "databáze se nevrátila");
        assert!(!na.exists(), "na cizím místě něco zůstalo");
    }
}
