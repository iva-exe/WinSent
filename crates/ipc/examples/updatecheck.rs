//! Brána: cesta k aktualizaci je průchozí.
//! `cargo run -p ipc --example updatecheck`
//!
//! Ověřuje síťovou část, ne samotnou aktualizaci: že jde zjistit
//! commit, stáhnout `version.txt` i `WinsentSetup.exe`, a že stažený
//! instalátor je opravdu program (začíná „MZ"), ne chybová stránka.
//! Nic se nespouští ani neinstaluje.

const RAW_HOST: &str = "raw.githubusercontent.com";
const API_HOST: &str = "api.github.com";
const REPO: &str = "iva-exe/WinSent";

fn main() {
    let mut fails = 0;

    let sha = match win_sys::http::get(API_HOST, &format!("/repos/{REPO}/commits/main"), |_| {}) {
        Ok(b) => {
            let t = String::from_utf8_lossy(&b).to_string();
            match t.find("\"sha\":\"") {
                Some(p) => t[p + 7..]
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .collect::<String>(),
                None => {
                    println!("CHYBA: odpověď GitHubu neobsahuje commit");
                    fails += 1;
                    String::new()
                }
            }
        }
        Err(e) => {
            println!("CHYBA: commit se nepodařilo zjistit: {e}");
            fails += 1;
            String::new()
        }
    };

    if sha.len() >= 7 {
        println!("commit: {}", &sha[..7]);
        match win_sys::http::get(
            RAW_HOST,
            &format!("/{REPO}/{sha}/release/version.txt"),
            |_| {},
        ) {
            Ok(b) => {
                let v = String::from_utf8_lossy(&b).trim().to_string();
                if v.is_empty() {
                    println!("CHYBA: version.txt je prázdný");
                    fails += 1;
                } else {
                    println!("verze v repu: {v}");
                }
            }
            Err(e) => {
                println!("CHYBA: version.txt: {e}");
                fails += 1;
            }
        }
        match win_sys::http::get(
            RAW_HOST,
            &format!("/{REPO}/{sha}/release/WinsentSetup.exe"),
            |_| {},
        ) {
            Ok(b) if b.len() >= 100_000 && &b[..2] == b"MZ" => {
                println!("instalátor: {:.1} MB, začíná MZ", b.len() as f64 / 1e6);
            }
            Ok(b) => {
                println!("CHYBA: instalátor je poškozený ({} B)", b.len());
                fails += 1;
            }
            Err(e) => {
                println!("CHYBA: instalátor: {e}");
                fails += 1;
            }
        }
    }

    // Nainstalovaná verze — bez ní se aktualizace nenabízí.
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    match std::fs::read_to_string(std::path::Path::new(&pf).join(r"Winsent\version.txt")) {
        Ok(v) => println!("nainstalováno: {}", v.trim()),
        Err(_) => println!("(běží z vývojového stromu — aktualizace se nenabízí)"),
    }

    println!(
        "\nBRÁNA updatecheck: {}",
        if fails == 0 { "PASS" } else { "FAIL" }
    );
    if fails > 0 {
        std::process::exit(1);
    }
}
