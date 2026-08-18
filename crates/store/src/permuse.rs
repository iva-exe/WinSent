//! Historie použití oprávnění (v9D, SPEC kap. 13.4).
//!
//! ConsentStore si pamatuje jen POSLEDNÍ použití: jakmile aplikace sáhne
//! na mikrofon podruhé, předchozí záznam se přepíše. Věta *„Discord
//! používal mikrofon včera 3 h 12 min"* proto nejde přečíst z registru —
//! musí si sezení zapisovat služba sama, jak je vidí přicházet.
//!
//! Zápis je záměrně idempotentní přes klíč (aplikace, schopnost, začátek):
//! sledování registru dorazí i několikrát během jednoho sezení a pokaždé
//! nese tentýž začátek, jen jiný konec.

use rusqlite::{params, Connection};

use crate::Error;

/// Jedno sezení: kdy aplikace schopnost vzala a kdy pustila.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermUse {
    pub app: String,
    pub capability: String,
    pub start_ts: i64,
    /// `None` = drží ji právě teď.
    pub stop_ts: Option<i64>,
}

/// Zaznamená (nebo doplní) jedno sezení.
pub fn record(
    conn: &Connection,
    app: &str,
    capability: &str,
    start_ts: i64,
    stop_ts: Option<i64>,
) -> Result<(), Error> {
    // Bez začátku není co zapsat — nulový čas znamená „nikdy nepoužito".
    if start_ts <= 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO perm_use (app, capability, start_ts, stop_ts)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(app, capability, start_ts)
         -- Konec se jen doplňuje. Přepsat vyplněný konec zpět na NULL
         -- by ze zavřeného sezení udělalo věčně běžící.
         DO UPDATE SET stop_ts = COALESCE(excluded.stop_ts, perm_use.stop_ts)",
        params![app, capability, start_ts, stop_ts],
    )?;
    Ok(())
}

/// Sezení jedné aplikace se schopností, nejnovější první.
pub fn history(
    conn: &Connection,
    app: &str,
    capability: &str,
    limit: u32,
) -> Result<Vec<PermUse>, Error> {
    let mut st = conn.prepare(
        "SELECT start_ts, stop_ts FROM perm_use
         WHERE app = ?1 AND capability = ?2
         ORDER BY start_ts DESC LIMIT ?3",
    )?;
    let rows = st.query_map(params![app, capability, limit], |r| {
        Ok(PermUse {
            app: app.to_string(),
            capability: capability.to_string(),
            start_ts: r.get(0)?,
            stop_ts: r.get(1)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

/// Kolik sekund celkem aplikace schopnost držela od `from` (unix).
/// Otevřené sezení se počítá do `now`.
pub fn total_seconds(
    conn: &Connection,
    app: &str,
    capability: &str,
    from: i64,
    now: i64,
) -> Result<i64, Error> {
    let mut st = conn.prepare(
        "SELECT COALESCE(SUM(MIN(COALESCE(stop_ts, ?4), ?4) - MAX(start_ts, ?3)), 0)
         FROM perm_use
         WHERE app = ?1 AND capability = ?2 AND COALESCE(stop_ts, ?4) > ?3",
    )?;
    let secs: i64 = st.query_row(params![app, capability, from, now], |r| r.get(0))?;
    Ok(secs.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().expect("db");
        crate::migrations::run(&c).expect("migrace");
        c
    }

    // Totéž sezení dorazí z registru několikrát — podruhé už jen
    // s koncem. Nesmí z toho vzniknout dva řádky.
    #[test]
    fn repeated_reads_of_one_session_stay_one_row() {
        let c = db();
        record(&c, "app", "microphone", 1000, None).expect("zápis");
        record(&c, "app", "microphone", 1000, None).expect("zápis");
        record(&c, "app", "microphone", 1000, Some(1600)).expect("zápis");
        let h = history(&c, "app", "microphone", 10).expect("historie");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].stop_ts, Some(1600));
    }

    // Doplněný konec se nesmí ztratit, když pak dorazí čtení bez něj.
    #[test]
    fn closed_session_never_reopens() {
        let c = db();
        record(&c, "app", "webcam", 500, Some(900)).expect("zápis");
        record(&c, "app", "webcam", 500, None).expect("zápis");
        let h = history(&c, "app", "webcam", 10).expect("historie");
        assert_eq!(h[0].stop_ts, Some(900));
    }

    // Součet za období: sezení mimo okno se nepočítá, přesahující se
    // ořízne, běžící se počítá do „teď".
    #[test]
    fn total_counts_only_the_window() {
        let c = db();
        record(&c, "a", "microphone", 100, Some(200)).expect("staré"); // mimo
        record(&c, "a", "microphone", 900, Some(1100)).expect("přesah"); // 100 s v okně
        record(&c, "a", "microphone", 1200, None).expect("běží"); // 300 s do teď
        let total = total_seconds(&c, "a", "microphone", 1000, 1500).expect("součet");
        assert_eq!(total, 400);
    }

    // Nikdy nepoužité oprávnění nemá co zapisovat.
    #[test]
    fn never_used_writes_nothing() {
        let c = db();
        record(&c, "a", "location", 0, None).expect("zápis");
        assert!(history(&c, "a", "location", 10).expect("historie").is_empty());
    }
}
