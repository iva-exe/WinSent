//! Zápis vzorků sampleru do SQLite (v1 flusher, SPEC kap. 3.4/8).
//! Volá se z jediného zapisovacího vlákna služby, dávkově v transakci.

use core_types::proc::{DiskDesc, ProcRow, SystemSnapshot};
use rusqlite::{params, Connection, OptionalExtension};

/// Zapíše názvy disků (jednou při startu služby).
pub fn upsert_disk_names(conn: &Connection, disks: &[DiskDesc]) -> Result<(), rusqlite::Error> {
    let mut stmt =
        conn.prepare_cached("INSERT OR REPLACE INTO disk_names (disk, name) VALUES (?1, ?2)")?;
    for d in disks {
        stmt.execute(params![d.index as i64, d.model])?;
    }
    Ok(())
}

/// Co už je v `proc_names_v` zapsané — aby se nepřepisovalo pořád dokola.
///
/// Tabulka mapuje instanci procesu (pid + čas vzniku) na jméno a
/// identitu. Zapisovala se celá při KAŽDÉM ticku, tedy 250 řádků každou
/// sekundu, přestože se jméno procesu za jeho život prakticky nezmění.
/// `INSERT OR REPLACE` přitom řádek pokaždé opravdu přepíše a zanese do
/// WAL celé stránky — z toho vycházely desítky gigabajtů zápisů denně
/// na systémový disk.
///
/// Drží se v zapisovacím vlákně, takže žádný zámek nepotřebuje.
#[derive(Default)]
pub struct NameCache {
    seen: std::collections::HashMap<u32, Seen>,
}

/// Naposledy zapsaný stav jedné instance.
struct Seen {
    create_time: i64,
    /// Klíč řádku v `proc_names_v` spolu s pidem.
    first_ts: i64,
    name: String,
    identity_key: String,
    app_name: String,
    publisher: Option<String>,
    written: i64,
}

/// Jak často se řádek přepíše, i když se nic nezměnilo — kvůli
/// `last_ts`, podle kterého se pozná, že pid ještě žije.
const NAME_REFRESH_S: i64 = 300;

/// Rozdíl mezi epochou FILETIME (1601) a unixovou v sekundách.
const FILETIME_UNIX_DIFF_S: i64 = 11_644_473_600;

/// Unixový čas vzniku instance. Z času vzniku procesu, ne z prvního
/// vzorku: po restartu služby už běžící proces jinak dostane začátek
/// „teď" a jeho dřívější vzorky by připadly předchozí instanci pidu.
/// Neznámý nebo nesmyslný čas vzniku (pid 0, budoucnost) = `ts`.
fn instance_start(create_time: i64, ts: i64) -> i64 {
    if create_time <= 0 {
        return ts;
    }
    let s = create_time / 10_000_000 - FILETIME_UNIX_DIFF_S;
    if s > 0 && s <= ts {
        s
    } else {
        ts
    }
}

