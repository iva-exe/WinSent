//! Čtení historie vzorků pro UI (graf do minulosti, stav tasků v čase).
//! Volá se z read-only spojení v IPC handleru (WAL: čtenáři neblokují
//! zapisovací vlákno).

use core_types::proc::{DiskRate, GpuInfo, HistProcRow, SystemPoint};
use rusqlite::{params, Connection};

/// Detaily proměnných v čase `ts` (nejbližší vzorek ±2 s): jádra CPU,
/// disky a GPU senzory. Pro detail sekci při zámku grafu.
#[allow(clippy::type_complexity)]
pub fn detail_at(
    conn: &Connection,
    ts: i64,
) -> Result<Option<(i64, Vec<f32>, Vec<DiskRate>, Option<GpuInfo>)>, rusqlite::Error> {
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

    let mut stmt = conn.prepare_cached("SELECT pct FROM core_1s WHERE ts = ?1 ORDER BY core")?;
    let cores: Vec<f32> = stmt
        .query_map(params![actual], |r| {
            Ok(r.get::<_, Option<f64>>(0)?.unwrap_or(0.0) as f32)
        })?
        .collect::<Result<_, _>>()?;

    let mut stmt =
        conn.prepare_cached("SELECT disk, r_bps, w_bps FROM disk_1s WHERE ts = ?1 ORDER BY disk")?;
    let disks: Vec<DiskRate> = stmt
        .query_map(params![actual], |r| {
            Ok(DiskRate {
                index: r.get::<_, i64>(0)? as u32,
                r_bps: r.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
                w_bps: r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
            })
        })?
        .collect::<Result<_, _>>()?;

    // GPU senzory z system_1s (NULL = tehdy nedostupné).
    let gpu = conn
        .query_row(
            "SELECT gpu_temp_c, gpu_vram_mb, gpu_power_w, gpu_clock_mhz
             FROM system_1s WHERE ts = ?1",
            params![actual],
            |r| {
                Ok(GpuInfo {
                    temp_c: r.get::<_, Option<f64>>(0)?.map(|v| v as f32),
                    vram_used_mb: r.get::<_, Option<i64>>(1)?.map(|v| v.max(0) as u64),
                    vram_total_mb: None,
                    power_w: r.get::<_, Option<f64>>(2)?.map(|v| v as f32),
                    clock_mhz: r.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u32),
                })
            },
        )
        .ok()
        .filter(|g: &GpuInfo| g.temp_c.is_some() || g.vram_used_mb.is_some());

    Ok(Some((actual, cores, disks, gpu)))
}

/// Historie jader [from, to]: (ts, jádro, pct).
pub fn core_history(
    conn: &Connection,
    from: i64,
    to: i64,
) -> Result<Vec<(i64, u32, f32)>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT ts, core, pct FROM core_1s
         WHERE ts BETWEEN ?1 AND ?2 ORDER BY ts, core",
    )?;
    let rows = stmt.query_map(params![from, to], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)? as u32,
            r.get::<_, Option<f64>>(2)?.unwrap_or(0.0) as f32,
        ))
    })?;
    rows.collect()
}

/// Historie disků [from, to]: (ts, disk, r_bps, w_bps).
pub fn disk_history(
    conn: &Connection,
    from: i64,
    to: i64,
) -> Result<Vec<(i64, u32, u64, u64)>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT ts, disk, r_bps, w_bps FROM disk_1s
         WHERE ts BETWEEN ?1 AND ?2
         UNION ALL
         SELECT ts, disk, r_bps, w_bps FROM disk_10s
         WHERE ts BETWEEN ?1 AND ?2
         UNION ALL
         SELECT ts, disk, r_bps, w_bps FROM disk_1m
         WHERE ts BETWEEN ?1 AND ?2
         ORDER BY ts, disk",
    )?;
    let rows = stmt.query_map(params![from, to], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)? as u32,
            r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
            r.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64,
        ))
    })?;
    rows.collect()
}

