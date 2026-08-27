//! Ruční sonda: přesun databáze jinam a zpátky.
//!
//!   cargo run -p ipc --example dbmove -- D:\WinsentData
//!   cargo run -p ipc --example dbmove --            (zpět na výchozí)
//!
//! Nastaví přání a vypíše stav. Vlastní stěhování udělá až START
//! služby — tahle sonda ho tedy jen připraví a ukáže, na co se čeká.

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_default();
    if let Err(e) = ipc::client::set_db_dir(dir.clone()) {
        eprintln!("nastavení selhalo: {e}");
        std::process::exit(1);
    }
    match ipc::client::query_db_location() {
        Ok(l) => {
            println!("leží v:  {}", l.current_path);
            println!("přání:   {:?}", l.wanted_dir);
            println!("výchozí: {}", l.default_dir);
            println!("velikost: {:.1} MB", l.bytes as f64 / 1e6);
            println!(
                "{}",
                if l.pending {
                    "čeká na restart služby"
                } else {
                    "na místě, není co stěhovat"
                }
            );
        }
        Err(e) => eprintln!("dotaz selhal: {e}"),
    }
}
