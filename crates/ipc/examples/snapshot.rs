//! Ruční test sampleru: `cargo run -p ipc --example snapshot`.
//! Vytiskne systémové metriky a top 10 procesů dle CPU.

fn main() {
    let sys = match ipc::client::query_system() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("query_system selhal: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "CPU {:.1} %  RAM {}/{} MB  GPU {:?}  ↓{} B/s ↑{} B/s  procesů {}",
        sys.cpu_pct,
        sys.mem_used_mb,
        sys.mem_total_mb,
        sys.gpu_pct,
        sys.net_rx_bps,
        sys.net_tx_bps,
        sys.proc_count
    );
    println!(
        "jader: {} (C0 {:.0} %)  gpu detail: {:?}",
        sys.cores.len(),
        sys.cores.first().copied().unwrap_or(0.0),
        sys.gpu
    );

    let mut procs = match ipc::client::query_procs() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("query_procs selhal: {e}");
            std::process::exit(1);
        }
    };
    procs.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
    for p in procs.iter().take(10) {
        println!(
            "{:>7}  {:5.1} %  {:>9.1} MB  {}",
            p.pid,
            p.cpu_pct,
            p.ws_bytes as f64 / 1048576.0,
            p.name
        );
    }
}
