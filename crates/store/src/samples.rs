//! Zápis vzorků sampleru do SQLite (v1 flusher, SPEC kap. 3.4/8).
//! Volá se z jediného zapisovacího vlákna služby, dávkově v transakci.

use core_types::proc::{DiskDesc, ProcRow, SystemSnapshot};
use rusqlite::{params, Connection};

/// Zapíše názvy disků (jednou při startu služby).
pub fn upsert_disk_names(conn: &Connection, disks: &[DiskDesc]) -> Result<(), rusqlite::Error> {
    let mut stmt =
        conn.prepare_cached("INSERT OR REPLACE INTO disk_names (disk, name) VALUES (?1, ?2)")?;
    for d in disks {
        stmt.execute(params![d.index as i64, d.model])?;
    }
    Ok(())
}

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
            "INSERT OR REPLACE INTO system_1s
                 (ts, cpu_pct, mem_used_mb, net_rx_bps, net_tx_bps, gpu_pct,
                  cpu_clock_mhz, cpu_clock_max_mhz,
                  gpu_temp_c, gpu_vram_mb, gpu_power_w, gpu_clock_mhz)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            "INSERT OR REPLACE INTO sample_1s (ts, proc_id, cpu_pm, ws_kb, priv_kb, io_r, io_w)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        // Jména + identita pro čtení historie (pid → poslední stav) —
        // náhled minulosti tak seskupuje a ikonuje stejně jako živý list.
        let mut name_stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO proc_names
                 (pid, name, last_ts, identity_key, app_name, publisher)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
            ])?;
            name_stmt.execute(params![
                p.pid as i64,
                p.name,
                ts,
                p.identity_key,
                p.app_name,
                p.publisher,
            ])?;
        }
    }
    tx.commit()
}