/// Systémové body v rozsahu [from, to] (unix s, včetně). Čte napříč
/// retenční kaskádou (1s → 10s → 1m) — úrovně se nepřekrývají (retence
/// maže po agregaci), takže UNION ALL stačí; body jen řídnou s věkem.
pub fn system_history(
    conn: &Connection,
    from: i64,
    to: i64,
) -> Result<Vec<SystemPoint>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT ts, cpu_pct, mem_used_mb, net_rx_bps, net_tx_bps, gpu_pct
         FROM system_1s WHERE ts BETWEEN ?1 AND ?2
         UNION ALL
         SELECT ts, cpu_pct, mem_used_mb, net_rx_bps, net_tx_bps, gpu_pct
         FROM system_10s WHERE ts BETWEEN ?1 AND ?2
         UNION ALL
         SELECT ts, cpu_pct, mem_used_mb, net_rx_bps, net_tx_bps, gpu_pct
         FROM system_1m WHERE ts BETWEEN ?1 AND ?2
         ORDER BY ts",
    )?;
    let rows = stmt.query_map(params![from, to], |r| {
        Ok(SystemPoint {
            ts: r.get(0)?,
            cpu_pct: r.get::<_, f64>(1)? as f32,
            mem_used_mb: r.get::<_, i64>(2)?.max(0) as u64,
            net_rx_bps: r.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64,
            net_tx_bps: r.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64,
            gpu_pct: r.get::<_, Option<f64>>(5)?.map(|v| v as f32),
        })
    })?;
    rows.collect()
}

/// Stav procesů v čase `ts` — nejbližší existující vzorek. Hledá se
/// napříč retenční kaskádou: 1s (±2 s), pak 10s bucket (±10 s), pak
/// 1m bucket (±60 s) — starší náhled je z agregátů (avg za bucket).
/// Vrací (skutečný ts vzorku, řádky); None když v okně nic není.
pub fn procs_at(
    conn: &Connection,
    ts: i64,
) -> Result<Option<(i64, Vec<HistProcRow>)>, rusqlite::Error> {
    // Úrovně kaskády: (tabulka, tolerance hledání).
    for (table, tol) in [("sample_1s", 2), ("sample_10s", 10), ("sample_1m", 60)] {
        let actual: Option<i64> = conn
            .query_row(
                &format!(
                    "SELECT ts FROM {table} WHERE ts BETWEEN ?1 - {tol} AND ?1 + {tol}
                     ORDER BY ABS(ts - ?1) LIMIT 1"
                ),
                params![ts],
                |r| r.get(0),
            )
            .map(Some)
            .unwrap_or(None);
        let Some(actual) = actual else {
            continue;
        };

        let mut stmt = conn.prepare_cached(&format!(
            "SELECT s.proc_id, COALESCE(n.name, '(pid ' || s.proc_id || ')'),
                    s.cpu_pm, s.ws_kb, s.io_r, s.io_w, s.gpu_pm,
                    n.identity_key, n.app_name, n.publisher
             FROM {table} s LEFT JOIN proc_names n ON n.pid = s.proc_id
             WHERE s.ts = ?1"
        ))?;
        let rows = stmt
            .query_map(params![actual], |r| {
                Ok(HistProcRow {
                    pid: r.get::<_, i64>(0)? as u32,
                    name: r.get(1)?,
                    cpu_pct: r.get::<_, Option<i64>>(2)?.unwrap_or(0) as f32 / 10.0,
                    ws_bytes: r.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64 * 1024,
                    disk_r_bps: r.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64,
                    disk_w_bps: r.get::<_, Option<i64>>(5)?.unwrap_or(0).max(0) as u64,
                    // NULL = vzorek z doby před přidáním sloupce; „neznámo" je
                    // něco jiného než „nula procent" a UI to rozlišuje.
                    gpu_pct: r.get::<_, Option<i64>>(6)?.map(|v| v as f32 / 10.0),
                    identity_key: r.get(7)?,
                    app_name: r.get(8)?,
                    publisher: r.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Some((actual, rows)));
    }
    Ok(None)
}
