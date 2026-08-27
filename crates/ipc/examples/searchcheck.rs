//! Brána searchcheck: co vyhledávání nabízí, to musí jít prohledat.
//!
//! Přesně tudy šlo hledání souborů k zemi, aniž si toho kdokoliv všiml.
//! Služba u svazku hlásila „hotovo", ale index v paměti neměla — nejdřív
//! proto, že se do sdílené mapy vkládal až za úklidovou analýzou a za
//! největšími položkami všech svazků (naměřeno sedm minut), a pak proto,
//! že ho janitor po pěti minutách nečinnosti uvolnil a nikdo ho nepostavil
//! zpátky. UI tu chybu měnilo na prázdný výsledek, takže hledání na tom
//! disku napořád tvrdilo „nic se nenašlo".
//!
//! Brána dělá přesně to, co dělá UI: vezme svazky, které služba nabízí,
//! a na každém zkusí hledat. Nabídnutý svazek, na kterém hledání selže
//! nebo nic nevrátí, je chyba.

use std::time::{Duration, Instant};

fn main() {
    // Po startu služby se auto-index rozjíždí až po patnácti sekundách,
    // takže prázdný seznam ještě nic neznamená. Chvíli se počká —
    // ale ne donekonečna, prázdno napořád je taky závada.
    let cekani = Instant::now();
    let indexing = loop {
        match ipc::client::query_cleanup() {
            Ok((indexing, _, _)) if !indexing.is_empty() => break indexing,
            Ok(_) => {
                if cekani.elapsed() > Duration::from_secs(90) {
                    eprintln!("BRÁNA searchcheck: FAIL — služba nenabízí ani jeden svazek k hledání");
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_secs(3));
            }
            Err(e) => {
                eprintln!("query_cleanup selhal: {e}");
                std::process::exit(1);
            }
        }
    };

    let mut selhani = 0;
    let mut zkusenych = 0;
    for (letter, zaznamu, hotovo, chyba) in &indexing {
        if let Some(d) = chyba {
            // Svazek s chybou se v UI nenabízí; hlásí se jen do výpisu,
            // ať je vidět, proč chybí.
            println!("  {letter}: index selhal — {d}");
            continue;
        }
        if !hotovo {
            println!("  {letter}: index se ještě staví ({zaznamu} záznamů)");
            continue;
        }
        zkusenych += 1;
        let t0 = Instant::now();
        // „e" najde něco na každém svazku, na kterém vůbec něco je.
        match ipc::client::search_files(*letter, "e".into(), 5) {
            Ok(hits) if !hits.is_empty() => println!(
                "  {letter}: {} nálezů za {} ms (index {zaznamu} záznamů)",
                hits.len(),
                t0.elapsed().as_millis()
            ),
            Ok(_) => {
                eprintln!("  {letter}: hledání nevrátilo nic, přestože svazek má {zaznamu} záznamů");
                selhani += 1;
            }
            Err(e) => {
                eprintln!("  {letter}: hledání selhalo — {e}");
                selhani += 1;
            }
        }
    }

    if zkusenych == 0 {
        eprintln!("BRÁNA searchcheck: FAIL — žádný svazek není k dispozici k hledání");
        std::process::exit(1);
    }
    if selhani > 0 {
        eprintln!("BRÁNA searchcheck: FAIL ({selhani} svazků)");
        std::process::exit(1);
    }
    println!("BRÁNA searchcheck: PASS ({zkusenych} svazků)");
}
