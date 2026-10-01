//! Retenční kaskáda (SPEC kap. 8) — běží v zapisovacím vlákně služby
//! na BELOW_NORMAL prioritě, jeden krok za minutu.
//!
//! v3 naostro: před smazáním se agreguje (avg + max, ať špička nezmizí):
//!
//! ```text
//! sample_1s   → 1 hodina  → agreguj do sample_10s → smaž
//! sample_10s  → 7 dní     → agreguj do sample_1m  → smaž
//! sample_1m   → 1 rok     → smaž
//! event/incident → navždy (jsou malé)
//! ```
//!
//! Hranice řezu se zarovnávají na velikost bucketu — jinak by se
//! poslední, nekompletní bucket zapsal napůl a zbytek řádků by pak
//! INSERT OR IGNORE zahodil. core_1s se neagreguje (per-jádro detail
//! má smysl jen čerstvý), jen maže.

use rusqlite::{Connection, Transaction, TransactionBehavior};

/// Retence surových 1s vzorků: 1 hodina.
const KEEP_1S_S: i64 = 3600;
/// Retence 10s agregátů: 7 dní.
const KEEP_10S_S: i64 = 7 * 86_400;
/// Retence 1m agregátů: 1 rok.
const KEEP_1M_S: i64 = 366 * 86_400;

/// Jména instancí procesů (`proc_names_v`) se drží o hodinu déle než
/// poslední 1m vzorky — řádek instance se obnovuje jen jednou za pět
/// minut, takže vzorek bývá o kus novější než jeho `last_ts`.
const KEEP_NAMES_SLACK_S: i64 = 3600;

/// Provede dávku SQL v jedné transakci, která se při chybě VRÁTÍ.
///
/// Dřív kroky posílaly holé `BEGIN; … COMMIT;` přes `execute_batch`.
/// Když příkaz uprostřed selhal (typicky SQLITE_BUSY, protože cizí
/// nástroj nad databází držel zápisový zámek déle než busy_timeout),
/// dávka skončila chybou dřív, než došla ke COMMIT — a zapisovací
/// spojení zůstalo navždy uvnitř transakce. Každý další insert_tick
/// pak padal na „cannot start a transaction within a transaction"
/// a do restartu služby se nezapsal jediný vzorek; události zapsané
/// mimo transakci se při ukončení zahodily rollbackem. `Transaction`
/// při dropu bez commitu pošle ROLLBACK sám.
///
/// IMMEDIATE bere zápisový zámek hned na BEGIN: busy handler pak čeká
/// korektně, místo aby deferred transakce narazila na zámek až při
/// povýšení čtení na zápis (tam WAL vrací BUSY bez čekání).
fn in_tx(conn: &Connection, sql: &str) -> Result<(), rusqlite::Error> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    tx.execute_batch(sql)?;
    tx.commit()
}