impl NameCache {
    /// Má se řádek instance zapsat? Vrací jeho klíč `first_ts`, nebo
    /// None, když je zapsaný čerstvě. Zároveň si zapamatuje, co se
    /// zapisuje. Když pid v paměti není (start služby, nový proces),
    /// zeptá se databáze na jeho poslední instanci — jinak by každý
    /// restart služby založil všem běžícím procesům nový řádek.
    fn plan(
        &mut self,
        conn: &Connection,
        p: &ProcRow,
        ts: i64,
    ) -> Result<Option<i64>, rusqlite::Error> {
        let start = instance_start(p.create_time, ts);
        // Tatáž instance → obnoví se její řádek (last_ts, identita).
        // Jiný proces pod stejným pidem → nový řádek; starý zůstává pro
        // vzorky z doby, kdy pid patřil jemu.
        let first_ts = match self.seen.get(&p.pid) {
            Some(s) if s.create_time == p.create_time => {
                if s.name == p.name
                    && s.identity_key == p.identity_key
                    && s.app_name == p.app_name
                    && s.publisher == p.publisher
                    && ts - s.written < NAME_REFRESH_S
                {
                    return Ok(None);
                }
                s.first_ts
            }
            // Recyklovaný pid. Nová instance musí začít až po té staré,
            // jinak by dotaz historie pořád vybíral starou.
            Some(s) => start.max(s.first_ts + 1),
            None => {
                let last: Option<(i64, Option<i64>)> = conn
                    .prepare_cached(
                        "SELECT first_ts, create_time FROM proc_names_v
                         WHERE pid = ?1 ORDER BY first_ts DESC LIMIT 1",
                    )?
                    .query_row(params![p.pid as i64], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?;
                match last {
                    Some((f, Some(ct))) if ct == p.create_time => f,
                    // Řádek z doby před migrací (bez času vzniku) nebo
                    // jiný proces — nová instance.
                    Some((f, _)) => start.max(f + 1),
                    None => start,
                }
            }
        };
        self.seen.insert(
            p.pid,
            Seen {
                create_time: p.create_time,
                first_ts,
                name: p.name.clone(),
                identity_key: p.identity_key.clone(),
                app_name: p.app_name.clone(),
                publisher: p.publisher.clone(),
                written: ts,
            },
        );
        Ok(Some(first_ts))
    }

    /// Zapomene pidy, které v aktuálním vzorku nejsou — jinak by mapa
    /// rostla s každým procesem, který kdy běžel.
    fn retain(&mut self, procs: &[ProcRow]) {
        if self.seen.len() <= procs.len() * 2 {
            return;
        }
        let live: std::collections::HashSet<u32> = procs.iter().map(|p| p.pid).collect();
        self.seen.retain(|pid, _| live.contains(pid));
    }
}

/// Zapíše jeden tick sampleru (systém + všechny procesy) v transakci.
/// CPU se ukládá v promile (INTEGER), paměti v kB — dle SPEC kap. 8.
pub fn insert_tick(
    conn: &mut Connection,
    ts: i64,
    sys: &SystemSnapshot,
    procs: &[ProcRow],
    names: &mut NameCache,
) -> Result<(), rusqlite::Error> {
    let tx = conn.transaction()?;
    insert_tick_in(&tx, ts, sys, procs, names)?;
    tx.commit()
}

/// Totéž co [`insert_tick`], ale bez vlastní transakce: volající ji drží
/// otevřenou přes několik ticků a commituje po dávkách.
///
/// Proč dávky: SQLite zapisuje po stránkách (4 KB) a každý commit přepíše
/// do WAL všechny stránky, kterých se dotkl — i tu napůl plnou na konci
/// tabulky, kterou příští sekunda dopíše a přepíše znovu. Commit každou
/// sekundu tak znamenal 55 KB/s zápisu za data, kterých je pár kB
/// (změřeno na skutečném schématu: po 10 tickách 22 KB/s, tedy z ~4,8
/// na ~1,9 GB/den). Volající po chybě musí transakci vrátit
/// ([`crate::rollback_if_open`]) a zahodit `names` — cache by jinak
/// věřila řádkům, které rollback smazal.
pub fn insert_tick_in(
    tx: &Connection,
    ts: i64,
    sys: &SystemSnapshot,
    procs: &[ProcRow],
    names: &mut NameCache,
) -> Result<(), rusqlite::Error> {
    {
        tx.execute(
            "INSERT OR REPLACE INTO system_1s
                 (ts, cpu_pct, mem_used_mb, net_rx_bps, net_tx_bps, gpu_pct,
                  cpu_clock_mhz, cpu_clock_max_mhz,
                  gpu_temp_c, gpu_vram_mb, gpu_power_w, gpu_clock_mhz,
                  disk_qlen, disk_lat_ms, hard_flt_rate, thermal_throttle)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                     ?13, ?14, ?15, ?16)",
            params![
                ts,
                sys.cpu_pct as f64,
                sys.mem_used_mb as i64,
                sys.net_rx_bps as i64,
                sys.net_tx_bps as i64,
                sys.gpu_pct.map(|v| v as f64),
                sys.cpu_clock_mhz as i64,
                sys.cpu_clock_max_mhz as i64,
                sys.gpu.and_then(|g| g.temp_c).map(|v| v as f64),
                sys.gpu.and_then(|g| g.vram_used_mb).map(|v| v as i64),
                sys.gpu.and_then(|g| g.power_w).map(|v| v as f64),
                sys.gpu.and_then(|g| g.clock_mhz).map(|v| v as i64),
                sys.disk_qlen as f64,
                sys.disk_lat_ms as f64,
                sys.hard_flt_rate as i64,
                sys.thermal_throttle as i64,
            ],
        )?;

        // Jádra CPU a disky — historie pro detail sekci a per-disk grafy.
        let mut core_stmt = tx
            .prepare_cached("INSERT OR REPLACE INTO core_1s (ts, core, pct) VALUES (?1, ?2, ?3)")?;
        for (i, pct) in sys.cores.iter().enumerate() {
            core_stmt.execute(params![ts, i as i64, *pct as f64])?;
        }
        let mut disk_stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO disk_1s (ts, disk, r_bps, w_bps) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for d in &sys.disks {
            disk_stmt.execute(params![ts, d.index as i64, d.r_bps as i64, d.w_bps as i64])?;
        }

        let mut stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO sample_1s
                 (ts, proc_id, cpu_pm, ws_kb, priv_kb, io_r, io_w, gpu_pm)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        // Jména + identita pro čtení historie (instance → poslední stav) —
        // náhled minulosti tak seskupuje a ikonuje stejně jako živý list.
        // Upsert i pro „obnovu": kdyby tick s novým řádkem spadl a vrátil
        // se, cache by o řádku věděla a holý UPDATE by ho už nikdy nezaložil.
        let mut name_stmt = tx.prepare_cached(
            "INSERT INTO proc_names_v
                 (pid, first_ts, create_time, name, last_ts,
                  identity_key, app_name, publisher)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (pid, first_ts) DO UPDATE SET
                 create_time = excluded.create_time,
                 name = excluded.name,
                 last_ts = excluded.last_ts,
                 identity_key = excluded.identity_key,
                 app_name = excluded.app_name,
                 publisher = excluded.publisher",
        )?;
        for p in procs {
            stmt.execute(params![
                ts,
                p.pid as i64,
                (p.cpu_pct * 10.0) as i64,
                (p.ws_bytes / 1024) as i64,
                (p.priv_bytes / 1024) as i64,
                p.disk_r_bps as i64,
                p.disk_w_bps as i64,
                (p.gpu_pct * 10.0) as i64,
            ])?;
            if let Some(first_ts) = names.plan(tx, p, ts)? {
                name_stmt.execute(params![
                    p.pid as i64,
                    first_ts,
                    p.create_time,
                    p.name,
                    ts,
                    p.identity_key,
                    p.app_name,
                    p.publisher,
                ])?;
            }
        }
    }
    names.retain(procs);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FILETIME z unixového času.
    fn ft(unix: i64) -> i64 {
        (unix + FILETIME_UNIX_DIFF_S) * 10_000_000
    }

