//! Brána: startovací položky, které patří Windows, se nepřepínají.
//! `cargo run -p ipc --example onstartcheck`
//!
//! Ověřuje tři věci naráz:
//!   1. každá položka označená `system` má `toggleable == false`
//!      (jinak by UI nabídlo přepínač, který skončí odmítnutím),
//!   2. pokus o přepnutí systémové položky vrstva ODMÍTNE — a to
//!      i pro položku, kterou UI nikdy neposílá, protože do pipe může
//!      psát kterýkoli přihlášený uživatel,
//!   3. cesta úlohy bez úvodního lomítka (`Microsoft\Windows\…`) je
//!      odmítnutá taky. Plánovač takový tvar bere, takže by se jím dalo
//!      prefixové pravidlo obejít jedním smazaným znakem.
//!
//! Odmítnutí NIC nemění — vrstva vrací verdikt dřív, než se sáhne na
//! systém, takže brána je bezpečná i na produkčním stroji.

use core_types::action::Action;

fn main() {
    let items = match ipc::client::query_startup() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("query_startup selhal: {e}");
            std::process::exit(1);
        }
    };

    let sys: Vec<_> = items.iter().filter(|i| i.system).collect();
    let third: Vec<_> = items.iter().filter(|i| !i.system).collect();
    println!(
        "položek {} — systémových {}, třetích stran {}",
        items.len(),
        sys.len(),
        third.len()
    );

    let mut fails = 0;

    // 1. Systémová položka nesmí být přepínatelná.
    for i in sys.iter().filter(|i| i.toggleable) {
        println!("CHYBA: systémová položka je přepínatelná: {}", i.id);
        fails += 1;
    }

    // Rozpad podle zdroje, ať je vidět, co se schovává.
    let mut by_source = std::collections::BTreeMap::new();
    for i in &sys {
        *by_source.entry(i.source.clone()).or_insert(0u32) += 1;
    }
    println!("  skryto po zdrojích: {by_source:?}");
    for i in sys.iter().take(3) {
        println!("  {} [{}] — {:?}", i.name, i.source, i.system_reason);
    }

    // 2. Přepnutí systémové položky musí vrstva odmítnout.
    if let Some(t) = sys.first() {
        match ipc::client::toggle_action(Action::StartupToggle {
            id: t.id.clone(),
            on: false,
        }) {
            Ok(r) if r.verdict == "deny" => {
                println!("  deny OK: {} → {:?}", t.id, r.deny_reason);
            }
            Ok(r) => {
                println!("CHYBA: {} nebylo odmítnuto ({})", t.id, r.verdict);
                fails += 1;
            }
            Err(e) => {
                println!("CHYBA: toggle selhal jinak než odmítnutím: {e}");
                fails += 1;
            }
        }
    }

    // 3. Úloha bez úvodního lomítka — obcházení prefixového pravidla.
    for id in [
        r"task|Microsoft\Windows\UpdateOrchestrator\Reboot",
        r"task|\Microsoft\Windows\UpdateOrchestrator\Reboot",
        r"task|MICROSOFT\WINDOWS\Defrag\ScheduledDefrag",
    ] {
        match ipc::client::toggle_action(Action::StartupToggle {
            id: id.to_string(),
            on: false,
        }) {
            Ok(r) if r.verdict == "deny" => println!("  deny OK: {id}"),
            Ok(r) => {
                println!("CHYBA: {id} nebylo odmítnuto ({})", r.verdict);
                fails += 1;
            }
            Err(e) => {
                println!("CHYBA: {id} skončilo chybou místo odmítnutí: {e}");
                fails += 1;
            }
        }
    }

    println!(
        "\nBRÁNA onstartcheck: {}",
        if fails == 0 { "PASS" } else { "FAIL" }
    );
    if fails > 0 {
        std::process::exit(1);
    }
}
