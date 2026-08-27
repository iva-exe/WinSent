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
    // Kdy jsme relaci viděli — u otevřené je to strop pro počítání.
    seen_ts: i64,
) -> Result<(), Error> {
    // Bez začátku není co zapsat — nulový čas znamená „nikdy nepoužito".
    if start_ts <= 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO perm_use (app, capability, start_ts, stop_ts, seen_ts)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(app, capability, start_ts)
         -- Konec se jen doplňuje. Přepsat vyplněný konec zpět na NULL
         -- by ze zavřeného sezení udělalo věčně běžící.
         -- seen_ts se naopak posouvá pořád: je to poslední okamžik,
         -- kdy jsme relaci viděli otevřenou, a u sezení bez konce
         -- určuje, kam až se smí počítat.
         DO UPDATE SET stop_ts = COALESCE(excluded.stop_ts, perm_use.stop_ts),
                       seen_ts = MAX(COALESCE(perm_use.seen_ts, 0), excluded.seen_ts)",
        params![app, capability, start_ts, stop_ts, seen_ts],
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
        // Stejné pravidlo jako v totals(): sezení bez konce se počítá
        // jen po poslední pozorování, ne do teď.
        "SELECT COALESCE(SUM(MIN(COALESCE(stop_ts, seen_ts, start_ts), ?4) - MAX(start_ts, ?3)), 0)
         FROM perm_use
         WHERE app = ?1 AND capability = ?2
           AND COALESCE(stop_ts, seen_ts, start_ts) > ?3",
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
        record(&c, "app", "microphone", 1000, None, 1100).expect("zápis");
        record(&c, "app", "microphone", 1000, None, 1200).expect("zápis");
        record(&c, "app", "microphone", 1000, Some(1600), 1600).expect("zápis");
        let h = history(&c, "app", "microphone", 10).expect("historie");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].stop_ts, Some(1600));
    }

    // Probíhající relace musí do součtu růst.
    //
    // Windows během hovoru do ConsentStore nic nezapisují, takže se
    // `seen_ts` neposune samo — sledování ho proto potvrzuje jednou za
    // minutu. Bez toho vycházel hodinový hovor jako nula a na jednom
    // řádku stálo „Naposledy: právě teď" a vedle „Posledních 30 dnů:
    // nepoužito".
    #[test]
    fn probihajici_relace_roste_se_seen_ts() {
        let c = db();
        record(&c, "app", "microphone", 1000, None, 1000).expect("zápis");
        assert_eq!(
            total_seconds(&c, "app", "microphone", 0, 5000).expect("součet"),
            0,
            "hned po startu ještě není co počítat"
        );
        // Minutová potvrzení během hovoru.
        for t in [1060, 1120, 1180] {
            record(&c, "app", "microphone", 1000, None, t).expect("zápis");
        }
        assert_eq!(
            total_seconds(&c, "app", "microphone", 0, 5000).expect("součet"),
            180,
            "relace musí růst po poslední pozorování"
        );
    }

    // Relace, které konec nikdy nedopsal, se nesmí uzavřít na nulu.
    //
    // `last_used` z registru je maximum ze začátku a konce, takže po
    // výpadku napájení nebo BSODu se rovná začátku. Kdyby se poslal
    // jako `stop_ts`, přepsal by NULL a smazal celý naměřený čas.
    #[test]
    fn nedokoncena_relace_neprijde_o_cas() {
        let c = db();
        record(&c, "app", "microphone", 1000, None, 1000).expect("zápis");
        record(&c, "app", "microphone", 1000, None, 4600).expect("hodina hovoru");
        assert_eq!(
            total_seconds(&c, "app", "microphone", 0, 9000).expect("součet"),
            3600
        );
        // Tady daemon POZNÁ, že konec Windows nezapsaly, a stop_ts
        // neposílá — čas zůstává.
        record(&c, "app", "microphone", 1000, None, 4600).expect("po pádu");
        assert_eq!(
            total_seconds(&c, "app", "microphone", 0, 9000).expect("součet"),
            3600,
            "hodina se nesmí ztratit"
        );
    }

    // Doplněný konec se nesmí ztratit, když pak dorazí čtení bez něj.
    #[test]
    fn closed_session_never_reopens() {
        let c = db();
        record(&c, "app", "webcam", 500, Some(900), 900).expect("zápis");
        record(&c, "app", "webcam", 500, None, 950).expect("zápis");
        let h = history(&c, "app", "webcam", 10).expect("historie");
        assert_eq!(h[0].stop_ts, Some(900));
    }

    // Součet za období: sezení mimo okno se nepočítá, přesahující se
    // ořízne, běžící se počítá do „teď".
    #[test]
    fn total_counts_only_the_window() {
        let c = db();
        record(&c, "a", "microphone", 100, Some(200), 200).expect("staré"); // mimo
        record(&c, "a", "microphone", 900, Some(1100), 1100).expect("přesah"); // 100 s v okně
        // Běžící sezení: naposledy viděné v 1500, tedy 300 s v okně.
        record(&c, "a", "microphone", 1200, None, 1500).expect("běží");
        let total = total_seconds(&c, "a", "microphone", 1000, 1500).expect("součet");
        assert_eq!(total, 400);
    }

    // Nikdy nepoužité oprávnění nemá co zapisovat.
    #[test]
    fn never_used_writes_nothing() {
        let c = db();
        record(&c, "a", "location", 0, None, 100).expect("zápis");
        assert!(history(&c, "a", "location", 10).expect("historie").is_empty());
    }
}

/// Součty za období pro VŠECHNY dvojice (aplikace, schopnost).
///
/// Jeden dotaz místo jednoho na řádek: UI má u každého oprávnění
/// ukázat, kolik času ho aplikace držela, a sedmdesát samostatných
/// dotazů kvůli tomu je zbytečné.
pub fn totals(conn: &Connection, from: i64, now: i64) -> Result<Vec<(String, String, i64)>, Error> {
    let mut st = conn.prepare(
        // U sezení bez konce se počítá jen po poslední pozorování.
        // Kdyby se počítalo do teď, jedna relace, kterou Windows nikdy
        // nezavřely, by spolkla celé okno.
        "SELECT app, capability,
                SUM(MIN(COALESCE(stop_ts, seen_ts, start_ts), ?2) - MAX(start_ts, ?1))
         FROM perm_use
         WHERE COALESCE(stop_ts, seen_ts, start_ts) > ?1
         GROUP BY app, capability",
    )?;
    let rows = st.query_map(params![from, now], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2).unwrap_or(0).max(0),
        ))
    })?;
    Ok(rows.flatten().collect())
}
