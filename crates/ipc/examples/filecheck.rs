//! Ruční test v4C: `cargo run -p ipc --example filecheck`.
//! Svazky + SMART, stavba MFT indexu C: a hledání.

fn main() {
    match ipc::client::query_volumes() {
        Ok((vols, health)) => {
            for v in &vols {
                println!(
                    "{}: [{}] {:.1}/{:.1} GB volných",
                    v.letter,
                    v.fs,
                    v.free_bytes as f64 / 1e9,
                    v.total_bytes as f64 / 1e9
                );
            }
            for h in &health {
                println!(
                    "disk {}: {} — temp={:?}°C opotřebení={:?}% hodin={:?}",
                    h.index, h.model, h.temp_c, h.used_pct, h.power_on_hours
                );
            }
        }
        Err(e) => {
            eprintln!("query_volumes selhal: {e}");
            std::process::exit(1);
        }
    }

    let t0 = std::time::Instant::now();
    match ipc::client::build_file_index('C') {
        Ok(n) => println!("index C: {} záznamů za {} ms", n, t0.elapsed().as_millis()),
        Err(e) => {
            eprintln!("build_file_index selhal: {e}");
            std::process::exit(1);
        }
    }
    let t0 = std::time::Instant::now();
    match ipc::client::search_files('C', "syswatch".into(), 50) {
        Ok(hits) => {
            println!(
                "hledání syswatch: {} nálezů za {} ms",
                hits.len(),
                t0.elapsed().as_millis()
            );
            for h in hits.iter().take(5) {
                println!("  {} ({:?} B)", h.path, h.size_bytes);
            }
        }
        Err(e) => eprintln!("search_files selhal: {e}"),
    }
}