    fn proc(pid: u32, name: &str, vznik: i64) -> ProcRow {
        ProcRow {
            pid,
            name: name.into(),
            create_time: ft(vznik),
            ..Default::default()
        }
    }

    fn jmeno_v(conn: &Connection, ts: i64) -> String {
        let (_, rows) = crate::history::procs_at(conn, ts).unwrap().expect("vzorek");
        rows.into_iter().next().expect("řádek").name
    }

    // Recyklovaný PID: starý vzorek si nechá jméno procesu, kterému
    // patřil, ne toho, kdo ten PID nese dnes.
    #[test]
    fn recyklovany_pid_nepreda_jmeno_do_minulosti() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        let mut names = NameCache::default();
        let sys = SystemSnapshot::default();
        insert_tick(
            &mut conn,
            1_000_000,
            &sys,
            &[proc(4242, "stary.exe", 999_000)],
            &mut names,
        )
        .unwrap();
        insert_tick(
            &mut conn,
            1_000_500,
            &sys,
            &[proc(4242, "novy.exe", 1_000_400)],
            &mut names,
        )
        .unwrap();
        assert_eq!(jmeno_v(&conn, 1_000_000), "stary.exe");
        assert_eq!(jmeno_v(&conn, 1_000_500), "novy.exe");
    }

    // Restart služby (prázdná cache) nesmí běžícímu procesu založit
    // novou instanci — jinak by tabulka rostla s každým startem.
    #[test]
    fn restart_sluzby_nezalozi_novou_instanci() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        let sys = SystemSnapshot::default();
        let p = proc(77, "bezi.exe", 500_000);
        insert_tick(
            &mut conn,
            600_000,
            &sys,
            std::slice::from_ref(&p),
            &mut NameCache::default(),
        )
        .unwrap();
        insert_tick(&mut conn, 700_000, &sys, &[p], &mut NameCache::default()).unwrap();
        let (n, last): (i64, i64) = conn
            .query_row("SELECT COUNT(*), MAX(last_ts) FROM proc_names_v", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(n, 1, "restart založil další instanci");
        assert_eq!(last, 700_000, "last_ts se neobnovil");
    }

    // Řádek z doby před migrací (jen pid, bez času vzniku) zůstane pro
    // starou historii; nový proces pod tím pidem dostane vlastní řádek.
    #[test]
    fn stary_radek_slouzi_jen_minulosti() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO proc_names_v (pid, first_ts, name, last_ts) VALUES (9, 0, 'pred.exe', 100)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sample_1s (ts, proc_id, cpu_pm, ws_kb) VALUES (50, 9, 0, 0)",
            [],
        )
        .unwrap();
        insert_tick(
            &mut conn,
            2_000_000,
            &SystemSnapshot::default(),
            &[proc(9, "po.exe", 1_999_990)],
            &mut NameCache::default(),
        )
        .unwrap();
        assert_eq!(jmeno_v(&conn, 50), "pred.exe");
        assert_eq!(jmeno_v(&conn, 2_000_000), "po.exe");
    }
}
