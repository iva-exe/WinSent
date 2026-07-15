//! Čtení historie vzorků pro UI (graf do minulosti, stav tasků v čase).
//! Volá se z read-only spojení v IPC handleru (WAL: čtenáři neblokují
//! zapisovací vlákno).

use core_types::proc::{HistProcRow, SystemPoint};
use rusqlite::{params, Connection};

/// Systémové body v rozsahu [from, to] (unix s, včetně).
pub fn system_history(
    conn: &Connection,
    from: i64,
    to: i64,
) -> Result<Vec<SystemPoint>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT ts, cpu_pct, mem_used_mb, net_rx_bps, net_tx_bps
         FROM system_1s WHERE ts BETWEEN ?1 AND ?2 ORDER BY ts",
    )?;
    let rows = stmt.query_map(params![from, to], |r| {
        Ok(SystemPoint {
            ts: r.get(0)?,
            cpu_pct: r.get::<_, f64>(1)? as f32,
            mem_used_mb: r.get::<_, i64>(2)?.max(0) as u64,
            net_rx_bps: r.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64,
            net_tx_bps: r.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64,
        })
    })?;
    rows.collect()
}

/// Stav procesů v čase `ts` — nejbližší existující vzorek ±2 s.
/// Vrací (skutečný ts vzorku, řádky); None když v okně nic není.
pub fn procs_at(
    conn: &Connection,
    ts: i64,
) -> Result<Option<(i64, Vec<HistProcRow>)>, rusqlite::Error> {
    // Nejbližší vzorek podle |odchylky|, max 2 s daleko.
    let actual: Option<i64> = conn
        .query_row(
            "SELECT ts FROM system_1s WHERE ts BETWEEN ?1 - 2 AND ?1 + 2
             ORDER BY ABS(ts - ?1) LIMIT 1",
            params![ts],
            |r| r.get(0),
        )
        .map(Some)
        .unwrap_or(None);
    let Some(actual) = actual else {
        return Ok(None);
    };

    let mut stmt = conn.prepare_cached(
        "SELECT s.proc_id, COALESCE(n.name, '(pid ' || s.proc_id || ')'),
                s.cpu_pm, s.ws_kb
         FROM sample_1s s LEFT JOIN proc_names n ON n.pid = s.proc_id
         WHERE s.ts = ?1",
    )?;
    let rows = stmt
        .query_map(params![actual], |r| {
            Ok(HistProcRow {
                pid: r.get::<_, i64>(0)? as u32,
                name: r.get(1)?,
                cpu_pct: r.get::<_, Option<i64>>(2)?.unwrap_or(0) as f32 / 10.0,
                ws_bytes: r.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64 * 1024,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some((actual, rows)))
}
