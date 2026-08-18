//! Brána v9D — historie použití oprávnění.
//! `cargo run -p ipc --example permusecheck`
//!
//! Definice hotového (ROADMAP v9): *„Discord používal mikrofon včera
//! 3 h 12 min."* Takovou větu z registru přečíst NEJDE — ConsentStore
//! si pamatuje jen poslední sezení a při dalším použití ho přepíše.
//! Historii si proto zapisuje služba sama, jak ji vidí přicházet přes
//! `RegNotifyChangeKeyValue`.
//!
//! Hlídá se, že se opravdu zapisuje, že sezení dávají smysl (konec po
//! začátku, žádné budoucí časy) a že se dotaz vejde do rozpočtu.

use std::time::Instant;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn main() {
    let mut fail = 0;

    match ipc::client::ping() {
        Ok(p) if p.protocol_version == core_types::ipc::PROTOCOL_VERSION => {
            println!("OK  protokol v{}", p.protocol_version)
        }
        Ok(p) => {
            println!(
                "!!  služba běží na v{}, čekáme v{}",
                p.protocol_version,
                core_types::ipc::PROTOCOL_VERSION
            );
            std::process::exit(1);
        }
        Err(e) => {
            println!("!!  služba neodpovídá: {e}");
            std::process::exit(1);
        }
    }

    let sec = match ipc::client::query_security() {
        Ok(s) => s,
        Err(e) => {
            println!("!!  dotaz na Security selhal: {e}");
            std::process::exit(1);
        }
    };

    // Kandidáti: co už někdy použité bylo. Na čerstvě nainstalovaném
    // stroji jich být nemusí — to není chyba, jen se nedá co ověřit.
    let used: Vec<_> = sec
        .permissions
        .iter()
        .filter(|p| p.last_start.is_some())
        .collect();
    println!("    záznamů s historií použití: {}", used.len());
    if used.is_empty() {
        println!("--  na tomhle stroji zatím nikdo nic nepoužil — nelze ověřit");
        println!("\nv9D-history: PASS (nic k ověření)");
        return;
    }

    let now = unix_now();
    // Rozpočet se měří na jednotlivých dotazech níž.
    let mut with_history = 0;
    let mut checked = 0;
    let mut worst_ms = 0u128;

    for p in used.iter().take(25) {
        let t = Instant::now();
        let (sessions, total_s) =
            match ipc::client::query_perm_use(p.app.clone(), p.capability.clone(), 30) {
                Ok(x) => x,
                Err(e) => {
                    println!("!!  dotaz na historii selhal: {e}");
                    fail += 1;
                    break;
                }
            };
        worst_ms = worst_ms.max(t.elapsed().as_millis());
        checked += 1;
        if sessions.is_empty() {
            continue;
        }
        with_history += 1;

        // Sezení musí dávat smysl: konec po začátku, nic z budoucnosti.
        for s in &sessions {
            if let Some(stop) = s.stop_ts {
                if stop < s.start_ts {
                    println!("!!  {} · {}: konec před začátkem", p.app_name, p.capability);
                    fail += 1;
                }
            }
            if s.start_ts > now + 60 {
                println!("!!  {} · {}: sezení z budoucnosti", p.app_name, p.capability);
                fail += 1;
            }
        }
        // Součet nesmí přesáhnout délku okna.
        if total_s > 30 * 86_400 {
            println!("!!  {} · {}: součet delší než okno", p.app_name, p.capability);
            fail += 1;
        }
    }

    println!(
        "OK  ověřeno {checked} oprávnění, historii má {with_history}, nejpomalejší dotaz {worst_ms} ms"
    );
    if worst_ms >= 500 {
        println!("!!  dotaz na historii trvá {worst_ms} ms — mimo rozpočet");
        fail += 1;
    }

    // Přehled pro člověka: kde se co nasbíralo.
    let mut top: Vec<(String, String, i64)> = Vec::new();
    for p in used.iter().take(25) {
        if let Ok((_, total)) = ipc::client::query_perm_use(p.app.clone(), p.capability.clone(), 30)
        {
            if total > 0 {
                top.push((p.app_name.clone(), p.capability.clone(), total));
            }
        }
    }
    top.sort_by_key(|(_, _, t)| std::cmp::Reverse(*t));
    if !top.is_empty() {
        println!("\n    nejvíc času za 30 dní:");
        for (name, cap, secs) in top.iter().take(8) {
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            println!("      {name} · {cap}: {h} h {m} min");
        }
    } else {
        println!("\n    zatím se nic nenasbíralo — historie roste od instalace téhle verze");
    }

    println!();
    if fail == 0 {
        println!("v9D-history: PASS");
    } else {
        println!("v9D-history: FAIL ({fail} problémů)");
        std::process::exit(1);
    }
}