/// Jeden krok retenční kaskády.
pub fn tick(conn: &Connection) -> Result<(), rusqlite::Error> {
    let now = chrono_now_unix();

    // ── 1s → 10s (zarovnáno na 10 s) ──
    let cut = (now - KEEP_1S_S) / 10 * 10;
    in_tx(
        conn,
        &format!(
            "INSERT OR IGNORE INTO system_10s
            (ts, cpu_pct, cpu_pct_max, mem_used_mb, net_rx_bps, net_tx_bps,
             gpu_pct, gpu_pct_max, gpu_temp_c, cpu_clock_mhz)
        SELECT (ts/10)*10, AVG(cpu_pct), MAX(cpu_pct),
               CAST(AVG(mem_used_mb) AS INTEGER),
               CAST(AVG(net_rx_bps) AS INTEGER), CAST(AVG(net_tx_bps) AS INTEGER),
               AVG(gpu_pct), MAX(gpu_pct), AVG(gpu_temp_c),
               CAST(AVG(cpu_clock_mhz) AS INTEGER)
        FROM system_1s WHERE ts < {cut} GROUP BY ts/10;
        INSERT OR IGNORE INTO sample_10s
            (ts, proc_id, cpu_pm, cpu_pm_max, ws_kb, io_r, io_w, gpu_pm, gpu_pm_max)
        SELECT (ts/10)*10, proc_id, CAST(AVG(cpu_pm) AS INTEGER), MAX(cpu_pm),
               CAST(AVG(ws_kb) AS INTEGER),
               CAST(AVG(io_r) AS INTEGER), CAST(AVG(io_w) AS INTEGER),
               CAST(AVG(gpu_pm) AS INTEGER), MAX(gpu_pm)
        FROM sample_1s WHERE ts < {cut} GROUP BY ts/10, proc_id;
        INSERT OR IGNORE INTO disk_10s (ts, disk, r_bps, w_bps)
        SELECT (ts/10)*10, disk, CAST(AVG(r_bps) AS INTEGER),
               CAST(AVG(w_bps) AS INTEGER)
        FROM disk_1s WHERE ts < {cut} GROUP BY ts/10, disk;
        DELETE FROM system_1s WHERE ts < {cut};
        DELETE FROM sample_1s WHERE ts < {cut};
        DELETE FROM disk_1s   WHERE ts < {cut};
        DELETE FROM core_1s   WHERE ts < {cut};"
        ),
    )?;

    // ── 10s → 1m (zarovnáno na 60 s); avg z avg je OK — buckety mají
    // stejnou váhu, max se přenáší jako max z max ──
    let cut = (now - KEEP_10S_S) / 60 * 60;
    in_tx(
        conn,
        &format!(
            "INSERT OR IGNORE INTO system_1m
            (ts, cpu_pct, cpu_pct_max, mem_used_mb, net_rx_bps, net_tx_bps,
             gpu_pct, gpu_pct_max, gpu_temp_c, cpu_clock_mhz)
        SELECT (ts/60)*60, AVG(cpu_pct), MAX(cpu_pct_max),
               CAST(AVG(mem_used_mb) AS INTEGER),
               CAST(AVG(net_rx_bps) AS INTEGER), CAST(AVG(net_tx_bps) AS INTEGER),
               AVG(gpu_pct), MAX(gpu_pct_max), AVG(gpu_temp_c),
               CAST(AVG(cpu_clock_mhz) AS INTEGER)
        FROM system_10s WHERE ts < {cut} GROUP BY ts/60;
        INSERT OR IGNORE INTO sample_1m
            (ts, proc_id, cpu_pm, cpu_pm_max, ws_kb, io_r, io_w, gpu_pm, gpu_pm_max)
        SELECT (ts/60)*60, proc_id, CAST(AVG(cpu_pm) AS INTEGER), MAX(cpu_pm_max),
               CAST(AVG(ws_kb) AS INTEGER),
               CAST(AVG(io_r) AS INTEGER), CAST(AVG(io_w) AS INTEGER),
               CAST(AVG(gpu_pm) AS INTEGER), MAX(gpu_pm_max)
        FROM sample_10s WHERE ts < {cut} GROUP BY ts/60, proc_id;
        INSERT OR IGNORE INTO disk_1m (ts, disk, r_bps, w_bps)
        SELECT (ts/60)*60, disk, CAST(AVG(r_bps) AS INTEGER),
               CAST(AVG(w_bps) AS INTEGER)
        FROM disk_10s WHERE ts < {cut} GROUP BY ts/60, disk;
        DELETE FROM system_10s WHERE ts < {cut};
        DELETE FROM sample_10s WHERE ts < {cut};
        DELETE FROM disk_10s   WHERE ts < {cut};"
        ),
    )?;

    // ── 1m → po roce pryč ──
    // Spolu se vzorky odchází i jména instancí, na které už žádný
    // vzorek neukazuje — instance procesů přibývají s každým spuštěním,
    // takže bez řezu by tabulka rostla do nekonečna.
    let cut = now - KEEP_1M_S;
    let names_cut = cut - KEEP_NAMES_SLACK_S;
    in_tx(
        conn,
        &format!(
            "DELETE FROM system_1m WHERE ts < {cut};
        DELETE FROM sample_1m WHERE ts < {cut};
        DELETE FROM disk_1m   WHERE ts < {cut};
        DELETE FROM proc_names_v WHERE last_ts < {names_cut};"
        ),
    )?;

    Ok(())
}

