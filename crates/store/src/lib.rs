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
    #[error("databázi se nepodařilo přesunout z {from} do {to}: {source}")]
    Move {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "databázi {path} se před přesunem nepodařilo uzavřít (WAL s posledními \
         zápisy nejde zapsat do hlavního souboru): {detail}"
    )]
    MoveWal { path: PathBuf, detail: String },
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
    // Tentýž soubor zapsaný jinak — jiná velikost písmen ve složce
    // („d:\data" proti stopě „D:\Data"), junction typu C:\Users\All Users.
    // Porovnání PathBuf je citlivé na velikost písmen, takže dřív padlo
    // MoveBlocked na ŽIVOU databázi a rada „smaž ji ručně" vedla ke
    // smazání jediné kopie historie. Není co stěhovat.
    if to.exists() && same_file(from, to) {
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
    // Osiřelý WAL na cíli (hlavní soubor tam není) by SQLite po přesunu
    // přehrál jako „horký" WAL přes NAŠI databázi — hlavička WAL s obsahem
    // databáze svázaná není, takže by vrátil cizí stránky. Bez hlavního
    // souboru nikomu nepatří; když nejde smazat, radši se nestěhuje.
    for pripona in ["-wal", "-shm"] {
        let b = side_file(to, pripona);
        if b.exists() {
            std::fs::remove_file(&b).map_err(|source| Error::Move {
                from: from.to_path_buf(),
                to: b.clone(),
                source,
            })?;
        }
    }
    // WAL NENÍ odvozený soubor: po nečistém konci (pád, výpadek proudu)
    // nese commitnuté transakce, které ještě nejsou v hlavním souboru.
    // Dřív se stěhoval zvlášť a jeho selhání se mlčky zahodilo — databáze
    // na novém místě se pak otevřela bez posledních minut historie.
    // Proto se nejdřív všechno zapíše do hlavního souboru a stěhuje se
    // jediný soubor.
    close_wal(from)?;
    if let Err(prvni) = std::fs::rename(from, to) {
        // Přes hranici svazku `rename` nefunguje — pak kopie a smazání.
        // (Na stejném svazku selže, když soubor drží cizí handle bez
        // sdílení mazání — zálohovač, antivir, prohlížeč DB.)
        let presun = std::fs::copy(from, to).and_then(|_| std::fs::remove_file(from));
        if let Err(source) = presun {
            // Dřív kopie na cíli zůstala ležet: služba jela dál na starém
            // místě, historie rostla tam, a každý další start hlásil
            // MoveBlocked na zastaralou kopii. Cíl před přesunem neexistoval
            // (kontrola výše), takže se maže jen to, co jsme sami vytvořili
            // — a jen dokud zdroj pořád leží na svém místě.
            if from.exists() {
                let _ = std::fs::remove_file(to);
            }
            tracing::debug!(error = %prvni, "rename databáze selhal, zkoušela se kopie");
            return Err(Error::Move {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source,
            });
        }
    }
    Ok(())
}

/// Soubor vedle databáze (`-wal`, `-shm`).
fn side_file(db: &Path, pripona: &str) -> PathBuf {
    PathBuf::from(format!("{}{pripona}", db.display()))
}

/// Ukazují obě cesty na tentýž soubor? Kanonizace na Windows rozbalí
/// junctiony a vrátí velikost písmen podle disku. Když selže, bere se
/// to jako dva různé soubory — pak zasáhne opatrné MoveBlocked.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Zapíše WAL do hlavního souboru a přepne databázi z WAL, takže SQLite
/// sám smaže `-wal` i `-shm`. Při dalším otevření je `open` zase přepne
/// na WAL. Selhání (poškozená databáze, drží ji jiný proces) přesun
/// odmítne — data jsou přednější než přání.
fn close_wal(db: &Path) -> Result<(), Error> {
    let chyba = |detail: String| Error::MoveWal {
        path: db.to_path_buf(),
        detail,
    };
    // Bez SQLITE_OPEN_CREATE: zdroj musí existovat, nic se nezakládá.
    let c = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| chyba(e.to_string()))?;
    let busy: i64 = c
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .map_err(|e| chyba(e.to_string()))?;
    if busy != 0 {
        return Err(chyba("databázi má otevřenou jiný proces".into()));
    }
    c.pragma_update(None, "journal_mode", "DELETE")
        .map_err(|e| chyba(e.to_string()))?;
    c.close().map_err(|(_, e)| chyba(e.to_string()))?;
    // Pojistka: neprázdný WAL po uzavření znamená, že v něm pořád něco
    // je — stěhovat bez něj by bylo tiché zahození dat.
    let wal = side_file(db, "-wal");
    if let Ok(m) = std::fs::metadata(&wal) {
        if m.len() > 0 {
            return Err(chyba("WAL po uzavření pořád leží vedle databáze".into()));
        }
        let _ = std::fs::remove_file(&wal);
    }
    // -shm je jen index WAL; SQLite si ho kdykoli postaví znovu.
    let _ = std::fs::remove_file(side_file(db, "-shm"));
    Ok(())
}

