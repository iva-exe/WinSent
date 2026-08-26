//! Brána: služba hlásí verzi Windows a stav aktualizací.
//! `cargo run -p ipc --example oscheck`
//!
//! SPEC kap. 13.1 tyhle údaje na obrazovce „jsem chráněný?" vyžaduje,
//! ale nikdo je nesbíral — v záznamu o počítači tak chybělo i to, jaké
//! Windows na stroji vlastně běží. Kontroluje se, že přijdou vyplněné
//! a že se jméno systému shoduje s číslem sestavení (registr u
//! jedenáctky pořád tvrdí „Windows 10").

fn main() {
    let rep = match ipc::client::query_security() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("query_security selhal: {e}");
            std::process::exit(1);
        }
    };
    let o = &rep.protection.os;
    println!("  {} {} {}", o.product, o.display_version.as_deref().unwrap_or(""), o.arch);
    println!("  sestavení {}.{}", o.build, o.ubr);
    println!("  instalace {:?}", o.install_ts);
    println!("  hledáno {:?}, instalováno {:?}", o.update_last_search, o.update_last_install);
    println!(
        "  služba wuauserv start={:?}, zásada zakazuje={}",
        o.update_service_start, o.update_disabled_by_policy
    );

    let mut fails = 0;
    if o.build == 0 {
        println!("CHYBA: sestavení nepřečteno");
        fails += 1;
    }
    if o.product.trim().is_empty() {
        println!("CHYBA: jméno systému nepřečteno");
        fails += 1;
    }
    if o.arch.is_empty() {
        println!("CHYBA: architektura nepřečtena");
        fails += 1;
    }
    // Registr u Windows 11 pořád hlásí „Windows 10 Pro"; sestavení
    // 22000 a výš znamená jedenáctku a jméno se musí opravit.
    if o.build >= 22000 && o.product.contains("Windows 10") {
        println!("CHYBA: sestavení {} a jméno {:?}", o.build, o.product);
        fails += 1;
    }
    // Instalační čas v budoucnosti nebo před rokem 2000 je nesmysl.
    if let Some(t) = o.install_ts {
        if !(946_684_800..4_102_444_800).contains(&t) {
            println!("CHYBA: nesmyslný čas instalace {t}");
            fails += 1;
        }
    }

    println!("\nBRÁNA oscheck: {}", if fails == 0 { "PASS" } else { "FAIL" });
    if fails > 0 {
        std::process::exit(1);
    }
}