/// Unix čas bez závislosti na chrono crate.
fn chrono_now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    // Kaskáda: staré 1s vzorky se agregují do 10s (avg+max) a smažou.
    #[test]
    fn cascade_aggregates_before_delete() {
        let conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        let old = chrono_now_unix() - KEEP_1S_S - 100;
        let bucket = old / 10 * 10;
        // Dva vzorky v jednom bucketu: cpu 10 % a 30 % → avg 20, max 30.
        for (ts, cpu) in [(bucket, 10.0), (bucket + 1, 30.0)] {
            conn.execute(
                "INSERT INTO system_1s (ts, cpu_pct, mem_used_mb) VALUES (?1, ?2, 1000)",
                params![ts, cpu],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sample_1s (ts, proc_id, cpu_pm, ws_kb) VALUES (?1, 42, ?2, 500)",
                params![ts, (cpu * 10.0) as i64],
            )
            .unwrap();
        }
        tick(&conn).unwrap();

        let n1s: i64 = conn
            .query_row("SELECT COUNT(*) FROM system_1s", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n1s, 0, "staré 1s vzorky mají být smazané");
        let (avg, max): (f64, f64) = conn
            .query_row(
                "SELECT cpu_pct, cpu_pct_max FROM system_10s WHERE ts = ?1",
                params![bucket],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((avg - 20.0).abs() < 0.01, "avg: {avg}");
        assert!((max - 30.0).abs() < 0.01, "max: {max}");
        let (pm, pm_max): (i64, i64) = conn
            .query_row(
                "SELECT cpu_pm, cpu_pm_max FROM sample_10s WHERE ts = ?1 AND proc_id = 42",
                params![bucket],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(pm, 200);
        assert_eq!(pm_max, 300);
    }

    // Krok, který selže uprostřed dávky, nesmí nechat spojení viset
    // v transakci — jinak by se do restartu služby nezapsal žádný vzorek.
    #[test]
    fn selhany_krok_vrati_transakci() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        let old = chrono_now_unix() - KEEP_1S_S - 100;
        conn.execute(
            "INSERT INTO system_1s (ts, cpu_pct, mem_used_mb) VALUES (?1, 5.0, 1000)",
            params![old],
        )
        .unwrap();
        // Chybějící tabulka shodí třetí příkaz dávky, první dva už proběhly.
        conn.execute_batch("DROP TABLE disk_10s;").unwrap();
        assert!(tick(&conn).is_err(), "krok měl selhat");
        assert!(conn.is_autocommit(), "spojení zůstalo v transakci");
        let n10: i64 = conn
            .query_row("SELECT COUNT(*) FROM system_10s", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n10, 0, "napůl provedený krok se nevrátil");
        // Zápis vzorků (insert_tick) musí jít dál.
        conn.transaction()
            .expect("nová transakce")
            .commit()
            .unwrap();
    }

    // Mazání jmen instancí běží každou minutu v zápisové transakci —
    // musí jít přes index, ne průchodem celé (roční) tabulky.
    #[test]
    fn mazani_jmen_jde_pres_index() {
        let conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        let plan: Vec<String> = conn
            .prepare("EXPLAIN QUERY PLAN DELETE FROM proc_names_v WHERE last_ts < 100")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            plan.iter().any(|d| d.contains("proc_names_v_last")),
            "plán nepoužívá index: {plan:?}"
        );
    }

    // Cizí zapisovatel držící zámek: krok skončí BUSY, ale spojení se
    // z toho vzpamatuje a další krok po uvolnění zámku projde.
    #[test]
    fn zamek_ciziho_zapisovatele_nezablokuje_spojeni() {
        let dir = std::env::temp_dir().join("winsent-test-retence-busy");
        let _ = std::fs::remove_dir_all(&dir);
        let db = dir.join("s.db");
        let conn = crate::open(&db).unwrap();
        conn.busy_timeout(std::time::Duration::from_millis(50))
            .unwrap();
        let cizi = Connection::open(&db).unwrap();
        cizi.execute_batch("BEGIN IMMEDIATE;").unwrap();
        assert!(tick(&conn).is_err(), "krok měl narazit na zámek");
        assert!(conn.is_autocommit(), "spojení zůstalo v transakci");
        cizi.execute_batch("COMMIT;").unwrap();
        tick(&conn).expect("po uvolnění zámku má krok projít");
    }
}
