//! Brána: „Obnovit“ v Programech musí něco znamenat.
//! `cargo run -p ipc --example rescancheck`
//!
//! Změří, za jak dlouho po požadavku na sken přiteče do databáze nový
//! inventář — přesně na tenhle signál čeká UI, než překreslí seznam.

use std::time::{Duration, Instant};

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

    let (_, before) = ipc::client::query_inv_status().expect("stav inventáře");
    println!("--  poslední zápis inventáře: {before}");
    ipc::client::rescan_apps().expect("žádost o sken");

    let t0 = Instant::now();
    let mut seen_scanning = false;
    let mut done = false;
    while t0.elapsed() < Duration::from_secs(90) {
        std::thread::sleep(Duration::from_millis(1500));
        let (scanning, ts) = ipc::client::query_inv_status().expect("stav inventáře");
        seen_scanning |= scanning;
        if ts > before && !scanning {
            println!(
                "OK  nový inventář v DB za {:.1} s (razítko {ts})",
                t0.elapsed().as_secs_f32()
            );
            done = true;
            break;
        }
    }
    if !done {
        fail += 1;
        println!("!!  do 90 s nepřišel nový inventář — „Obnovit“ by mlčelo");
    }
    if seen_scanning {
        println!("OK  stav „skenuji“ byl vidět — točící se ikona má co ukazovat");
    } else {
        println!("--  sken proběhl mezi dvěma dotazy (nevadí, razítko rozhoduje)");
    }

    // Seznam po skenu musí jít přečíst a nesmí být prázdný.
    match ipc::client::query_apps() {
        Ok(apps) if !apps.is_empty() => println!("OK  seznam po skenu: {} aplikací", apps.len()),
        Ok(_) => {
            fail += 1;
            println!("!!  seznam po skenu je prázdný");
        }
        Err(e) => {
            fail += 1;
            println!("!!  seznam nejde přečíst: {e}");
        }
    }

    println!(
        "\n{}",
        if fail == 0 {
            "rescan: PASS"
        } else {
            "rescan: FAIL"
        }
    );
}