/// Vrátí zapisovací spojení z rozpracované transakce, pokud v ní zůstalo.
///
/// Pojistka pro zapisovací smyčku služby: kdyby jakýkoli zápis skončil
/// chybou uprostřed ručně otevřené transakce, spojení by v ní zůstalo
/// a každý další `transaction()` by padal na „cannot start a transaction
/// within a transaction" — do restartu by se nezapsal jediný vzorek.
/// Vrací `true`, když se něco vracelo.
pub fn rollback_if_open(conn: &Connection) -> bool {
    if conn.is_autocommit() {
        return false;
    }
    let _ = conn.execute_batch("ROLLBACK");
    true
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

    /// Skutečná SQLite databáze s `radku` řádky, čistě zavřená.
    fn databaze(p: &Path, radku: i64) {
        let c = Connection::open(p).expect("databáze");
        c.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();
        for i in 0..radku {
            c.execute("INSERT INTO t (x) VALUES (?1)", [i]).unwrap();
        }
    }

    fn radku(p: &Path) -> i64 {
        let c = Connection::open(p).expect("databáze");
        c.query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .expect("tabulka")
    }

    // Stav po pádu služby: databáze plus WAL s commitnutými, ale ještě
    // nezapsanými transakcemi. Přesun o ně nesmí přijít.
    #[test]
    fn presun_nezahodi_obsah_walu() {
        let zdroj = temp("presun-pad");
        let a = temp("presun-z");
        let b = temp("presun-na");
        let zive = zdroj.join(DB_FILE);
        {
            let c = Connection::open(&zive).unwrap();
            c.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE t (x INTEGER);",
            )
            .unwrap();
            for i in 0..500 {
                c.execute("INSERT INTO t (x) VALUES (?1)", [i]).unwrap();
            }
            // Snímek souborů za běhu = to, co zůstane po pádu.
            let z = a.join(DB_FILE);
            std::fs::copy(&zive, &z).unwrap();
            std::fs::copy(side_file(&zive, "-wal"), side_file(&z, "-wal")).unwrap();
        }
        let z = a.join(DB_FILE);
        assert!(
            std::fs::metadata(side_file(&z, "-wal")).unwrap().len() > 0,
            "test nemá neprázdný WAL"
        );
        let na = b.join(DB_FILE);
        move_db(&z, &na).expect("přesun");
        assert!(!z.exists(), "databáze zůstala na původním místě");
        assert!(
            !side_file(&z, "-wal").exists(),
            "WAL zůstal na původním místě"
        );
        assert_eq!(radku(&na), 500, "přesun ztratil řádky z WAL");
    }

    // Osiřelý WAL na cíli se nesmí přehrát přes přestěhovanou databázi.
    #[test]
    fn osirely_wal_na_cili_se_neprehraje() {
        let a = temp("sirotek-z");
        let b = temp("sirotek-na");
        let z = a.join(DB_FILE);
        let na = b.join(DB_FILE);
        databaze(&z, 3);
        naplnit(&side_file(&na, "-wal"), 4096);
        move_db(&z, &na).expect("přesun");
        assert!(
            !side_file(&na, "-wal").exists(),
            "cizí WAL zůstal u databáze"
        );
        assert_eq!(radku(&na), 3);
    }

    // Tentýž soubor zapsaný jinou velikostí písmen není „databáze na
    // cíli" — dřív to hlásilo MoveBlocked na živou databázi.
    #[test]
    fn stejna_cesta_jinak_zapsana_neni_kolize() {
        let a = temp("velikost-pismen");
        let z = a.join(DB_FILE);
        databaze(&z, 1);
        let jinak = PathBuf::from(a.to_string_lossy().to_uppercase()).join(DB_FILE);
        assert_ne!(z, jinak);
        move_db(&z, &jinak).expect("tentýž soubor není kolize");
        assert_eq!(radku(&z), 1, "databáze utrpěla");
    }

    // Když se zdroj po kopii nepodaří smazat (drží ho cizí proces), kopie
    // na cíli nesmí zůstat — jinak by každý další start hlásil MoveBlocked
    // na zastaralou kopii, zatímco historie dál roste ve zdroji.
    #[cfg(windows)]
    #[test]
    fn nepovedeny_presun_neneha_kopii_na_cili() {
        use std::os::windows::fs::OpenOptionsExt;
        let a = temp("zamek-z");
        let b = temp("zamek-na");
        let z = a.join(DB_FILE);
        let na = b.join(DB_FILE);
        databaze(&z, 2);
        {
            // Čtenář bez FILE_SHARE_DELETE: rename i smazání zdroje selžou.
            let _drzi = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0x1 | 0x2)
                .open(&z)
                .expect("cizí handle");
            assert!(move_db(&z, &na).is_err(), "přesun prošel přes zámek");
            assert!(z.exists(), "zdroj zmizel");
            assert!(!na.exists(), "na cíli zůstala kopie");
        }
        move_db(&z, &na).expect("po uvolnění má přesun projít");
        assert_eq!(radku(&na), 2);
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
        databaze(&z, 10);
        move_db(&z, &na).expect("tam");
        move_db(&na, &z).expect("zpátky");
        assert!(z.exists(), "databáze se nevrátila");
        assert!(!na.exists(), "na cizím místě něco zůstalo");
    }
}
