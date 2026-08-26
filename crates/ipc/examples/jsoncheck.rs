//! Brána: velká celá čísla přežijí cestu do UI beze změny.
//! `cargo run -p ipc --example jsoncheck`
//!
//! Čas vzniku procesu je FILETIME, dnes zhruba 1,34 × 10¹⁷. JavaScript
//! umí přesně jen celá čísla do 2⁵³ ≈ 9 × 10¹⁵, takže se hodnota
//! poslaná jako JSON číslo zaokrouhlí na násobek šestnácti. Validační
//! vrstva pak vrácené číslo neuznala za tentýž proces a ukončení
//! odmítla s hláškou o recyklovaném PID — nešlo ukončit skoro nic.
//!
//! UI dostává hodnoty přes serde_json, takže se přesně tak testuje.

/// Největší celé číslo, které JavaScript udrží přesně.
const JS_SAFE: i64 = 9_007_199_254_740_991;

fn main() {
    let procs = match ipc::client::query_procs() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("query_snapshot selhal: {e}");
            std::process::exit(1);
        }
    };
    if procs.is_empty() {
        println!("FAIL: sampler nevrátil žádný proces");
        std::process::exit(1);
    }

    let mut fails = 0;
    let mut big = 0;
    for p in &procs {
        if p.create_time > JS_SAFE {
            big += 1;
        }
        // Kolečko přes JSON musí vrátit tutéž hodnotu — VČETNĚ toho, co
        // s ní udělá JavaScript. Rust přečte JSON číslo přesně, takže
        // samotné kolečko v Rustu by chybu neodhalilo; musí se
        // napodobit krok, kdy hodnota projde jako double.
        let json = serde_json::to_string(p).expect("serializace");
        let v: serde_json::Value = serde_json::from_str(&json).expect("JSON");
        let ct = &v["create_time"];
        if let Some(n) = ct.as_i64() {
            let through_js = n as f64 as i64;
            if through_js != n {
                println!(
                    "CHYBA: {} ({}) chodí jako JSON číslo; JavaScript z {n} udělá {through_js}",
                    p.name, p.pid
                );
                fails += 1;
                continue;
            }
        }
        let back: core_types::proc::ProcRow = serde_json::from_str(&json).expect("deserializace");
        if back.create_time != p.create_time {
            println!(
                "CHYBA: {} ({}) čas vzniku {} → {}",
                p.name, p.pid, p.create_time, back.create_time
            );
            fails += 1;
        }
    }
    println!("  procesů {}, z toho nad hranicí přesnosti JS: {big}", procs.len());
    if big == 0 {
        println!("  POZOR: ani jeden čas vzniku není velký — brána nic neověřila");
    }

    println!("\nBRÁNA jsoncheck: {}", if fails == 0 { "PASS" } else { "FAIL" });
    if fails > 0 {
        std::process::exit(1);
    }
}
