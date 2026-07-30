//! Brána v9A — Hardware. `cargo run -p ipc --example v9check`
//!
//! Hlídá hlavně jedno pravidlo ze SPEC kap. 15.2: **nikdy nepředstírej
//! číslo, které nemáš.** Teplota bez zdroje je chyba, zdroj bez teploty
//! taky. A poslední stupeň kaskády (takty) musí fungovat vždy.

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
    let hw = match ipc::client::query_hardware() {
        Ok(h) => h,
        Err(e) => {
            println!("!!  hardwarový přehled selhal: {e}");
            std::process::exit(1);
        }
    };
    let first_ms = t0.elapsed().as_millis();

    // 1) Kaskáda: buď teplota SE zdrojem, nebo poctivé „nedostupné".
    let th = &hw.cpu_thermal;
    match th.celsius {
        Some(c) if th.temp_source != "nedostupné" && (1.0..=125.0).contains(&c) => {
            println!("OK  teplota CPU {c:.0} °C, zdroj: {}", th.temp_source)
        }
        Some(c) => {
            fail += 1;
            println!("!!  teplota {c} se zdrojem {} — nesmysl", th.temp_source);
        }
        None if th.temp_source == "nedostupné" => {
            println!("OK  teplota CPU nedostupná a přiznaná (žádné vymyšlené číslo)")
        }
        None => {
            fail += 1;
            println!("!!  bez teploty, ale zdroj hlásí {}", th.temp_source);
        }
    }

    // 2) Poslední stupeň kaskády musí fungovat na každém stroji.
    if th.max_mhz > 0 && th.clock_mhz > 0 {
        println!(
            "OK  takty {} / {} MHz — brzdí ho něco: {}",
            th.clock_mhz,
            th.max_mhz,
            if th.throttling { "ano" } else { "ne" }
        );
    } else {
        fail += 1;
        println!("!!  takty se nepodařilo přečíst — poslední stupeň kaskády selhal");
    }

    // 3) Deska a firmware.
    let b = &hw.board;
    if b.product.is_empty() && b.bios_version.is_empty() {
        fail += 1;
        println!("!!  SMBIOS nevrátil desku ani BIOS");
    } else {
        println!(
            "OK  deska {} {} · BIOS {} z {}",
            b.manufacturer, b.product, b.bios_version, b.bios_date
        );
    }

    // 4) Baterie: buď žádná (desktop), nebo smysluplná.
    match &hw.battery {
        None => println!("--  baterie žádná (desktop) — správně se nepředstírá"),
        Some(bat) => {
            let sane = bat.percent.is_none_or(|p| p <= 100)
                && bat.wear_pct.is_none_or(|w| (0.0..=100.0).contains(&w));
            // Opotřebení bez obou kapacit je vymyšlené číslo.
            let honest =
                bat.wear_pct.is_none() || (bat.design_mwh.is_some() && bat.full_mwh.is_some());
            if sane && honest {
                println!(
                    "OK  baterie {:?} %, opotřebení {:?}, cyklů {:?}",
                    bat.percent, bat.wear_pct, bat.cycles
                );
            } else {
                fail += 1;
                println!("!!  baterie hlásí nesmysl: {bat:?}");
            }
        }
    }

    // 5) Disky a svazky.
    if hw.volumes.is_empty() {
        fail += 1;
        println!("!!  žádné pevné svazky — systémový disk musí být vidět");
    } else {
        println!(
            "OK  {} disků, {} pevných svazků",
            hw.disks.len(),
            hw.volumes.len()
        );
        for d in &hw.disks {
            println!(
                "    disk {} {} — teplota {:?}, opotřebení {:?}",
                d.index, d.model, d.temp_c, d.used_pct
            );
        }
    }

    // 6) Rozpočet: kaskáda sahá na WMI, proto se výsledek cachuje.
    // Druhé volání musí být prakticky zdarma.
    let t1 = Instant::now();
    let _ = ipc::client::query_hardware();
    let cached_ms = t1.elapsed().as_millis();
    if cached_ms <= 50 {
        println!("OK  přehled: první {first_ms} ms, z cache {cached_ms} ms");
    } else {
        fail += 1;
        println!("!!  cache nefunguje: druhé volání {cached_ms} ms (WMI v cyklu je zakázané)");
    }

    println!("\n{}", if fail == 0 { "v9A: PASS" } else { "v9A: FAIL" });
}
