//! Brána: paměť aplikace musí sedět se Správcem úloh.
//! `cargo run -p ipc --example memcheck`
//!
//! Hlídá se jediná věc, kterou uživatel opravdu porovnává: číslo
//! u aplikace ve sloupci „Paměť". Sčítat celou pracovní sadu přes
//! procesy jedné aplikace znamená počítat sdílené stránky (systémové
//! DLL, sdílená paměť) u každého procesu znovu — u prohlížeče nebo hry
//! s deseti procesy z toho vyleze klidně dvojnásobek. Proto se sbírá
//! soukromá pracovní sada, přesně to pole, které sčítá Správce úloh.
//!
//! Kontrola stojí na tvrdém faktu: soukromé pracovní sady se nepřekrývají,
//! takže jejich součet přes VŠECHNY procesy se nemůže vejít nad fyzickou
//! paměť. Celá pracovní sada tuhle mez běžně překročí — a právě tím se
//! stará chyba pozná.

fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

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

    let rows = match ipc::client::query_procs() {
        Ok(r) => r,
        Err(e) => {
            println!("!!  dotaz na procesy selhal: {e}");
            std::process::exit(1);
        }
    };
    let sys = match ipc::client::query_system() {
        Ok(s) => s,
        Err(e) => {
            println!("!!  dotaz na systém selhal: {e}");
            std::process::exit(1);
        }
    };
    let ram = (sys.mem_total_mb as u64) * 1024 * 1024;
    println!("    procesů {}, fyzická paměť {:.1} GB", rows.len(), gb(ram));

    // 1) Součet přes všechny procesy se musí vejít do fyzické paměti.
    let total: u64 = rows.iter().map(|r| r.ws_bytes).sum();
    if total <= ram {
        println!(
            "OK  součet paměti procesů {:.2} GB ≤ {:.2} GB fyzické",
            gb(total),
            gb(ram)
        );
    } else {
        println!(
            "!!  součet paměti procesů {:.2} GB > {:.2} GB fyzické — sčítá se sdílená paměť",
            gb(total),
            gb(ram)
        );
        fail += 1;
    }

    // 2) Součet za aplikaci: žádná aplikace nesmí sama „sníst" víc,
    //    než kolik má stroj paměti.
    let mut apps: std::collections::HashMap<&str, (u64, u32, &str)> =
        std::collections::HashMap::new();
    for r in &rows {
        let e = apps
            .entry(&r.identity_key)
            .or_insert((0, 0, r.app_name.as_str()));
        e.0 += r.ws_bytes;
        e.1 += 1;
    }
    let mut top: Vec<_> = apps.values().collect();
    top.sort_by(|a, b| b.0.cmp(&a.0));

    let worst = top.first().map(|t| t.0).unwrap_or(0);
    if worst <= ram {
        println!("OK  největší aplikace {:.2} GB ≤ fyzická paměť", gb(worst));
    } else {
        println!("!!  největší aplikace {:.2} GB > fyzická paměť", gb(worst));
        fail += 1;
    }

    // 3) Nenulovost — kdyby se sbíralo špatné pole, byly by tu samé nuly.
    let zero = rows.iter().filter(|r| r.ws_bytes == 0).count();
    if zero * 4 <= rows.len() {
        println!("OK  procesů s nulovou pamětí {zero} z {}", rows.len());
    } else {
        println!(
            "!!  {zero} z {} procesů hlásí nulovou paměť — špatné pole?",
            rows.len()
        );
        fail += 1;
    }

    // 4) Jádro věci: hlásí služba SOUKROMOU pracovní sadu?
    //
    // Gate si vezme vlastní snapshot a porovná ho proti tomu, co přišlo
    // po IPC. Soukromá sada je vždy podmnožinou celé, takže žádný proces
    // nesmí hlásit víc, než je jeho celá pracovní sada — a napříč
    // systémem musí být součet znatelně nižší (sdílené DLL). Kdyby se
    // do UI zase dostala celá sada, oba testy padnou.
    let mut buf = Vec::new();
    match win_sys::proc::snapshot_processes(&mut buf) {
        Ok(raw) => {
            let by_pid: std::collections::HashMap<u32, &win_sys::proc::RawProc> =
                raw.iter().map(|p| (p.pid, p)).collect();
            let mut over = 0;
            let (mut sum_reported, mut sum_full) = (0u64, 0u64);
            for r in &rows {
                let Some(p) = by_pid.get(&r.pid) else { continue };
                // Vzorky dělí zlomek sekundy, takže malý pohyb nahoru
                // je normální — hlídá se hrubý nepoměr, ne přesná rovnost.
                if r.ws_bytes > p.ws_bytes + p.ws_bytes / 10 + 4 * 1024 * 1024 {
                    over += 1;
                }
                sum_reported += r.ws_bytes;
                sum_full += p.ws_bytes;
            }
            // Oba snapshoty dělí zlomek sekundy a paměť se mezitím hýbe,
            // takže ojedinělý překročený proces je posun měření, ne chyba.
            // Záměna polí by se projevila napříč celým seznamem.
            let limit = (rows.len() / 50).max(1);
            if over <= limit {
                println!("OK  procesů nad svou celou pracovní sadou: {over} (mez {limit})");
            } else {
                println!("!!  {over} procesů hlásí víc, než je jejich celá pracovní sada");
                fail += 1;
            }
            let ratio = if sum_full > 0 {
                sum_reported as f64 / sum_full as f64
            } else {
                1.0
            };
            if ratio < 0.95 {
                println!(
                    "OK  hlásí se soukromá sada: {:.2} GB z {:.2} GB celkové ({:.0} % — zbytek je sdílené)",
                    gb(sum_reported),
                    gb(sum_full),
                    ratio * 100.0
                );
            } else {
                println!(
                    "!!  hlášená paměť je {:.0} % celé pracovní sady — sdílené stránky se počítají znovu",
                    ratio * 100.0
                );
                fail += 1;
            }
        }
        Err(e) => {
            println!("!!  vlastní snapshot procesů selhal: {e}");
            fail += 1;
        }
    }

    // Výpis pro ruční srovnání se Správcem úloh (karta Procesy,
    // sloupec „Paměť"). Čísla by měla sedět na desítky MB.
    println!("\n    10 největších aplikací — porovnej se Správcem úloh:");
    for (bytes, procs, name) in top.iter().take(10) {
        println!(
            "      {:>9.1} MB  {:>2} proc.  {}",
            *bytes as f64 / 1024.0 / 1024.0,
            procs,
            name
        );
    }

    println!();
    if fail == 0 {
        println!("BRÁNA memcheck: PASS");
    } else {
        println!("BRÁNA memcheck: FAIL ({fail} problémů)");
        std::process::exit(1);
    }
}
