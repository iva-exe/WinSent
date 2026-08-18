//! Brána v10 — Drivers. `cargo run -p ipc --example v10check`
//!
//! Definice hotového (ROADMAP v10): „Seznam ovladačů odpovídá Správci
//! zařízení." a „Ověř, že inventář neblokuje a je rychlý."
//!
//! Druhá půlka původní v10 — opt-in instalace ovladačů — se nestaví;
//! nástroj o systému vypovídá, neovládá ho. Instalovat a vracet ovladače
//! umí Windows Update a dělá to líp.

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
    let r = match ipc::client::query_drivers() {
        Ok(r) => r,
        Err(e) => {
            println!("!!  dotaz na ovladače selhal: {e}");
            std::process::exit(1);
        }
    };
    let first_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    let _ = ipc::client::query_drivers();
    let cached_ms = t1.elapsed().as_millis();

    // 1) Něco tu být musí — počítač bez ovladačů neexistuje.
    if r.drivers.is_empty() {
        println!("!!  žádné ovladače");
        fail += 1;
    } else {
        println!("OK  {} ovladačů", r.drivers.len());
    }

    // 2) Jedno zařízení = jeden řádek. Bez toho by měla jedna myš
    //    šestnáct řádků o témž ovladači.
    let mut seen = std::collections::HashSet::new();
    let dupes = r
        .drivers
        .iter()
        .filter(|d| !seen.insert(&d.group_key))
        .count();
    if dupes == 0 {
        println!("OK  žádné zařízení není v seznamu dvakrát");
    } else {
        println!("!!  {dupes} zařízení je v seznamu víckrát");
        fail += 1;
    }

    // 3) Řádek bez jména zařízení je k ničemu.
    let nameless = r.drivers.iter().filter(|d| d.device.is_empty()).count();
    if nameless == 0 {
        println!("OK  každý ovladač ví, kterému zařízení patří");
    } else {
        println!("!!  {nameless} ovladačů bez zařízení");
        fail += 1;
    }

    // 4) Rozpočet: SetupAPI je rychlé, WMI by tu bylo poznat na první
    //    pohled (Win32_PnPSignedDriver umí zatuhnout na desítky sekund).
    if first_ms < 3000 {
        println!("OK  dotaz: první {first_ms} ms, z cache {cached_ms} ms");
    } else {
        println!("!!  dotaz trval {first_ms} ms — sahá se někam, kam by se nemělo?");
        fail += 1;
    }

    // 5) Rozpoznání doinstalovaných musí něco najít; stroj úplně bez
    //    ovladačů od výrobce je teoreticky možný, ale stojí za zmínku.
    println!(
        "    od výrobců {} · s problémem {}",
        r.third_party, r.with_problem
    );
    if r.third_party == 0 {
        println!("--  žádný doinstalovaný ovladač (čerstvá instalace Windows?)");
    }

    println!("\n    nejstarší ovladače:");
    for d in r.drivers.iter().take(8) {
        println!(
            "      {:<34} {:<22} {:<12} {}",
            trim(&d.device, 33),
            trim(&d.provider, 21),
            d.version,
            d.date
        );
    }

    println!();
    if fail == 0 {
        println!("v10: PASS");
    } else {
        println!("v10: FAIL ({fail} problémů)");
        std::process::exit(1);
    }
}

fn trim(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}
