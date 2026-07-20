//! Migrace schématu — lineární, číslované, idempotentní systém.
//!
//! Verze schématu se drží v SQLite `PRAGMA user_version`. Každá migrace
//! je SQL blok, který se aplikuje v transakci; po úspěchu se verze
//! zvýší. Migrace se nikdy nemění zpětně — jen se přidávají na konec.

use rusqlite::Connection;

/// Seznam migrací v pořadí aplikace. Index 0 = přechod na verzi 1.
/// v0 zakládá jen tabulku `meta`; datové tabulky (app, proc_instance,
/// sample_*…) přibudou s kolektory ve v1+.
const MIGRATIONS: &[&str] = &[
    // → verze 1: meta tabulka (klíč/hodnota) pro provozní údaje.
    "CREATE TABLE meta (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) WITHOUT ROWID;
    INSERT INTO meta (key, value) VALUES ('created_ts', strftime('%s','now'));",
    // → verze 2 (v1): vzorky sampleru dle SPEC kap. 8.
    // sample_1s.proc_id je zatím PID — proc_instance (stabilní identita
    // z ETW ProcessStart) přijde ve v3, pak přibude migrace s převodem.
    // system_1s má sloupce dle SPEC; v1 plní jen cpu_pct a mem, zbytek
    // NULL (senzory/disk přijdou ve v3/v9).
    "CREATE TABLE system_1s (
        ts INTEGER PRIMARY KEY,
        cpu_pct REAL, mem_used_mb INTEGER, commit_mb INTEGER,
        disk_qlen REAL, disk_lat_ms REAL, hard_flt_rate INTEGER,
        gpu_pct REAL, thermal_throttle INTEGER,
        cpu_temp_c REAL, cpu_temp_src TEXT,
        cpu_clock_mhz INTEGER, cpu_clock_max_mhz INTEGER
    ) WITHOUT ROWID;
    CREATE TABLE sample_1s (
        ts        INTEGER NOT NULL,
        proc_id   INTEGER NOT NULL,
        cpu_pm    INTEGER,
        ws_kb     INTEGER,
        priv_kb   INTEGER,
        io_r      INTEGER,
        io_w      INTEGER,
        hard_flt  INTEGER,
        PRIMARY KEY (ts, proc_id)
    ) WITHOUT ROWID;",
    // → verze 3: síť v system_1s + jména procesů pro čtení historie.
    // proc_names je dočasný můstek (pid → poslední známé jméno), než
    // ve v3 vznikne proc_instance se stabilní identitou z ETW.
    "ALTER TABLE system_1s ADD COLUMN net_rx_bps INTEGER;
    ALTER TABLE system_1s ADD COLUMN net_tx_bps INTEGER;
    CREATE TABLE proc_names (
        pid     INTEGER PRIMARY KEY,
        name    TEXT NOT NULL,
        last_ts INTEGER NOT NULL
    ) WITHOUT ROWID;",
    // → verze 4: historie detailů proměnných — jádra CPU, disky a GPU
    // senzory, aby zámek času ukázal i detail sekci z minulosti.
    "ALTER TABLE system_1s ADD COLUMN gpu_temp_c REAL;
    ALTER TABLE system_1s ADD COLUMN gpu_vram_mb INTEGER;
    ALTER TABLE system_1s ADD COLUMN gpu_power_w REAL;
    ALTER TABLE system_1s ADD COLUMN gpu_clock_mhz INTEGER;
    CREATE TABLE core_1s (
        ts   INTEGER NOT NULL,
        core INTEGER NOT NULL,
        pct  REAL,
        PRIMARY KEY (ts, core)
    ) WITHOUT ROWID;
    CREATE TABLE disk_1s (
        ts    INTEGER NOT NULL,
        disk  INTEGER NOT NULL,
        r_bps INTEGER,
        w_bps INTEGER,
        PRIMARY KEY (ts, disk)
    ) WITHOUT ROWID;
    CREATE TABLE disk_names (
        disk INTEGER PRIMARY KEY,
        name TEXT NOT NULL
    ) WITHOUT ROWID;",
    // → verze 5 (v2): identita aplikací i pro historii — bez ní se
    // list při náhledu minulosti seskupoval jinak než živý (jen podle
    // jména) a neměl ikony (klíčované identity_key).
    "ALTER TABLE proc_names ADD COLUMN identity_key TEXT;
    ALTER TABLE proc_names ADD COLUMN app_name TEXT;
    ALTER TABLE proc_names ADD COLUMN publisher TEXT;",
];

/// Aplikuje všechny dosud neaplikované migrace. Bezpečné volat při
/// každém startu — už aplikované se přeskočí podle `user_version`.
pub fn run(conn: &Connection) -> Result<(), rusqlite::Error> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let target = (idx + 1) as u32;
        if target <= current {
            continue;
        }
        tracing::info!(from = current, to = target, "aplikuji migraci schématu");
        conn.execute_batch(&format!(
            "BEGIN;\n{sql}\nPRAGMA user_version = {target};\nCOMMIT;"
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Migrace musí být idempotentní — druhý běh nesmí nic změnit ani selhat.
    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        run(&conn).unwrap();
        let v: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
    }
}
