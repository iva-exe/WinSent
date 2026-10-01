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
    // → verze 6 (v3): retenční kaskáda naostro (SPEC kap. 8) + event a
    // incident tabulky (SPEC kap. 16.4). Agregáty nesou avg i max —
    // špička nesmí zmizet průměrováním. ts agregátu = začátek bucketu.
    // Odchylka od SPEC: incident.app_id → identity_key (tabulka app
    // vznikne až s inventářem ve v4, pak se dá dopropojit).
    "CREATE TABLE system_10s (
        ts INTEGER PRIMARY KEY,
        cpu_pct REAL, cpu_pct_max REAL,
        mem_used_mb INTEGER,
        net_rx_bps INTEGER, net_tx_bps INTEGER,
        gpu_pct REAL, gpu_pct_max REAL,
        gpu_temp_c REAL, cpu_clock_mhz INTEGER
    ) WITHOUT ROWID;
    CREATE TABLE system_1m (
        ts INTEGER PRIMARY KEY,
        cpu_pct REAL, cpu_pct_max REAL,
        mem_used_mb INTEGER,
        net_rx_bps INTEGER, net_tx_bps INTEGER,
        gpu_pct REAL, gpu_pct_max REAL,
        gpu_temp_c REAL, cpu_clock_mhz INTEGER
    ) WITHOUT ROWID;
    CREATE TABLE sample_10s (
        ts INTEGER NOT NULL, proc_id INTEGER NOT NULL,
        cpu_pm INTEGER, cpu_pm_max INTEGER,
        ws_kb INTEGER, io_r INTEGER, io_w INTEGER,
        PRIMARY KEY (ts, proc_id)
    ) WITHOUT ROWID;
    CREATE TABLE sample_1m (
        ts INTEGER NOT NULL, proc_id INTEGER NOT NULL,
        cpu_pm INTEGER, cpu_pm_max INTEGER,
        ws_kb INTEGER, io_r INTEGER, io_w INTEGER,
        PRIMARY KEY (ts, proc_id)
    ) WITHOUT ROWID;
    CREATE TABLE disk_10s (
        ts INTEGER NOT NULL, disk INTEGER NOT NULL,
        r_bps INTEGER, w_bps INTEGER,
        PRIMARY KEY (ts, disk)
    ) WITHOUT ROWID;
    CREATE TABLE disk_1m (
        ts INTEGER NOT NULL, disk INTEGER NOT NULL,
        r_bps INTEGER, w_bps INTEGER,
        PRIMARY KEY (ts, disk)
    ) WITHOUT ROWID;
    CREATE TABLE event (
        id     INTEGER PRIMARY KEY,
        ts     INTEGER NOT NULL,
        kind   TEXT NOT NULL,
        pid    INTEGER,
        detail TEXT
    );
    CREATE INDEX ix_event_ts ON event(ts DESC);
    CREATE TABLE incident (
        id           INTEGER PRIMARY KEY,
        ts           INTEGER NOT NULL,
        kind         TEXT NOT NULL,
        identity_key TEXT,
        culprit      TEXT,
        detail       TEXT,
        etl_path     TEXT,
        window_from  INTEGER,
        window_to    INTEGER
    );
    CREATE INDEX ix_incident_ts ON incident(ts DESC);",
    // → verze 7 (v4): inventář aplikací + mapa souborů (SPEC kap. 5, 8).
    // identity_key spojuje inventář s procesy (kaskáda v2) a ikonami.
    "CREATE TABLE app (
        id            INTEGER PRIMARY KEY,
        identity_key  TEXT NOT NULL UNIQUE,
        kind          TEXT NOT NULL,
        display_name  TEXT NOT NULL,
        publisher     TEXT,
        version       TEXT,
        install_date  INTEGER,
        icon_blob     BLOB,
        first_seen    INTEGER NOT NULL,
        last_seen     INTEGER NOT NULL
    );
    CREATE TABLE app_path (
        app_id      INTEGER NOT NULL REFERENCES app(id) ON DELETE CASCADE,
        path        TEXT NOT NULL,
        role        TEXT NOT NULL,
        source      TEXT NOT NULL,
        confidence  TEXT NOT NULL,
        size_bytes  INTEGER,
        size_ts     INTEGER,
        PRIMARY KEY (app_id, path)
    );",
    // → verze 8 (v5): audit mutací (SPEC 17.6) — každá akce, schválená
    // i zamítnutá, nechává trvalou stopu; `reversible` drží cestu zpět.
    "CREATE TABLE audit (
        id          INTEGER PRIMARY KEY,
        ts          INTEGER NOT NULL,
        action      TEXT NOT NULL,
        target      TEXT NOT NULL,
        class       TEXT NOT NULL,
        verdict     TEXT NOT NULL,
        deny_reason TEXT,
        outcome     TEXT,
        reversible  TEXT,
        detail      TEXT
    );
    CREATE INDEX ix_audit_ts ON audit(ts DESC);",
    // Historie použití oprávnění (v9D). ConsentStore si pamatuje jen
    // POSLEDNÍ použití — jakmile aplikace sáhne na mikrofon podruhé,
    // ten předchozí záznam přepíše. Aby šlo říct „Discord používal
    // mikrofon včera 3 h 12 min", musí si sezení zapisovat služba sama.
    // Klíč přes (app, capability, start) dělá zápis idempotentní: totéž
    // sezení se při opakovaném čtení jen aktualizuje, nepřidá.
    "CREATE TABLE perm_use (
        app        TEXT NOT NULL,
        capability TEXT NOT NULL,
        start_ts   INTEGER NOT NULL,
        stop_ts    INTEGER,
        PRIMARY KEY (app, capability, start_ts)
    ) WITHOUT ROWID;
    CREATE INDEX ix_perm_use_ts ON perm_use(start_ts DESC);",
    // → GPU v historii procesů. Sloupec chyběl od začátku: živý
    // list GPU ukazoval, ale do vzorků se nikdy nezapisovalo, takže
    // náhled minulosti měl u každého procesu prázdno. Per mille
    // stejně jako cpu_pm — 0,1 % je jemnější, než jaká je vůbec
    // přesnost čítače.
    "ALTER TABLE sample_1s  ADD COLUMN gpu_pm INTEGER;
    ALTER TABLE sample_10s ADD COLUMN gpu_pm INTEGER;
    ALTER TABLE sample_10s ADD COLUMN gpu_pm_max INTEGER;
    ALTER TABLE sample_1m  ADD COLUMN gpu_pm INTEGER;
    ALTER TABLE sample_1m  ADD COLUMN gpu_pm_max INTEGER;",
    // → kdy naposledy jsme relaci VIDĚLI otevřenou.
    //
    // Bez toho se otevřená relace počítala až do teď — a relace, která
    // nikdy neskončí (aplikace spadla, Windows konec nedopsaly), pak
    // hlásila 720 hodin mikrofonu za třicetidenní okno. Konec se bere
    // z posledního pozorování, ne z aktuálního času.
    "ALTER TABLE perm_use ADD COLUMN seen_ts INTEGER;",
    // → jména procesů po INSTANCÍCH, ne po PID.
    //
    // proc_names držela pro každý pid jediný řádek s posledním jménem.
    // Windows PID recyklují (po restartu všechny), takže náhled minulosti
    // připisoval týdny staré vzorky — CPU, paměť, ikonu i vydavatele —
    // procesu, který ten PID nesl až dnes. Instance = (pid, create_time);
    // first_ts je unixový čas jejího vzniku a dotaz bere poslední
    // instanci, která začala nejpozději v čase vzorku.
    //
    // Staré řádky přejdou s first_ts = 0: kdy jejich proces vznikl, se
    // nikdy neukládalo, takže starší historii opravit nejde. proc_names
    // zůstává — starší verze služby (návrat k předchozímu vydání) do ní
    // zapisuje a bez ní by jí padal každý zápis vzorků.
    //
    // Index na last_ts kvůli retenci: maže se po ní každou minutu a bez
    // indexu by to byl průchod celou tabulkou (řádek za každé spuštění
    // procesu po celý rok) uvnitř zápisové transakce, tedy zdržení
    // zápisu vzorků.
    "CREATE TABLE proc_names_v (
        pid          INTEGER NOT NULL,
        first_ts     INTEGER NOT NULL,
        create_time  INTEGER,
        name         TEXT NOT NULL,
        last_ts      INTEGER NOT NULL,
        identity_key TEXT,
        app_name     TEXT,
        publisher    TEXT,
        PRIMARY KEY (pid, first_ts)
    ) WITHOUT ROWID;
    CREATE INDEX proc_names_v_last ON proc_names_v(last_ts);
    INSERT INTO proc_names_v
        (pid, first_ts, create_time, name, last_ts, identity_key, app_name, publisher)
    SELECT pid, 0, NULL, name, last_ts, identity_key, app_name, publisher
    FROM proc_names;",
    // → úklid falešných záseků ze spánku.
    //
    // Detektor záseků měřil čas hodinami, které běží i během uspání,
    // takže každé probuzení PC založilo „zásek" dlouhý jako celý spánek
    // (v reálné DB 21 h a 7,3 h) s viníkem podle toho, co běželo po
    // probuzení. Detektor je opravený; tyhle záznamy by ale v sekci
    // Incidents a v exportech strašily dál. Zásek přes deset minut se ve
    // skutečnosti nestane — tak dlouho zamrzlý systém skončí resetem.
    "DELETE FROM incident
        WHERE kind = 'stall' AND CAST(json_extract(detail, '$.lag_ms') AS INTEGER) > 600000;
    DELETE FROM event
        WHERE kind = 'stall' AND CAST(json_extract(detail, '$.lag_ms') AS INTEGER) > 600000;",
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

#[cfg(test)]
mod uklid_tests {
    use super::*;

    /// Falešné záseky ze spánku zmizí, skutečné zůstanou.
    #[test]
    fn migrace_uklidi_zaseky_ze_spanku() {
        let conn = Connection::open_in_memory().unwrap();
        // Stav před úklidem: všechno kromě poslední migrace.
        for sql in &MIGRATIONS[..MIGRATIONS.len() - 1] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch(
            r#"INSERT INTO incident (ts, kind, detail) VALUES
                 (1, 'stall', '{"lag_ms":75803755,"cause":"paging"}'),
                 (2, 'stall', '{"lag_ms":475}'),
                 (3, 'app_crash', '{"lag_ms":99999999}');
               INSERT INTO event (ts, kind, detail) VALUES
                 (1, 'stall', '{"lag_ms":26326048}'),
                 (2, 'stall', '{"lag_ms":1200}');"#,
        )
        .unwrap();
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as u32 - 1).unwrap();
        run(&conn).unwrap();
        let inc: Vec<i64> = conn
            .prepare("SELECT ts FROM incident ORDER BY ts")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(inc, vec![2, 3]);
        let ev: i64 = conn.query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0)).unwrap();
        assert_eq!(ev, 1);
    }
}
