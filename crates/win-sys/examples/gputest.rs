//! Ruční test vendor-neutrální GPU cesty (PDH + registry + D3DKMT):
//! `cargo run -p win-sys --example gputest`. Bez služby, bez NVML —
//! přesně to, co uvidí stroj s AMD/Intel GPU. Senzory D3DKMT se dají
//! porovnat s `nvidia-smi --query-gpu=temperature.gpu,fan.speed,
//! power.draw,power.limit --format=csv`.

fn main() {
    let basic = win_sys::gpubasic::detect();
    println!(
        "registry: name={:?} vram_total={:?} MB",
        basic.name, basic.vram_total_mb
    );

    // Bez NVML (None) — výběr podle největší dedikované VRAM.
    let kmt = win_sys::gpukmt::KmtAdapter::open_main(None);
    match &kmt {
        Some(k) => println!(
            "d3dkmt: {} dedicated={} MB shared_limit={} MB",
            k.pdh_luid, k.dedicated_mb, k.shared_limit_mb
        ),
        None => println!("d3dkmt: žádný vhodný adaptér"),
    }

    let Some(mut pdh) = win_sys::gpuproc::GpuPerProc::init() else {
        println!("PDH GPU Engine counter není k dispozici");
        return;
    };
    pdh.set_main_luid(kmt.as_ref().map(|k| k.pdh_luid.clone()));
    for i in 0..3 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let s = pdh.sample();
        let mut top: Vec<(u32, f32)> = s.per_pid.iter().map(|(&p, &v)| (p, v)).collect();
        top.sort_by(|a, b| b.1.total_cmp(&a.1));
        top.truncate(3);
        println!(
            "vzorek {}: total={:?} % vram_used={:?} MB top_pid={:?}",
            i, s.total_pct, s.vram_used_mb, top
        );
        if let Some(k) = &kmt {
            let t0 = std::time::Instant::now();
            let perf = k.sample();
            println!("  senzory ({} µs): {:?}", t0.elapsed().as_micros(), perf);
        }
        println!("  enginy: {:?}", s.engines);
        println!(
            "  sdílená={:?} MB  vram podle procesů (pid, MB): {:?}",
            s.shared_used_mb,
            &s.mem_by_pid[..s.mem_by_pid.len().min(5)]
        );
    }
}
