//! Retenční smyčka (SPEC kap. 8) — běží v zapisovacím vlákně služby
//! na BELOW_NORMAL prioritě.
//!
//! v1: prosté mazání surových vzorků starších než 1 hodina, aby DB
//! nerostla. Plná kaskáda (agregace 1s → 10s → 1m před smazáním)
//! přijde ve v3 s historií — mazat se pak bude až PO agregaci.

use rusqlite::{params, Connection};

/// Retence surových vzorků: 1 hodina (SPEC kap. 8, kaskáda v3).
const SAMPLE_1S_KEEP_S: i64 = 3600;

/// Jeden krok retence.
pub fn tick(conn: &Connection) -> Result<(), rusqlite::Error> {
    let cutoff = chrono_now_unix() - SAMPLE_1S_KEEP_S;
    let a = conn.execute("DELETE FROM sample_1s WHERE ts < ?1", params![cutoff])?;
    let b = conn.execute("DELETE FROM system_1s WHERE ts < ?1", params![cutoff])?;
    let c = conn.execute("DELETE FROM core_1s WHERE ts < ?1", params![cutoff])?;
    let d = conn.execute("DELETE FROM disk_1s WHERE ts < ?1", params![cutoff])?;
    if a + b + c + d > 0 {
        tracing::debug!(
            sample_1s = a,
            system_1s = b,
            core_1s = c,
            disk_1s = d,
            "retence smazala staré vzorky"
        );
    }
    Ok(())
}

/// Unix čas bez závislosti na chrono crate.
fn chrono_now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
