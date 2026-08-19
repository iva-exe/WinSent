//! Ověření, že služba najde k pádu výpisy a určí viníka.
//! `cargo run -p ipc --example dumpcheck`
fn main() {
    let rows = match ipc::client::query_crash_reports(20) {
        Ok(r) => r,
        Err(e) => { println!("!! {e}"); std::process::exit(1); }
    };
    let mut nasel = 0;
    for r in rows.iter().take(6) {
        let t = ipc::client::query_incident_dumps(r.app.clone(), r.ts, String::new())
            .unwrap_or_else(|e| format!("chyba: {e}"));
        let vinik = t.lines().find(|l| l.contains("VINÍK:"));
        let zdroju = t.matches("Soubor:").count();
        if zdroju > 0 { nasel += 1; }
        println!("{:<26} zdrojů {zdroju:>2}  {}", r.app, vinik.unwrap_or("(bez viníka z dumpu)").trim());
    }
    println!("\npádů s nalezenými podklady: {nasel} z 6");
}
