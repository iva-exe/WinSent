//! Ruční test vendor-neutrální GPU cesty (PDH + registry):
//! `cargo run -p win-sys --example gputest`. Bez služby, bez NVML —
//! přesně to, co uvidí stroj s AMD/Intel GPU.

fn main() {
    let basic = win_sys::gpubasic::detect();
    println!(
        "registry: name={:?} vram_total={:?} MB",
        basic.name, basic.vram_total_mb
    );

    let Some(mut pdh) = win_sys::gpuproc::GpuPerProc::init() else {
        println!("PDH GPU Engine counter není k dispozici");
        return;
    };
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
    }
}
