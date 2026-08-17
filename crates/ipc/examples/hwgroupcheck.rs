//! Brána: hardware se slučuje do skutečných kusů.
//! `cargo run -p ipc --example hwgroupcheck`
//!
//! Windows rozepíšou jeden kus hardwaru na řadu zařízení — rozhraní,
//! HID kolekce, vlastní sběrnice výrobce. Seznam pak vypadá jako
//! „x krát nějaký HID". Sloučit je podle jména by ale bylo horší než
//! nesloučit nic: „USB Input Device" bývá v systému šestkrát a jsou to
//! dvě různá zařízení po třech rozhraních.
//!
//! Hlídají se proto obě strany: že se seznam opravdu zkrátil, a že se
//! přitom neslily věci, které patří každá jinam.

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

    let hw = match ipc::client::query_hardware() {
        Ok(h) => h,
        Err(e) => {
            println!("!!  dotaz na hardware selhal: {e}");
            std::process::exit(1);
        }
    };
    let devs = &hw.devices;
    println!("    zařízení v systému: {}", devs.len());

    // 1) Klíč i jméno skupiny musí být vyplněné u všech.
    let empty = devs
        .iter()
        .filter(|d| d.group_key.is_empty() || d.group_name.is_empty())
        .count();
    if empty == 0 {
        println!("OK  každé zařízení má klíč i jméno skupiny");
    } else {
        println!("!!  {empty} zařízení bez klíče nebo jména skupiny");
        fail += 1;
    }

    // 2) Jméno skupiny je shodné pro všechny její členy — jinak by
    //    záleželo na tom, který řádek se trefí první.
    let mut groups: std::collections::HashMap<&str, Vec<&core_types::proc::DeviceRow>> =
        std::collections::HashMap::new();
    for d in devs {
        groups.entry(&d.group_key).or_default().push(d);
    }
    let inconsistent = groups
        .values()
        .filter(|g| g.iter().any(|d| d.group_name != g[0].group_name))
        .count();
    if inconsistent == 0 {
        println!("OK  jméno skupiny je uvnitř skupiny jednotné");
    } else {
        println!("!!  {inconsistent} skupin má rozhádaná jména");
        fail += 1;
    }

    // 3) Seznam se musí opravdu zkrátit. Na stroji s periferiemi je
    //    rozdíl výrazný; kdyby slučování přestalo fungovat, spadne to.
    println!(
        "    po sloučení: {} kusů ({} ušetřených řádků)",
        groups.len(),
        devs.len() - groups.len()
    );
    if groups.len() < devs.len() {
        println!("OK  slučování něco spojilo");
    } else {
        println!("!!  nic se nesloučilo — klíč nefunguje");
        fail += 1;
    }

    // 4) Nesmí vzniknout skupina, která spojí dvě různá VID/PID.
    //    Tohle je ta drahá chyba: dvě myši v jednom řádku.
    let mut mixed = 0;
    for (key, g) in &groups {
        let ids: std::collections::HashSet<String> = g
            .iter()
            .filter_map(|d| {
                let id = d.hardware_id.to_ascii_uppercase();
                let vid = id.find("VID_")?;
                let pid = id.find("PID_")?;
                Some(format!("{}|{}", &id[vid..vid + 8], &id[pid..pid + 8]))
            })
            .collect();
        if ids.len() > 1 {
            println!("!!  skupina {key} míchá zařízení: {ids:?}");
            mixed += 1;
        }
    }
    if mixed == 0 {
        println!("OK  žádná skupina nemíchá dvě různá zařízení");
    } else {
        fail += 1;
    }

    // 5) Problém kteréhokoliv rozhraní se nesmí ztratit — UI ho bere
    //    z členů, takže tady stačí ukázat, kde jaký je.
    let bad: Vec<_> = devs.iter().filter(|d| d.problem_code != 0).collect();
    println!("    zařízení s problémem: {}", bad.len());
    for d in bad.iter().take(5) {
        println!("      {} · {} (kód {})", d.group_name, d.name, d.problem_code);
    }

    // Přehled největších úspor — sem se dívá člověk, když ladí klíč.
    let mut big: Vec<_> = groups.values().filter(|g| g.len() > 1).collect();
    big.sort_by_key(|g| std::cmp::Reverse(g.len()));
    println!("\n    nejvíc rozepsaná zařízení:");
    for g in big.iter().take(8) {
        println!("      {:>3} řádků → {}", g.len(), g[0].group_name);
    }

    println!();
    if fail == 0 {
        println!("BRÁNA hwgroupcheck: PASS");
    } else {
        println!("BRÁNA hwgroupcheck: FAIL ({fail} problémů)");
        std::process::exit(1);
    }
}
