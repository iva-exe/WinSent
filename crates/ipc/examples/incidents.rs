//! Ruční test v3: `cargo run -p ipc --example incidents`.
//! Vytiskne události za poslední hodinu a poslední incidenty.

fn main() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    match ipc::client::query_events(now - 3600, now) {
        Ok(events) => {
            println!("události za hodinu: {}", events.len());
            for e in events.iter().rev().take(5) {
                println!(
                    "  [{}] {} pid={:?} {}",
                    e.ts,
                    e.kind,
                    e.pid,
                    e.detail.as_deref().unwrap_or("")
                );
            }
        }
        Err(e) => {
            eprintln!("query_events selhal: {e}");
            std::process::exit(1);
        }
    }

    match ipc::client::query_incidents(10) {
        Ok(incidents) => {
            println!("incidentů: {}", incidents.len());
            for i in &incidents {
                println!(
                    "  [{}] {} viník={:?} okno={:?}..{:?}",
                    i.ts, i.kind, i.culprit, i.window_from, i.window_to
                );
            }
        }
        Err(e) => {
            eprintln!("query_incidents selhal: {e}");
            std::process::exit(1);
        }
    }
}
