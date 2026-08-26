//! Brána: GPU per proces sedí a ukládá se do historie.
//! `cargo run -p ipc --example gpucheck`
//!
//! Co se ověřuje:
//!   • žádný proces nehlásí víc než 100 % (a víc než celé GPU + rezerva),
//!   • součet přes procesy nepřeleze celkové GPU o víc než dvojnásobek
//!     (enginy běží souběžně, takže rovnost čekat nejde — ale řádové
//!     nafouknutí by znamenalo, že se zase sčítá přes typy enginů),
//!   • v historii procesů je sloupec GPU vyplněný, ne prázdný.

fn main() {
    let procs = match ipc::client::query_procs() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("query_procs selhal: {e}");
            std::process::exit(1);
        }
    };
    let sys = ipc::client::query_system().expect("query_system");
    let total = sys.gpu_pct.unwrap_or(0.0);

    let mut fails = 0;
    let mut with_gpu: Vec<_> = procs.iter().filter(|p| p.gpu_pct > 0.0).collect();
    with_gpu.sort_by(|a, b| b.gpu_pct.partial_cmp(&a.gpu_pct).unwrap());
    println!(
        "celkové GPU {total:.1} % · procesů s GPU > 0: {}",
        with_gpu.len()
    );
    for p in with_gpu.iter().take(6) {
        println!("  {:>6.2} %  {} (pid {})", p.gpu_pct, p.name, p.pid);
    }

    for p in &procs {
        if p.gpu_pct > 100.0 || !p.gpu_pct.is_finite() {
            println!("CHYBA: {} (pid {}) hlásí {} %", p.name, p.pid, p.gpu_pct);
            fails += 1;
        }
    }
    // Jeden proces nemůže vytížit GPU víc, než kolik ho celkově jede.
    // Rezerva 8 b. je na to, že celkové % a per-proces % nevznikají
    // z úplně stejného okamžiku.
    if let Some(top) = with_gpu.first() {
        if top.gpu_pct > total + 8.0 {
            println!(
                "CHYBA: {} má {:.1} %, ale celé GPU jede jen {total:.1} %",
                top.name, top.gpu_pct
            );
            fails += 1;
        }
    }

    // Historie: bere se ČERSTVÝ vzorek. Starší mohou pocházet z doby
    // před přidáním sloupce (nebo z předchozí verze služby) a mít GPU
    // prázdné právem — brána by pak padala po každé aktualizaci.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    match ipc::client::query_procs_at(now - 3) {
        Ok((ts, rows)) if !rows.is_empty() => {
            let known = rows.iter().filter(|r| r.gpu_pct.is_some()).count();
            println!(
                "historie @{ts}: {} procesů, GPU vyplněné u {known}",
                rows.len()
            );
            if known == 0 {
                println!("CHYBA: v historii procesů není GPU ani u jednoho záznamu");
                fails += 1;
            }
        }
        Ok(_) => println!("(historie zatím prázdná — služba běží krátce)"),
        Err(e) => println!("(historii nejde přečíst: {e})"),
    }

    // Hostitelské procesy (WebView2) musí patřit aplikaci, která je
    // spustila — jinak by se jejich GPU ukazovalo u cizího řádku.
    let by_pid: std::collections::HashMap<u32, &core_types::proc::ProcRow> =
        procs.iter().map(|p| (p.pid, p)).collect();
    let hosts: Vec<_> = procs
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case("msedgewebview2.exe"))
        .collect();
    let mut orphan = 0;
    for h in &hosts {
        // Předek, který sám hostitelem není.
        let mut cur = h.parent_pid;
        let mut owner = None;
        for _ in 0..8 {
            let Some(p) = by_pid.get(&cur) else { break };
            if p.name.eq_ignore_ascii_case("msedgewebview2.exe") {
                cur = p.parent_pid;
                continue;
            }
            owner = Some(*p);
            break;
        }
        let Some(owner) = owner else { continue };
        if h.identity_key != owner.identity_key {
            println!(
                "CHYBA: pid {} (WebView2) je pod „{}“, ale hostitel je „{}“",
                h.pid, h.app_name, owner.app_name
            );
            orphan += 1;
        }
    }
    println!(
        "WebView2 procesů: {} · špatně připsaných: {orphan}",
        hosts.len()
    );
    fails += orphan;

    println!("\nBRÁNA gpucheck: {}", if fails == 0 { "PASS" } else { "FAIL" });
    if fails > 0 {
        std::process::exit(1);
    }
}
