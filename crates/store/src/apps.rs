//! Inventář aplikací v DB (v4, SPEC kap. 5, 8): zápis výsledku skenu
//! a čtení pro IPC. Velikosti cest jsou lazy — sken je neplní, doplňují
//! se on-demand a při dalším skenu se zachovávají.

use core_types::proc::{AppPathRow, AppRow};
use rusqlite::{params, Connection};

/// Vstup zápisu — jedna aplikace ze skenu (mirror collector-inv typů,
/// store na kolektorech nesmí záviset).
pub struct ScanApp {
    pub identity_key: String,
    pub kind: String,
    pub display_name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_ts: Option<i64>,
    pub paths: Vec<ScanPath>,
}

pub struct ScanPath {
    pub path: String,
    pub role: String,
    pub source: String,
    pub confidence: String,
}

/// Nahradí inventář výsledkem skenu: upsert aplikací i cest (velikosti
/// se zachovávají), aplikace a cesty mimo sken zmizí (odinstalované).
pub fn replace_inventory(conn: &mut Connection, apps: &[ScanApp]) -> Result<(), rusqlite::Error> {
    let now = now_unix();
    let tx = conn.transaction()?;
    {
        // Temp tabulky pro mark-and-sweep (bez JSON závislosti).
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS scan_paths (path TEXT PRIMARY KEY) ;
             CREATE TEMP TABLE IF NOT EXISTS scan_keys  (key  TEXT PRIMARY KEY) ;
             DELETE FROM scan_paths; DELETE FROM scan_keys;",
        )?;
        let mut upsert_app = tx.prepare_cached(
            "INSERT INTO app (identity_key, kind, display_name, publisher, version,
                              install_date, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(identity_key) DO UPDATE SET
                 kind = excluded.kind,
                 display_name = excluded.display_name,
                 publisher = excluded.publisher,
                 version = excluded.version,
                 install_date = excluded.install_date,
                 last_seen = excluded.last_seen",
        )?;
        let mut get_id = tx.prepare_cached("SELECT id FROM app WHERE identity_key = ?1")?;
        let mut upsert_path = tx.prepare_cached(
            "INSERT INTO app_path (app_id, path, role, source, confidence)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(app_id, path) DO UPDATE SET
                 role = excluded.role,
                 source = excluded.source,
                 confidence = excluded.confidence",
        )?;
        for app in apps {
            upsert_app.execute(params![
                app.identity_key,
                app.kind,
                app.display_name,
                app.publisher,
                app.version,
                app.install_ts,
                now,
            ])?;
            let app_id: i64 = get_id.query_row(params![app.identity_key], |r| r.get(0))?;
            tx.execute("DELETE FROM scan_paths", [])?;
            {
                let mut mark =
                    tx.prepare_cached("INSERT OR IGNORE INTO scan_paths (path) VALUES (?1)")?;
                for p in &app.paths {
                    upsert_path.execute(params![app_id, p.path, p.role, p.source, p.confidence])?;
                    mark.execute(params![p.path])?;
                }
            }
            // Cesty, které tento sken už nenašel.
            tx.execute(
                "DELETE FROM app_path WHERE app_id = ?1
                 AND path NOT IN (SELECT path FROM scan_paths)",
                params![app_id],
            )?;
        }
        // Aplikace, které sken nenašel (odinstalované) — pryč i s cestami.
        {
            let mut mark =
                tx.prepare_cached("INSERT OR IGNORE INTO scan_keys (key) VALUES (?1)")?;
            for app in apps {
                mark.execute(params![app.identity_key])?;
            }
        }
        tx.execute(
            "DELETE FROM app WHERE identity_key NOT IN (SELECT key FROM scan_keys)",
            [],
        )?;
    }
    tx.commit()
}

