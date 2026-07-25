//! Brána v6: `cargo run -p ipc --example startupcheck`.
//! Čtení položek + přepnutí 1 položky tam a zpět přes validační
//! vrstvu, s ověřením, že se stav opravdu změnil.

use core_types::action::Action;

fn main() {
    let items = match ipc::client::query_startup() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("query_startup selhal: {e}");
            std::process::exit(1);
        }
    };
    let mut by_source = std::collections::BTreeMap::new();
    for i in &items {
        *by_source.entry(i.source.clone()).or_insert(0u32) += 1;
    }
    println!("startup položek: {} {:?}", items.len(), by_source);
    for i in items.iter().filter(|i| i.identity_key.is_some()).take(3) {
        println!("  {} [{}] → aplikace {:?}", i.name, i.source, i.app_name);
    }

    let mut fails = 0;
    // Přepnout uživatelskou Run položku (nejbezpečnější kategorie).
    let Some(target) = items
        .iter()
        .find(|i| i.source == "run_user" && i.toggleable && i.enabled)
    else {
        println!("(žádná zapnutá HKCU Run položka — přepínací test přeskočen)");
        return;
    };
    println!("\ntest přepnutí: {} ({})", target.name, target.id);

    let t = std::time::Instant::now();
    let off = ipc::client::toggle_action(Action::StartupToggle {
        id: target.id.clone(),
        on: false,
    })
    .expect("toggle off");
    println!(
        "  vypnout: {} / {:?} / {} ms (klient {} ms)",
        off.verdict,
        off.outcome,
        off.duration_ms,
        t.elapsed().as_millis()
    );
    if off.verdict != "allow" || off.outcome.as_deref() != Some("ok") {
        fails += 1;
    }
    // Ověřit čerstvým čtením ze systému.
    let after = ipc::client::query_startup().expect("re-read");
    let now_off = after
        .iter()
        .find(|i| i.id == target.id)
        .map(|i| !i.enabled)
        .unwrap_or(false);
    println!(
        "  stav po vypnutí: {}",
        if now_off {
            "vypnuto ✓"
        } else {
            "NEZMĚNĚNO ✗"
        }
    );
    if !now_off {
        fails += 1;
    }

    let on = ipc::client::toggle_action(Action::StartupToggle {
        id: target.id.clone(),
        on: true,
    })
    .expect("toggle on");
    println!("  zpět zapnout: {} / {:?}", on.verdict, on.outcome);
    let back = ipc::client::query_startup().expect("re-read 2");
    let restored = back
        .iter()
        .find(|i| i.id == target.id)
        .map(|i| i.enabled)
        .unwrap_or(false);
    println!(
        "  stav po vrácení: {}",
        if restored {
            "zapnuto ✓"
        } else {
            "NEVRÁCENO ✗"
        }
    );
    if !restored {
        fails += 1;
    }

    // Winlogon hook musí být zamítnutý.
    let denied = ipc::client::toggle_action(Action::StartupToggle {
        id: "shell|Userinit".into(),
        on: false,
    })
    .expect("toggle shell");
    println!(
        "\nWinlogon hook: {} {:?}",
        denied.verdict, denied.deny_reason
    );
    if denied.verdict != "deny" {
        fails += 1;
    }

    // Neexistující položka musí být zamítnutá.
    let ghost = ipc::client::toggle_action(Action::StartupToggle {
        id: "run_user|NeexistujiciPolozkaXyz".into(),
        on: false,
    })
    .expect("toggle ghost");
    println!(
        "neexistující položka: {} {:?}",
        ghost.verdict, ghost.deny_reason
    );
    if ghost.verdict != "deny" {
        fails += 1;
    }

    println!("\ncelkem selhání: {fails}");
    std::process::exit(if fails == 0 { 0 } else { 1 });
}
