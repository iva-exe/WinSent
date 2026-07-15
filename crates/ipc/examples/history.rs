//! Ruční test historie: `cargo run -p ipc --example history`.
//! Vytiskne počet bodů za poslední minutu a stav procesů před ~10 s.

fn main() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    match ipc::client::query_system_history(now - 60, now) {
        Ok(points) => {
            let last = points.last();
            println!(
                "historie: {} bodů za 60 s; poslední: {:?}",
                points.len(),
                last.map(|p| (p.ts, p.cpu_pct, p.net_rx_bps))
            );
        }
        Err(e) => {
            eprintln!("query_system_history selhal: {e}");
            std::process::exit(1);
        }
    }

    match ipc::client::query_procs_at(now - 10) {
        Ok((ts, rows)) => {
            println!(
                "procs_at({}): {} řádků, ts vzorku {}",
                now - 10,
                rows.len(),
                ts
            );
            for r in rows.iter().take(3) {
                println!("  {:>7}  {:5.1} %  {}", r.pid, r.cpu_pct, r.name);
            }
        }
        Err(e) => {
            eprintln!("query_procs_at selhal: {e}");
            std::process::exit(1);
        }
    }
}
