//! Brána: „používá právě teď" u oprávnění musí odpovídat realitě.
//! `cargo run -p ipc --example permlive`
//!
//! ConsentStore si konec relace pamatuje jen tehdy, když ho Windows
//! stihnou zapsat. Když aplikace spadne nebo počítač ztratí napájení,
//! `LastUsedTimeStop` zůstane nulový a záznam pak tvrdí „používá teď"
//! klidně rok. Naměřeno na cizím stroji: 32 aplikací prý drželo
//! mikrofon od února 2025, přestože systém nabootoval týž den v 18:57.
//!
//! Co se ověřuje u každého řádku s `in_use`:
//!   1. relace začala PO posledním startu systému,
//!   2. program, kterému patří, opravdu běží.

fn main() {
    let sec = match ipc::client::query_security() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("query_security selhal: {e}");
            std::process::exit(1);
        }
    };
    let procs = ipc::client::query_procs().unwrap_or_default();
    let sys = ipc::client::query_system().ok();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let boot = now - sys.as_ref().map(|s| s.uptime_s as i64).unwrap_or(0);

    let names: std::collections::HashSet<String> =
        procs.iter().map(|p| p.name.to_ascii_lowercase()).collect();
    let families: std::collections::HashSet<String> = procs
        .iter()
        .filter_map(|p| p.identity_key.strip_prefix("msix:"))
        .map(|f| f.to_ascii_lowercase())
        .collect();

    let live: Vec<_> = sec.permissions.iter().filter(|p| p.in_use).collect();
    println!(
        "oprávnění {} · z toho „používá teď\" {} · systém běží od {}",
        sec.permissions.len(),
        live.len(),
        boot
    );

    let mut fails = 0;
    for p in &live {
        if let Some(start) = p.last_start {
            if start < boot - 120 {
                println!(
                    "CHYBA: {} / {} hlásí použití teď, ale relace začala {} s před startem systému",
                    p.app_name,
                    p.capability,
                    boot - start
                );
                fails += 1;
            }
        }
        let running = if p.enforced {
            families.contains(&p.app.to_ascii_lowercase())
        } else {
            p.app
                .rsplit(char::from(92u8))
                .next()
                .map(|e| names.contains(&e.to_ascii_lowercase()))
                .unwrap_or(false)
        };
        if !running {
            println!(
                "CHYBA: {} / {} hlásí použití teď, ale žádný takový proces neběží ({})",
                p.app_name, p.capability, p.app
            );
            fails += 1;
        }
    }
    // Když skoro žádný záznam nemá čas použití, něco je špatně s cestou
    // ke klíči v registru — a pak by „používá teď" bylo trvale prázdné,
    // což vypadá jako „všechno v pořádku". Přesně tahle regrese se tu
    // jednou stala: chybějící zpětné lomítko v klíči.
    let with_time = sec.permissions.iter().filter(|p| p.last_used.is_some()).count();
    println!("s časem posledního použití: {with_time} z {}", sec.permissions.len());
    if sec.permissions.len() > 20 && with_time == 0 {
        println!("CHYBA: ani jedno oprávnění nemá čas použití — čte se špatný klíč?");
        fails += 1;
    }

    for p in live.iter().take(5) {
        println!("  {} — {} (od {})", p.app_name, p.capability, p.last_start.unwrap_or(0));
    }

    println!("\nBRÁNA permlive: {}", if fails == 0 { "PASS" } else { "FAIL" });
    if fails > 0 {
        std::process::exit(1);
    }
}
