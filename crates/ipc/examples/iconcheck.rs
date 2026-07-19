//! Rychlý test ikon: `cargo run -p ipc --example iconcheck`.
//! Vezme pár procesů, zjistí jejich identity_key a zkusí ikonu.
fn main() {
    let procs = ipc::client::query_procs().expect("query_procs");
    let mut seen = std::collections::HashSet::new();
    let mut ok = 0;
    let mut tried = 0;
    for p in procs.iter() {
        if !seen.insert(p.identity_key.clone()) {
            continue;
        }
        // Dej workeru chvíli na doběhnutí u prvních klíčů.
        std::thread::sleep(std::time::Duration::from_millis(60));
        match ipc::client::query_icon(p.identity_key.clone()) {
            Ok(Some(ic)) => {
                ok += 1;
                if ok <= 8 {
                    println!("IKONA {}x{}  {}  ({})", ic.w, ic.h, p.app_name, p.identity_key);
                }
            }
            Ok(None) => {}
            Err(e) => { eprintln!("chyba: {e}"); return; }
        }
        tried += 1;
        if tried >= 40 { break; }
    }
    println!("--- ikon získáno: {ok} z {tried} unikátních klíčů ---");
}
