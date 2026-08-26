//! Diagnostika oprávnění: co přesně vrací `consent::consents()`.
//!
//! `cargo run -p win-sys --example consentprobe`
//!
//! Hodí se při ladění „používá právě teď": ukáže, kolik záznamů má
//! vůbec čas použití (když skoro žádný, je špatně cesta ke klíči)
//! a kolik jich platí za živá.
fn main() {
    let all = win_sys::consent::consents();
    let with_time = all.iter().filter(|c| c.last_start.is_some()).count();
    println!("záznamů: {}", all.len());
    println!("s časem použití: {with_time}");
    println!("používá právě teď: {}", all.iter().filter(|c| c.in_use).count());
    for c in all.iter().filter(|c| c.in_use) {
        println!("  ŽIVÉ  {:<12} od {:?}  {}", c.capability, c.last_start, c.app);
    }
    let filter: Vec<String> = std::env::args().skip(1).map(|a| a.to_lowercase()).collect();
    for c in all.iter().filter(|c| {
        !filter.is_empty() && filter.iter().any(|f| c.app.to_lowercase().contains(f))
    }) {
        println!(
            "  {:<12} in_use={:<5} start={:?} used={:?}  {}",
            c.capability, c.in_use, c.last_start, c.last_used, c.app
        );
    }
}
