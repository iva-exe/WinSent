//! Brána: incidenty umí přečíst, co o pádech píšou Windows.
//! `cargo run -p ipc --example crashcheck`
//!
//! Definice hotového: k pádu má být vidět lidsky co se stalo, kdo je
//! viník a co se dělo. Hlídá se, že se hlášení opravdu čtou a že se
//! nikdy netvrdí příčina, kterou neznáme.

fn main() {
    let mut fail = 0;

    match ipc::client::ping() {
        Ok(p) if p.protocol_version == core_types::ipc::PROTOCOL_VERSION => {
            println!("OK  protokol v{}", p.protocol_version)
        }
        Ok(p) => {
            println!("!!  služba běží na v{}, čekáme v{}", p.protocol_version,
                core_types::ipc::PROTOCOL_VERSION);
            std::process::exit(1);
        }
        Err(e) => {
            println!("!!  služba neodpovídá: {e}");
            std::process::exit(1);
        }
    }

    let t = std::time::Instant::now();
    let rows = match ipc::client::query_crash_reports(40) {
        Ok(r) => r,
        Err(e) => {
            println!("!!  dotaz na hlášení selhal: {e}");
            std::process::exit(1);
        }
    };
    let ms = t.elapsed().as_millis();
    println!("    hlášení o pádech: {} (dotaz {ms} ms)", rows.len());

    if rows.is_empty() {
        println!("--  na tomhle stroji Windows žádný pád nezaznamenaly");
        println!("\nBRÁNA crashcheck: PASS (nic k ověření)");
        return;
    }

    // 1) Každé hlášení musí mít větu, ne prázdno.
    let bez = rows.iter().filter(|r| r.summary.trim().is_empty()).count();
    if bez == 0 {
        println!("OK  každé hlášení má srozumitelné shrnutí");
    } else {
        println!("!!  {bez} hlášení bez shrnutí");
        fail += 1;
    }

    // 2) Shrnutí musí jmenovat aplikaci — jinak není o čem.
    let bezapp = rows.iter().filter(|r| !r.summary.contains(&r.app)).count();
    if bezapp == 0 {
        println!("OK  shrnutí vždy jmenuje aplikaci");
    } else {
        println!("!!  {bezapp} hlášení nejmenuje aplikaci");
        fail += 1;
    }

    // 3) Pád v systémové knihovně se NESMÍ vydávat za chybu Windows.
    const SYS: &[&str] = &["ntdll.dll", "kernelbase.dll", "kernel32.dll"];
    let mut lzi = 0;
    for r in &rows {
        if SYS.iter().any(|s| r.module.eq_ignore_ascii_case(s))
            && !r.detail.contains("neznamená chybu Windows")
        {
            lzi += 1;
        }
    }
    if lzi == 0 {
        println!("OK  systémová knihovna se nevydává za viníka");
    } else {
        println!("!!  {lzi} hlášení svaluje vinu na systémovou knihovnu");
        fail += 1;
    }

    // 4) Rozpočet: čtení protokolu je historie, ne živá metrika, ale
    //    vteřiny by znamenaly, že se prochází celý protokol.
    if ms < 3000 {
        println!("OK  dotaz v rozpočtu ({ms} ms)");
    } else {
        println!("!!  dotaz trval {ms} ms — nefiltruje se dost?");
        fail += 1;
    }

    println!("\n    ukázka:");
    for r in rows.iter().take(5) {
        println!("      {}", r.summary);
        if r.repeats > 1 {
            println!("        ({}× ve stejném místě)", r.repeats);
        }
    }

    println!();
    if fail == 0 {
        println!("BRÁNA crashcheck: PASS");
    } else {
        println!("BRÁNA crashcheck: FAIL ({fail} problémů)");
        std::process::exit(1);
    }
}
