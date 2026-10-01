//! Čtení historie vzorků pro UI (graf do minulosti, stav tasků v čase).
//! Volá se z read-only spojení v IPC handleru (WAL: čtenáři neblokují
//! zapisovací vlákno).

use core_types::proc::{DiskRate, GpuInfo, HistProcRow, SystemPoint};
use rusqlite::{params, Connection};

/// Detaily proměnných v čase `ts`: jádra CPU, disky a GPU senzory. Pro
/// detail sekci při zámku grafu.
///
/// Hledá se napříč retenční kaskádou stejně jako v `procs_at`: 1s (±2 s),
/// pak 10s (±10 s), pak 1m (±60 s). Dřív se hledalo jen v 1s tabulkách,
/// které žijí hodinu — pro starší zámek služba vrátila chybu a UI
/// ukázalo u zamčeného času živé disky a GPU, aniž by to bylo poznat.
/// Agregáty nesou méně: jádra jen v 1s úrovni (core_1s se neagreguje),
/// z GPU jen teplotu.
#[allow(clippy::type_complexity)]
pub fn detail_at(
    conn: &Connection,
    ts: i64,
) -> Result<Option<(i64, Vec<f32>, Vec<DiskRate>, Option<GpuInfo>)>, rusqlite::Error> {
    // Úrovně kaskády: (systémová tabulka, disková tabulka, tolerance, surová?).
    for (sys_table, disk_table, tol, raw) in [
        ("system_1s", "disk_1s", 2, true),
        ("system_10s", "disk_10s", 10, false),
        ("system_1m", "disk_1m", 60, false),
    ] {
        let actual: Option<i64> = conn
            .query_row(
                &format!(
                    "SELECT ts FROM {sys_table} WHERE ts BETWEEN ?1 - {tol} AND ?1 + {tol}
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

        let cores: Vec<f32> = if raw {
            let mut stmt =
                conn.prepare_cached("SELECT pct FROM core_1s WHERE ts = ?1 ORDER BY core")?;
            let v = stmt
                .query_map(params![actual], |r| {
                    Ok(r.get::<_, Option<f64>>(0)?.unwrap_or(0.0) as f32)
                })?
                .collect::<Result<_, _>>()?;
            v
        } else {
            Vec::new()
        };

        let mut stmt = conn.prepare_cached(&format!(
            "SELECT disk, r_bps, w_bps FROM {disk_table} WHERE ts = ?1 ORDER BY disk"
        ))?;
        let disks: Vec<DiskRate> = stmt
            .query_map(params![actual], |r| {
                Ok(DiskRate {
                    index: r.get::<_, i64>(0)? as u32,
                    r_bps: r.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
                    w_bps: r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
                })
            })?
            .collect::<Result<_, _>>()?;

        // GPU senzory (NULL = tehdy nedostupné). Agregáty drží jen
        // teplotu — VRAM, příkon a takt se do nich nepřenáší.
        let sql = if raw {
            "SELECT gpu_temp_c, gpu_vram_mb, gpu_power_w, gpu_clock_mhz
             FROM system_1s WHERE ts = ?1"
                .to_string()
        } else {
            format!("SELECT gpu_temp_c, NULL, NULL, NULL FROM {sys_table} WHERE ts = ?1")
        };
        let gpu = conn
            .query_row(&sql, params![actual], |r| {
                Ok(GpuInfo {
                    temp_c: r.get::<_, Option<f64>>(0)?.map(|v| v as f32),
                    vram_used_mb: r.get::<_, Option<i64>>(1)?.map(|v| v.max(0) as u64),
                    vram_total_mb: None,
                    power_w: r.get::<_, Option<f64>>(2)?.map(|v| v as f32),
                    clock_mhz: r.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u32),
                })
            })
            .ok()
            .filter(|g: &GpuInfo| g.temp_c.is_some() || g.vram_used_mb.is_some());

        return Ok(Some((actual, cores, disks, gpu)));
    }
    Ok(None)
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
    // Úrovně kaskády: (tabulka, tolerance hledání, délka bucketu − 1).
    for (table, tol, span) in [
        ("sample_1s", 2, 0),
        ("sample_10s", 10, 9),
        ("sample_1m", 60, 59),
    ] {
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

        // Jméno patří INSTANCI procesu, ne PID: bere se poslední instance
        // toho pidu, která začala nejpozději v čase vzorku. Dřív se
        // spojovalo jen přes pid a tabulka držela poslední jméno — po
        // recyklaci PID (každý restart) dostal týdny starý vzorek jméno,
        // ikonu i vydavatele procesu, který ten PID nesl až dnes. Agregát
        // má ts = začátek bucketu, proto tolerance o délku bucketu.
        // Vzorek bez odpovídající instance dostane „(pid N)".
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT s.proc_id, COALESCE(n.name, '(pid ' || s.proc_id || ')'),
                    s.cpu_pm, s.ws_kb, s.io_r, s.io_w, s.gpu_pm,
                    n.identity_key, n.app_name, n.publisher
             FROM {table} s LEFT JOIN proc_names_v n ON n.pid = s.proc_id
                AND n.first_ts = (SELECT MAX(v.first_ts) FROM proc_names_v v
                                  WHERE v.pid = s.proc_id AND v.first_ts <= s.ts + {span})
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

#[cfg(test)]
mod tests {
    use super::*;

    // Zámek grafu starší než hodina: 1s tabulky už jsou pryč, detail se
    // musí najít v agregátech — jinak UI ukázalo živé disky a GPU.
    #[test]
    fn detail_najde_i_agregat() {
        let conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO system_10s (ts, cpu_pct, mem_used_mb, gpu_temp_c) VALUES (1000, 5, 1, 61.5);
             INSERT INTO disk_10s (ts, disk, r_bps, w_bps) VALUES (1000, 0, 111, 222);
             INSERT INTO system_1m (ts, cpu_pct, mem_used_mb) VALUES (5000, 5, 1);",
        )
        .unwrap();
        let (ts, cores, disks, gpu) = detail_at(&conn, 1004).unwrap().expect("10s detail");
        assert_eq!(ts, 1000);
        assert!(cores.is_empty(), "jádra se neagregují");
        assert_eq!(disks.len(), 1);
        assert_eq!((disks[0].r_bps, disks[0].w_bps), (111, 222));
        let gpu = gpu.expect("teplota GPU z agregátu");
        assert_eq!(gpu.temp_c, Some(61.5));
        assert_eq!(gpu.vram_used_mb, None);
        // 1m úroveň bez GPU dat: detail je, GPU ne.
        let (ts, _, disks, gpu) = detail_at(&conn, 5030).unwrap().expect("1m detail");
        assert_eq!(ts, 5000);
        assert!(disks.is_empty());
        assert!(gpu.is_none());
        assert!(detail_at(&conn, 99_999).unwrap().is_none());
    }
}
