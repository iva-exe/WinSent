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
    println!(
        "disky: {:?}  takt {}/{} MHz  uptime {} s  vláken {}  handlů {}",
        sys.disks,
        sys.cpu_clock_mhz,
        sys.cpu_clock_max_mhz,
        sys.uptime_s,
        sys.threads_total,
        sys.handles_total
    );
    match ipc::client::query_sys_info() {
        Ok(info) => println!(
            "sysinfo: {} | base {} MHz | {}C/{}T | L1 {} L2 {} L3 {} kB | RAM {} modulů/{} slotů {:?} | GPU {:?} | disky {:?}",
            info.cpu_name,
            info.cpu_base_mhz,
            info.physical_cores,
            info.logical_cores,
            info.l1_kb,
            info.l2_kb,
            info.l3_kb,
            info.ram_modules.len(),
            info.ram_slots,
            info.ram_modules.iter().map(|m| (m.size_mb, m.configured_mts, m.slot.clone())).collect::<Vec<_>>(),
            info.gpu_name,
            info.disks
        ),
        Err(e) => eprintln!("query_sys_info selhal: {e}"),
    }

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
            "{:>7}  {:5.1} %  {:>9.1} MB  [{:?}/{:?}] app={} pub={:?}  ({})",
            p.pid,
            p.cpu_pct,
            p.ws_bytes as f64 / 1048576.0,
            p.protection,
            p.confidence,
            p.app_name,
            p.publisher,
            p.name
        );
    }
    // Top 8 podle GPU % (per-proces, PDH GPU Engine).
    let mut by_gpu = procs.clone();
    by_gpu.sort_by(|a, b| b.gpu_pct.total_cmp(&a.gpu_pct));
    println!("--- GPU per proces (top 8) ---");
    for p in by_gpu.iter().take(8) {
        println!("  GPU {:5.1} %  {:>7}  {}", p.gpu_pct, p.pid, p.name);
    }
    // Seskupení podle aplikace (jako v UI stromu).
    use std::collections::BTreeMap;
    let mut by_app: BTreeMap<String, (usize, f32)> = BTreeMap::new();
    for p in &procs {
        let e = by_app.entry(p.app_name.clone()).or_default();
        e.0 += 1;
        e.1 += p.cpu_pct;
    }
    println!("\n--- seskupení aplikace → procesy ---");
    let mut groups: Vec<_> = by_app.into_iter().collect();
    groups.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
    for (app, (n, cpu)) in groups.iter().take(12) {
        println!("  {n:>3}×  {cpu:5.1} %  {app}");
    }
}
