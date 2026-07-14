//! Retenční smyčka (SPEC kap. 8) — běží v samostatném vlákně na
//! BELOW_NORMAL prioritě. v0 nemá datové tabulky, takže krok nemá co
//! mazat — smyčka ale běží, aby infrastruktura existovala a měřila se
//! od začátku.

use rusqlite::Connection;

/// Jeden krok retence. Ve v1+ sem přijde kaskáda
/// `sample_1s → sample_10s → sample_1m` dle SPEC kap. 8.
pub fn tick(_conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::debug!("retenční krok: zatím není co mazat (v0)");
    Ok(())
}
