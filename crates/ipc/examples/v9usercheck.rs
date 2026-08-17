//! Brána v9E — Users. `cargo run -p ipc --example v9usercheck`
//!
//! Definice hotového (ROADMAP v9): „Users ukáže, kdo má admin práva."
//! K tomu podmínka celé v9: každá sekce v rozpočtu, žádné WMI zatuhnutí.
//! Právě proto se účty čtou přes netapi32 a ne přes `Win32_UserAccount`,
//! který se na stroji v doméně ptá řadiče.

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
    let r = match ipc::client::query_users() {
        Ok(r) => r,
        Err(e) => {
            println!("!!  dotaz na účty selhal: {e}");
            std::process::exit(1);
        }
    };
    let first_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    let _ = ipc::client::query_users();
    let cached_ms = t1.elapsed().as_millis();

    // 1) Skupina správců se musí najít v jazyce systému. Natvrdo
    //    „Administrators" by na české instalaci nenašlo nic a sekce by
    //    tvrdila, že admina nemá nikdo.
    if r.admin_group.is_empty() {
        println!("!!  skupina správců nemá jméno");
        fail += 1;
    } else {
        println!("OK  skupina správců: {}", r.admin_group);
    }

    // 2) Aspoň jeden účet — do Windows se musí dát přihlásit.
    if r.users.is_empty() {
        println!("!!  žádný lokální účet");
        fail += 1;
    } else {
        println!("OK  {} lokálních účtů", r.users.len());
    }

    // 3) DoD: musí být vidět, KDO má admin práva.
    let admins: Vec<&core_types::proc::UserRow> = r.users.iter().filter(|u| u.admin).collect();
    let total_admins = admins.len() + r.foreign_admins.len();
    if total_admins == 0 {
        println!("!!  nikdo nemá práva správce — párování se skupinou nefunguje");
        fail += 1;
    } else {
        println!(
            "OK  správců {} ({} lokálních, {} mimo tenhle počítač)",
            total_admins,
            admins.len(),
            r.foreign_admins.len()
        );
    }

    // 4) SID musí být u všech — bez něj by členství ve skupině
    //    vycházelo jen náhodou.
    let no_sid = r.users.iter().filter(|u| !u.sid.starts_with("S-1-")).count();
    if no_sid == 0 {
        println!("OK  každý účet má SID");
    } else {
        println!("!!  {no_sid} účtů bez SID");
        fail += 1;
    }

    // 5) Rozpočet: čtení místní databáze účtů je otázka milisekund.
    //    Vteřiny by znamenaly, že se někam sáhlo přes síť.
    if first_ms < 1000 {
        println!("OK  dotaz: první {first_ms} ms, z cache {cached_ms} ms");
    } else {
        println!("!!  dotaz trval {first_ms} ms — čte se něco přes síť?");
        fail += 1;
    }

    println!("\n    účty:");
    for u in &r.users {
        let mut tags = Vec::new();
        if u.admin {
            tags.push("správce");
        }
        if u.disabled {
            tags.push("vypnutý");
        }
        if u.locked {
            tags.push("zamčený");
        }
        if u.password_not_required {
            tags.push("heslo nevyžadováno");
        }
        if u.microsoft {
            tags.push("účet Microsoft");
        }
        let last = if u.last_logon == 0 {
            "nikdy".to_string()
        } else {
            format!("{}× naposledy {}", u.logons, u.last_logon)
        };
        println!(
            "      {:<20} {:<34} {}",
            u.name,
            tags.join(", "),
            last
        );
    }
    for f in &r.foreign_admins {
        println!("      {:<20} {} ({})", f.name, "správce mimo SAM", f.kind);
    }

    println!();
    if fail == 0 {
        println!("v9E: PASS");
    } else {
        println!("v9E: FAIL ({fail} problémů)");
        std::process::exit(1);
    }
}
