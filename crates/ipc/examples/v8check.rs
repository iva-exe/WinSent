//! Brána v8: držitelé souborů, mazání do koše přes vrstvu, detekce
//! chybějících instalací. `cargo run -p ipc --example v8check`

fn main() {
    let mut fails = 0;

    // ── 1. Kdo drží soubor: vlastní běžící .exe drží sám sebe ──
    let self_exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    match ipc::client::query_holders(vec![self_exe.clone()]) {
        Ok(hs) => {
            println!("drzitele {}: {}", self_exe, hs.len());
            for h in hs.iter().take(3) {
                println!("  {} (pid {}, {})", h.name, h.pid, h.kind);
            }
        }
        Err(e) => {
            println!("CHYBA query_holders: {e}");
            fails += 1;
        }
    }

    // ── 2. Systémový soubor NESMÍ jít smazat ──
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    for probe in [
        format!(r"{sysroot}\System32\kernel32.dll"),
        format!(r"{sysroot}"),
        r"C:\".to_string(),
    ] {
        match ipc::client::plan_action(core_types::action::Action::DeleteFiles {
            paths: vec![probe.clone()],
        }) {
            Ok(Err(deny)) => println!("OK zamitnuto {probe}: {:?}", deny.deny_reason),
            Ok(Ok(_)) => {
                println!("SELHANI: {probe} PROSLO validaci!");
                fails += 1;
            }
            Err(e) => {
                println!("CHYBA plan: {e}");
                fails += 1;
            }
        }
    }

    // ── 3. Skutečné smazání testovacího souboru do koše ──
    let tmp = std::env::temp_dir().join("winsent-v8-gate.tmp");
    std::fs::write(&tmp, b"gate").expect("zapsat testovaci soubor");
    let path = tmp.to_string_lossy().into_owned();
    match ipc::client::plan_action(core_types::action::Action::DeleteFiles {
        paths: vec![path.clone()],
    }) {
        Ok(Ok(plan)) => {
            println!("plan mazani: {} kroku", plan.steps.len());
            match ipc::client::execute_action(plan.plan_id) {
                Ok(r) => {
                    let gone = !tmp.exists();
                    println!(
                        "provedeni: {} / {:?}; soubor pryc: {gone}",
                        r.verdict, r.outcome
                    );
                    if !gone {
                        fails += 1;
                    }
                }
                Err(e) => {
                    println!("CHYBA execute: {e}");
                    fails += 1;
                }
            }
        }
        Ok(Err(d)) => {
            println!("SELHANI: temp soubor zamitnut: {:?}", d.deny_reason);
            fails += 1;
        }
        Err(e) => {
            println!("CHYBA plan: {e}");
            fails += 1;
        }
    }
    let _ = std::fs::remove_file(&tmp);

    // ── 4. Aplikace, po kterých zbyl jen záznam ──
    match ipc::client::query_apps() {
        Ok(apps) => {
            let ghosts: Vec<_> = apps.iter().filter(|a| a.missing_install).collect();
            println!("\naplikaci s chybejici instalaci: {}", ghosts.len());
            for a in ghosts.iter().take(6) {
                println!("  {}", a.display_name);
            }
        }
        Err(e) => println!("CHYBA query_apps: {e}"),
    }

    // ── 5. Odinstalace: plán ANO (nespouštíme!), nesmysly NE ──
    for (key, want_plan) in [
        ("app:rozhodne neexistujici aplikace 999", false),
        ("msix:Microsoft.WindowsCalculator_8wekyb3d8bbwe", false),
        ("", false),
    ] {
        match ipc::client::plan_action(core_types::action::Action::UninstallApp {
            identity_key: key.to_string(),
        }) {
            Ok(Err(d)) if !want_plan => println!("OK zamitnuto {key}: {:?}", d.deny_reason),
            Ok(Ok(_)) if !want_plan => {
                println!("SELHANI: {key} dostal plan!");
                fails += 1;
            }
            Ok(_) => {}
            Err(e) => {
                println!("CHYBA plan uninstall: {e}");
                fails += 1;
            }
        }
    }

    // Reálná aplikace: plán se MÁ sestavit (ale neprovádíme ho).
    if let Ok(apps) = ipc::client::query_apps() {
        if let Some(app) = apps.iter().find(|a| {
            a.kind == "desktop"
                && !a.missing_install
                && a.display_name.to_lowercase().contains("discord")
        }) {
            match ipc::client::plan_action(core_types::action::Action::UninstallApp {
                identity_key: app.identity_key.clone(),
            }) {
                Ok(Ok(p)) => println!(
                    "OK plan odinstalace {}: {} kroku (NEPROVADIME)",
                    app.display_name,
                    p.steps.len()
                ),
                Ok(Err(d)) => println!(
                    "pozn.: {} nejde odinstalovat: {:?}",
                    app.display_name, d.deny_reason
                ),
                Err(e) => {
                    println!("CHYBA: {e}");
                    fails += 1;
                }
            }
        }
    }

    println!("\ncelkem selhani: {fails}");
    std::process::exit(if fails == 0 { 0 } else { 1 });
}