/// Seznam aplikací pro UI.
pub fn list_apps(conn: &Connection) -> Result<Vec<AppRow>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT a.identity_key, a.kind, a.display_name, a.publisher, a.version,
                a.install_date,
                (SELECT COUNT(*) FROM app_path p WHERE p.app_id = a.id),
                -- Instalační cesty aplikace (pro kontrolu, že ještě
                -- existují na disku — viz missing_install níž).
                (SELECT GROUP_CONCAT(p.path, '|') FROM app_path p
                  WHERE p.app_id = a.id AND p.role = 'install')
         FROM app a ORDER BY LOWER(a.display_name)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AppRow {
            identity_key: r.get(0)?,
            kind: r.get(1)?,
            display_name: r.get(2)?,
            publisher: r.get(3)?,
            version: r.get(4)?,
            install_ts: r.get(5)?,
            path_count: r.get::<_, i64>(6)?.max(0) as u32,
            // Instalační adresář v registru je, ale na disku není —
            // typicky ručně smazaná hra, po které zbyl jen záznam.
            // Kontroluje se jen existence (levné), ne obsah.
            missing_install: match r.get::<_, Option<String>>(7)? {
                Some(paths) if !paths.is_empty() => paths
                    .split('|')
                    .all(|p| !p.trim().is_empty() && !std::path::Path::new(p.trim()).exists()),
                _ => false,
            },
            // Doplní služba — store do registru nesahá.
            uninstaller_missing: false,
        })
    })?;
    rows.collect()
}

/// Mapa souborů aplikace.
pub fn app_map(conn: &Connection, identity_key: &str) -> Result<Vec<AppPathRow>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.path, p.role, p.source, p.confidence, p.size_bytes, p.size_ts
         FROM app_path p JOIN app a ON a.id = p.app_id
         WHERE a.identity_key = ?1
         ORDER BY CASE p.role
             WHEN 'install' THEN 0 WHEN 'data' THEN 1 WHEN 'config' THEN 2
             WHEN 'cache' THEN 3 WHEN 'logs' THEN 4 ELSE 5 END, p.path",
    )?;
    let rows = stmt.query_map(params![identity_key], |r| {
        Ok(AppPathRow {
            path: r.get(0)?,
            role: r.get(1)?,
            source: r.get(2)?,
            confidence: r.get(3)?,
            size_bytes: r.get::<_, Option<i64>>(4)?.map(|v| v.max(0) as u64),
            size_ts: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// Uloží spočtenou velikost cesty (cache pro příště).
pub fn set_path_size(
    conn: &Connection,
    identity_key: &str,
    path: &str,
    size_bytes: u64,
    ts: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE app_path SET size_bytes = ?3, size_ts = ?4
         WHERE path = ?2
           AND app_id = (SELECT id FROM app WHERE identity_key = ?1)",
        params![identity_key, path, size_bytes as i64, ts],
    )?;
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Upsert zachová velikost cesty přes rescan a smaže zaniklé.
    #[test]
    fn rescan_preserves_sizes_and_prunes() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        let app = |paths: Vec<&str>| ScanApp {
            identity_key: "app:test".into(),
            kind: "desktop".into(),
            display_name: "Test".into(),
            publisher: None,
            version: Some("1.0".into()),
            install_ts: None,
            paths: paths
                .into_iter()
                .map(|p| ScanPath {
                    path: p.into(),
                    role: "install".into(),
                    source: "registry".into(),
                    confidence: "high".into(),
                })
                .collect(),
        };
        replace_inventory(&mut conn, &[app(vec!["C:\\A", "C:\\B"])]).unwrap();
        set_path_size(&conn, "app:test", "C:\\A", 12345, 1).unwrap();
        replace_inventory(&mut conn, &[app(vec!["C:\\A"])]).unwrap();

        let map = app_map(&conn, "app:test").unwrap();
        assert_eq!(map.len(), 1, "C:\\B měla zmizet");
        assert_eq!(map[0].size_bytes, Some(12345), "velikost přežije rescan");

        replace_inventory(&mut conn, &[]).unwrap();
        assert!(list_apps(&conn).unwrap().is_empty(), "odinstalace maže");
    }
}
