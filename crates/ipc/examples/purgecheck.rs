//! Brána: úklid záznamu v registru se nabídne JEN po programu, který
//! na disku není. `cargo run -p ipc --example purgecheck`
//!
//! Nic se nemaže — žádá se pouze PLÁN (fáze 1), která je čtecí.
//! Ověřuje se obojí:
//!   • u nainstalovaného programu musí vrstva úklid ODMÍTNOUT,
//!   • u ducha (záznam je, instalace na disku není) musí dát plán,
//!     nebo odmítnout s důvodem — nikdy nespadnout.

use core_types::action::Action;

fn main() {
    let apps = match ipc::client::query_apps() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("query_apps selhal: {e}");
            std::process::exit(1);
        }
    };
    let ghosts: Vec<_> = apps
        .iter()
        .filter(|a| a.missing_install && a.uninstaller_missing && a.kind == "desktop")
        .collect();
    let live: Vec<_> = apps
        .iter()
        .filter(|a| !a.missing_install && a.kind == "desktop")
        .collect();
    println!("aplikací {} — duchů {}, živých {}", apps.len(), ghosts.len(), live.len());

    let mut fails = 0;

    // Živý program se z registru uklidit NESMÍ.
    for a in live.iter().take(5) {
        match ipc::client::plan_action(Action::PurgeGhost {
            identity_key: a.identity_key.clone(),
        }) {
            Ok(Ok(_)) => {
                println!("CHYBA: {} je na disku, a přesto se nabídl úklid", a.display_name);
                fails += 1;
            }
            Ok(Err(d)) => println!("  deny OK: {} → {}", a.display_name, d.deny_reason.unwrap_or_default()),
            Err(e) => {
                println!("CHYBA: {} skončilo chybou: {e}", a.display_name);
                fails += 1;
            }
        }
    }

    // Duch: plán se buď vydá, nebo se odmítne s důvodem.
    for a in ghosts.iter().take(5) {
        match ipc::client::plan_action(Action::PurgeGhost {
            identity_key: a.identity_key.clone(),
        }) {
            Ok(Ok(p)) => println!(
                "  plán OK: {} → {} kroků, první: {}",
                a.display_name,
                p.steps.len(),
                p.steps.first().map(|s| s.description.as_str()).unwrap_or("—")
            ),
            Ok(Err(d)) => println!("  deny: {} → {}", a.display_name, d.deny_reason.unwrap_or_default()),
            Err(e) => {
                println!("CHYBA: {} skončilo chybou: {e}", a.display_name);
                fails += 1;
            }
        }
    }

    println!("\nBRÁNA purgecheck: {}", if fails == 0 { "PASS" } else { "FAIL" });
    if fails > 0 {
        std::process::exit(1);
    }
}
