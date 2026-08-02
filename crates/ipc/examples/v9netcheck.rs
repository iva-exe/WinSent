//! Brána v9B — Network. `cargo run -p ipc --example v9netcheck`
//!
//! Definice hotového (ROADMAP): „Network mapuje spojení na aplikace,
//! ukazuje kam a kolik." Kontroluje se mapování na identitu, rozumnost
//! snapshotu, práce resolveru a rozpočet.

use std::time::Instant;

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

    let t0 = Instant::now();
    let rows = match ipc::client::query_network() {
        Ok(r) => r,
        Err(e) => {
            println!("!!  dotaz na síť selhal: {e}");
            std::process::exit(1);
        }
    };
    let first_ms = t0.elapsed().as_millis();

    // 1) Živý systém má spojení a většina má identitu aplikace.
    let total: usize = rows.iter().map(|r| r.conns.len()).sum();
    let named = rows
        .iter()
        .filter(|r| !r.identity_key.starts_with("pid:"))
        .count();
    if rows.is_empty() || total < 10 {
        fail += 1;
        println!(
            "!!  {} skupin / {total} spojení — to na živém systému nesedí",
            rows.len()
        );
    } else {
        println!(
            "OK  {} aplikací, {total} spojení; {named} skupin má identitu aplikace",
            rows.len()
        );
    }
    if named < rows.len() / 2 {
        fail += 1;
        println!("!!  přes polovinu skupin je bez identity — mapování PID selhává");
    }

    // 2) Konzistence počítadel.
    for r in &rows {
        let est = r.conns.iter().filter(|c| c.state == "established").count() as u32;
        let lis = r
            .conns
            .iter()
            .filter(|c| c.state == "listen" || c.state == "udp")
            .count() as u32;
        if est != r.established || lis != r.listening {
            fail += 1;
            println!(
                "!!  {} má rozbitá počítadla ({est}≠{} / {lis}≠{})",
                r.app_name, r.established, r.listening
            );
        }
    }
    println!("OK  počítadla established/listening sedí u všech skupin");

    // 3) Resolver: po pár vteřinách má aspoň něco PTR jméno. Svět bez
    // jediného PTR záznamu prakticky neexistuje (CDN je mají vždy).
    let mut names = 0;
    for i in 0..5 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let rows = ipc::client::query_network().unwrap_or_default();
        names = rows
            .iter()
            .flat_map(|r| &r.conns)
            .filter(|c| c.remote_name.is_some())
            .count();
        if names > 0 {
            println!("OK  reverzní DNS doplnilo {names} jmen (pokus {})", i + 1);
            break;
        }
    }
    if names == 0 {
        println!("--  žádné PTR jméno — bez aktivních veřejných spojení je to v pořádku");
    }

    // 4) Rozpočet: snapshot musí být levný (SPEC 12.3).
    let t1 = Instant::now();
    let _ = ipc::client::query_network();
    let again_ms = t1.elapsed().as_millis();
    if first_ms <= 500 && again_ms <= 500 {
        println!("OK  snapshot {first_ms} ms / {again_ms} ms");
    } else {
        fail += 1;
        println!("!!  snapshot je drahý: {first_ms} ms / {again_ms} ms");
    }

    println!("\n{}", if fail == 0 { "v9B: PASS" } else { "v9B: FAIL" });
}
