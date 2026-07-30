//! Co o tomhle stroji přečteme: `cargo run -p win-sys --example hwcheck`.

fn main() {
    win_sys::wic::init_com_for_thread();
    let n = std::thread::available_parallelism().map_or(1, |n| n.get());

    let cpu = win_sys::cpuinfo::cpu_static();
    println!("CPU        {cpu:?}");

    let t = win_sys::thermal::cpu_thermal(n);
    match t.celsius {
        Some(c) => println!("teplota    {c:.0} °C (zdroj: {})", t.source.as_str()),
        None => println!("teplota    nedostupná — zdroj: {}", t.source.as_str()),
    }
    println!(
        "takt       {} / {} MHz, throttling: {}",
        t.clock_mhz,
        t.max_mhz,
        if t.throttling() { "ano" } else { "ne" }
    );

    let b = win_sys::smbios::board();
    println!(
        "deska      {} {} ({})",
        b.manufacturer, b.product, b.version
    );
    println!(
        "BIOS       {} {} z {}",
        b.bios_vendor, b.bios_version, b.bios_date
    );
    println!("stroj      {} {}", b.system_manufacturer, b.system_product);

    let (mods, slots) = win_sys::smbios::ram_modules();
    println!("RAM        {} modulů v {slots} slotech", mods.len());
    for m in &mods {
        println!(
            "           {} — {} MB @ {} MT/s (modul umí {}) ({} {})",
            m.slot, m.size_mb, m.configured_mts, m.speed_mts, m.manufacturer, m.part_number
        );
    }

    match win_sys::battery::battery() {
        Some(b) => println!(
            "baterie    {:?} %, síť: {}, opotřebení: {:?}, cyklů: {:?}",
            b.percent,
            b.ac_online,
            b.wear_pct().map(|w| format!("{w:.1} %")),
            b.cycles
        ),
        None => println!("baterie    žádná (desktop)"),
    }

    for v in win_sys::volumes::volumes().iter().filter(|v| v.fixed) {
        println!(
            "svazek     {}: {} — {:.1} / {:.1} GB volných",
            v.letter,
            v.label,
            v.free_bytes as f64 / 1e9,
            v.total_bytes as f64 / 1e9
        );
    }
}
