//! validate — validační vrstva, srdce bezpečnosti (SPEC kap. 17).
//!
//! Jediná brána mezi UI a mutacemi systému. Naplní se ve v5; v0 drží
//! jen tvar rozhraní a princip deny-by-default. Vrstva je čistě
//! on-demand — žádné vlákno na pozadí, v klidu 0 % CPU (SPEC kap. 20).

/// Mutující akce ke schválení. Varianty přibudou ve v5 (kill, delete,
/// toggle…). v0 nemá žádnou — enum bez variant nejde zkonstruovat,
/// takže nejde ani nic schválit.
#[derive(Debug)]
pub enum Action {}

/// Živý stav OS načtený v okamžiku validace (SPEC kap. 17.3).
/// Validátor nikdy nevěří snapshotu z UI. v0: prázdný nosič.
#[derive(Debug, Default)]
pub struct LiveContext {}

/// Verdikt validace. Žádný exekutor se nesmí spustit bez `Allow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny { reason: String },
}

/// Jediný vstupní bod vrstvy (SPEC kap. 17.1). Neexistuje druhá cesta,
/// jak akci schválit. Když si nejsme jistí, akci zamítneme.
pub fn validate(action: &Action, _ctx: &LiveContext) -> Verdict {
    // v0: Action nemá varianty, tohle je nedosažitelné — ale kdyby
    // varianta přibyla bez validační logiky, odpověď je Deny.
    match *action {}
}
