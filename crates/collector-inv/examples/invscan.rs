//! Ruční test inventáře: `cargo run -p collector-inv --example invscan`.
//! (MSIX výčet všech uživatelů vyžaduje elevaci; bez ní bude prázdný.)

fn main() {
    let t0 = std::time::Instant::now();
    let apps = collector_inv::scan();
    let ms = t0.elapsed().as_millis();
    let with_exact = apps
        .iter()
        .filter(|a| a.paths.iter().any(|p| p.confidence == "exact"))
        .count();
    let with_guess = apps
        .iter()
        .filter(|a| a.paths.iter().any(|p| p.confidence == "guess"))
        .count();
    let msix = apps.iter().filter(|a| a.kind == "msix").count();
    println!(
        "aplikací: {} ({} msix) za {} ms; s exact cestami: {}, s guess: {}",
        apps.len(),
        msix,
        ms,
        with_exact,
        with_guess
    );
    // Ukázka: první aplikace s bohatou mapou (např. Chrome-like).
    if let Some(app) = apps.iter().max_by_key(|a| a.paths.len()) {
        println!(
            "\nnejbohatší mapa: {} ({} cest) [{}]",
            app.display_name,
            app.paths.len(),
            app.publisher.as_deref().unwrap_or("—")
        );
        for p in app.paths.iter().take(12) {
            println!(
                "  [{:9}/{:5}] {:7} {}",
                p.source, p.confidence, p.role, p.path
            );
        }
    }
    for name in ["chrome", "steam", "discord"] {
        if let Some(app) = apps
            .iter()
            .find(|a| a.display_name.to_lowercase().contains(name))
        {
            println!("\n{} — {} cest:", app.display_name, app.paths.len());
            for p in &app.paths {
                println!(
                    "  [{:9}/{:5}] {:7} {}",
                    p.source, p.confidence, p.role, p.path
                );
            }
        }
    }
}
