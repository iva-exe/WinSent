//! Výpis klasifikace startovacích položek (ladicí, ne brána).
fn main() {
    let items = ipc::client::query_startup().expect("query_startup");
    let want: Vec<String> = std::env::args().skip(1).map(|a| a.to_lowercase()).collect();
    for i in &items {
        let hay = format!("{} {} {}", i.id, i.name, i.command).to_lowercase();
        if !want.is_empty() && !want.iter().any(|w| hay.contains(w)) {
            continue;
        }
        println!(
            "{:<9} {:<13} {:<48} {}",
            if i.system { "SYSTEM" } else { "třetí" },
            i.source,
            i.name.chars().take(46).collect::<String>(),
            i.system_reason.clone().unwrap_or_default()
        );
    }
}
