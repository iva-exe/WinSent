//! Audit mutací (v5, SPEC 17.6) — součást bezpečnostního modelu, ne
//! jen log. Trvalý (retence nemaže); `reversible` drží konkrétní
//! cestu zpět pro budoucí „vrátit poslední akci".

use core_types::action::AuditRow;
use rusqlite::{params, Connection};

/// Zapíše auditní záznam; vrací id.
#[allow(clippy::too_many_arguments)]
pub fn insert(
    conn: &Connection,
    ts: i64,
    action: &str,
    target: &str,
    class: &str,
    verdict: &str,
    deny_reason: Option<&str>,
    outcome: Option<&str>,
    reversible: Option<&str>,
    detail: Option<&str>,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO audit (ts, action, target, class, verdict, deny_reason,
                            outcome, reversible, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            ts,
            action,
            target,
            class,
            verdict,
            deny_reason,
            outcome,
            reversible,
            detail
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Doplní výsledek k existujícímu záznamu. Používá se tam, kde akci
/// dokončí až uživatel mimo službu (odinstalátor v jeho relaci) —
/// verdikt se nikdy nepřepisuje, jen `outcome`/`detail`.
pub fn set_outcome(
    conn: &Connection,
    id: i64,
    outcome: &str,
    detail: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE audit SET outcome = ?2, detail = ?3 WHERE id = ?1",
        params![id, outcome, detail],
    )?;
    Ok(())
}

/// Doplní cestu zpět k existujícímu záznamu. Mazání do koše ji zná až
/// po přesunu: kolik položek doopravdy skončilo v koši (zbytek mohl
/// uživatel na dotaz Windows smazat natrvalo, nebo přesun zrušit).
pub fn set_reversible(
    conn: &Connection,
    id: i64,
    reversible: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE audit SET reversible = ?2 WHERE id = ?1",
        params![id, reversible],
    )?;
    Ok(())
}

/// Poslední auditní záznamy (nejnovější první).
pub fn recent(conn: &Connection, limit: u32) -> Result<Vec<AuditRow>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, ts, action, target, class, verdict, deny_reason, outcome, reversible
         FROM audit ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(AuditRow {
            id: r.get(0)?,
            ts: r.get(1)?,
            action: r.get(2)?,
            target: r.get(3)?,
            class: r.get(4)?,
            verdict: r.get(5)?,
            deny_reason: r.get(6)?,
            outcome: r.get(7)?,
            reversible: r.get(8)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deny i allow nechávají stopu; recent čte nejnovější první.
    #[test]
    fn audit_records_both_verdicts() {
        let conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        insert(
            &conn,
            100,
            "test_op",
            "fake:x",
            "T1",
            "deny",
            Some("cíl neexistuje"),
            None,
            None,
            None,
        )
        .unwrap();
        insert(
            &conn,
            101,
            "test_toggle",
            "test:a=true",
            "T0",
            "allow",
            None,
            Some("ok"),
            Some("přepnout zpět"),
            None,
        )
        .unwrap();
        let rows = recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].verdict, "allow");
        assert_eq!(rows[1].verdict, "deny");
        assert_eq!(rows[1].deny_reason.as_deref(), Some("cíl neexistuje"));
    }

    // Cesta zpět se doplní až po výsledku — a jen k danému záznamu.
    #[test]
    fn reversible_se_doplni_pozdeji() {
        let conn = Connection::open_in_memory().unwrap();
        crate::migrations::run(&conn).unwrap();
        let mazani = |ts| {
            insert(
                &conn,
                ts,
                "delete_files",
                "a.txt",
                "T2",
                "allow",
                None,
                Some("running"),
                None,
                None,
            )
            .unwrap()
        };
        let a = mazani(200);
        let b = mazani(201);
        set_outcome(&conn, a, "ok", "hotovo").unwrap();
        set_reversible(&conn, a, Some("obnovit z koše (1 položek)")).unwrap();
        set_outcome(&conn, b, "failed", "zrušeno").unwrap();
        let rows = recent(&conn, 10).unwrap();
        assert_eq!(rows[1].id, a);
        assert_eq!(rows[1].reversible.as_deref(), Some("obnovit z koše (1 položek)"));
        assert_eq!(rows[0].reversible, None, "zrušené mazání nic neslibuje");
    }
}
