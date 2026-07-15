//! Zápis vzorků sampleru do SQLite (v1 flusher, SPEC kap. 3.4/8).
//! Volá se z jediného zapisovacího vlákna služby, dávkově v transakci.

use core_types::proc::{ProcRow, SystemSnapshot};
use rusqlite::{params, Connection};

/// Zapíše jeden tick sampleru (systém + všechny procesy) v transakci.
/// CPU se ukládá v promile (INTEGER), paměti v kB — dle SPEC kap. 8.
pub fn insert_tick(
    conn: &mut Connection,
    ts: i64,
    sys: &SystemSnapshot,
    procs: &[ProcRow],
) -> Result<(), rusqlite::Error> {
    let tx = conn.transaction()?;
    {
        tx.execute(
            "INSERT OR REPLACE INTO system_1s (ts, cpu_pct, mem_used_mb) VALUES (?1, ?2, ?3)",
            params![ts, sys.cpu_pct as f64, sys.mem_used_mb as i64],
        )?;

        let mut stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO sample_1s (ts, proc_id, cpu_pm, ws_kb, priv_kb)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for p in procs {
            stmt.execute(params![
                ts,
                p.pid as i64,
                (p.cpu_pct * 10.0) as i64,
                (p.ws_bytes / 1024) as i64,
                (p.priv_bytes / 1024) as i64,
            ])?;
        }
    }
    tx.commit()
}
