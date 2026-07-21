//! Ruční test inventáře přes IPC: `cargo run -p ipc --example appcheck`.
//! Vypíše počet aplikací a mapu první „bohaté" aplikace vč. velikostí.

fn main() {
    let apps = match ipc::client::query_apps() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("query_apps selhal: {e}");
            std::process::exit(1);
        }
    };
    println!("aplikací v inventáři: {}", apps.len());
    let Some(rich) = apps.iter().max_by_key(|a| a.path_count) else {
        println!("(inventář je prázdný — sken možná ještě běží)");
        return;
    };
    println!(
        "nejbohatší: {} [{}] — {} cest",
        rich.display_name,
        rich.publisher.as_deref().unwrap_or("—"),
        rich.path_count
    );
    // Chrome jako testovací případ brány v4.
    let probe = apps
        .iter()
        .find(|a| a.display_name.to_lowercase().contains("chrome"))
        .unwrap_or(rich);
    match ipc::client::compute_app_sizes(probe.identity_key.clone()) {
        Ok(map) => {
            println!("\n{} — mapa s velikostmi:", probe.display_name);
            for p in map {
                let size = p
                    .size_bytes
                    .map(|b| format!("{:.1} MB", b as f64 / 1e6))
                    .unwrap_or_else(|| "—".into());
                println!(
                    "  [{:9}/{:5}] {:8} {:>10}  {}",
                    p.source, p.confidence, p.role, size, p.path
                );
            }
        }
        Err(e) => eprintln!("compute_app_sizes selhal: {e}"),
    }
}
