//! Zápis a čtení událostí a incidentů (SPEC kap. 16). Události jsou
//! drobné trvalé záznamy (zásek, start/stop procesu…); incident je
//! „velká" věc s forenzním oknem (pád, hang, BSOD, zásek s viníkem).
//! Obě tabulky jsou trvalé — retenční kaskáda je nemaže.

use rusqlite::{params, Connection};

/// Jedna událost pro UI (markery na časové ose).
#[derive(Debug, Clone)]
pub struct EventRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub pid: Option<u32>,
    pub detail: Option<String>,
}

/// Jeden incident pro UI (seznam + detail).
#[derive(Debug, Clone)]
pub struct IncidentRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub identity_key: Option<String>,
    pub culprit: Option<String>,
    pub detail: Option<String>,
    pub window_from: Option<i64>,
    pub window_to: Option<i64>,
}

/// Zapíše událost; vrací id řádku.
pub fn insert_event(
    conn: &Connection,
    ts: i64,
    kind: &str,
    pid: Option<u32>,
    detail: Option<&str>,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO event (ts, kind, pid, detail) VALUES (?1, ?2, ?3, ?4)",
        params![ts, kind, pid.map(|p| p as i64), detail],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Zapíše incident; vrací id řádku.
#[allow(clippy::too_many_arguments)]
pub fn insert_incident(
    conn: &Connection,
    ts: i64,
    kind: &str,
    identity_key: Option<&str>,
    culprit: Option<&str>,
    detail: Option<&str>,
    etl_path: Option<&str>,
    window_from: Option<i64>,
    window_to: Option<i64>,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO incident
             (ts, kind, identity_key, culprit, detail, etl_path, window_from, window_to)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ts,
            kind,
            identity_key,
            culprit,
            detail,
            etl_path,
            window_from,
            window_to
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Události v rozsahu [from, to] — pro markery na časové ose.
pub fn events_in(conn: &Connection, from: i64, to: i64) -> Result<Vec<EventRow>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, ts, kind, pid, detail FROM event
         WHERE ts BETWEEN ?1 AND ?2 ORDER BY ts",
    )?;
    let rows = stmt.query_map(params![from, to], |r| {
        Ok(EventRow {
            id: r.get(0)?,
            ts: r.get(1)?,
            kind: r.get(2)?,
            pid: r.get::<_, Option<i64>>(3)?.map(|p| p as u32),
            detail: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// Poslední incidenty (nejnovější první).
pub fn recent_incidents(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<IncidentRow>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, ts, kind, identity_key, culprit, detail, window_from, window_to
         FROM incident ORDER BY ts DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(IncidentRow {
            id: r.get(0)?,
            ts: r.get(1)?,
            kind: r.get(2)?,
            identity_key: r.get(3)?,
            culprit: r.get(4)?,
            detail: r.get(5)?,
            window_from: r.get(6)?,
            window_to: r.get(7)?,
        })
    })?;
    rows.collect()
}

/// Smaže záznam incidentu (mažeme jen VLASTNÍ DB záznam — žádná
/// mutace systému; uživatel má právo vyčistit si seznam).
pub fn delete_incident(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM incident WHERE id = ?1", params![id])?;
    Ok(())
}

/// Existuje už incident daného druhu v čase ±tol? (dedup při startu —
/// BSOD scan nesmí založit tentýž incident po každém restartu služby.)
pub fn incident_exists(
    conn: &Connection,
    kind: &str,
    ts: i64,
    tol_s: i64,
) -> Result<bool, rusqlite::Error> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM incident
         WHERE kind = ?1 AND ts BETWEEN ?2 - ?3 AND ?2 + ?3",
        params![kind, ts, tol_s],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}
