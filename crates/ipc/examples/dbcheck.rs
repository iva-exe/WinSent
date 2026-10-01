//! Brána: umístění databáze jde přečíst i změnit — a nic se přitom
//! neztratí. `cargo run -p ipc --example dbcheck`
//!
//! Databáze roste do stovek megabajtů a na stroji s malým nebo
//! opotřebovaným systémovým SSD ji jde odsunout jinam. Přesun sám dělá
//! až START služby, kdy ji nikdo nedrží otevřenou — stěhovat ji za běhu
//! by znamenalo přijít o rozepsaný WAL s posledními vzorky.
//!
//! Brána nechává výchozí umístění na pokoji: nastaví přání na dočasnou
//! složku, ověří, že ho služba přijala a hlásí čekající přesun, a hned
//! ho zase vrátí na výchozí. Žádný soubor se nepřesouvá.

fn main() {
    let mut fails = 0;

    let start = match ipc::client::query_db_location() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("query_db_location selhal: {e}");
            std::process::exit(1);
        }
    };
    println!("  leží v:        {}", start.current_path);
    println!("  přání:         {:?}", start.wanted_dir);
    println!("  výchozí:       {}", start.default_dir);
    println!(
        "  velikost:      {:.1} MB, volno {:.1} GB",
        start.bytes as f64 / 1e6,
        start.free_bytes as f64 / 1e9
    );

    if start.current_path.is_empty() || start.default_dir.is_empty() {
        println!("CHYBA: cesta k databázi je prázdná");
        fails += 1;
    }
    if start.bytes == 0 {
        println!("CHYBA: databáze má nulovou velikost");
        fails += 1;
    }
    // Volné místo se čte z toho svazku, kde databáze opravdu leží.
    if start.free_bytes == 0 {
        println!("CHYBA: volné místo se nezjistilo");
        fails += 1;
    }

    // Nesmysl musí služba odmítnout, ne uložit. Přání, které by při
    // startu selhalo, by uživatele připravilo o sběr dat a on by se to
    // dozvěděl až z logu.
    let nesmysl = r"Z:\rozhodne\neexistujici\svazek";
    match ipc::client::set_db_dir(nesmysl.to_string()) {
        Err(e) => println!("  odmítnuto správně: {e}"),
        Ok(()) => {
            println!("CHYBA: {nesmysl} nebyl odmítnut");
            fails += 1;
            let _ = ipc::client::set_db_dir(String::new());
        }
    }

    // Přesun databáze smí jen elevovaný správce: služba běží jako SYSTEM
    // a cesta od klienta by jí jinak dovolila zakládat a přepisovat
    // soubory, kam běžný uživatel nesmí. Brána běžně běží neelevovaně —
    // pak se ověřuje, že služba přesun ODMÍTNE a nic nezmění.
    if !proces_je_elevovany_spravce() {
        let docasna = std::env::temp_dir().join("winsent-dbcheck");
        match ipc::client::set_db_dir(docasna.display().to_string()) {
            Err(e) if e.to_string().contains("správce") => {
                println!("  neelevovanému klientovi odmítnuto správně: {e}")
            }
            Err(e) => {
                println!("CHYBA: odmítnuto, ale z jiného důvodu: {e}");
                fails += 1;
            }
            Ok(()) => {
                println!("CHYBA: služba přijala přesun databáze od neelevovaného klienta");
                fails += 1;
                // Hned vrátit — jinak by se databáze při příštím startu
                // služby přestěhovala do dočasné složky (stará služba
                // přání přijímá od kohokoli).
                if let Err(e) = ipc::client::set_db_dir(start.wanted_dir.clone()) {
                    println!("CHYBA: návrat přání selhal: {e}");
                }
            }
        }
        match ipc::client::query_db_location() {
            Ok(k) if k.wanted_dir == start.wanted_dir => println!("  nastavení beze změny"),
            Ok(k) => {
                println!("CHYBA: přání se změnilo na {:?}", k.wanted_dir);
                fails += 1;
            }
            Err(e) => {
                println!("CHYBA: kontrolní dotaz selhal: {e}");
                fails += 1;
            }
        }
        println!("\nBRÁNA dbcheck: {}", if fails == 0 { "PASS" } else { "FAIL" });
        if fails > 0 {
            std::process::exit(1);
        }
        return;
    }

    // Platná složka se musí přijmout a ohlásit jako čekající přesun.
    let docasna = std::env::temp_dir().join("winsent-dbcheck");
    let cesta = docasna.display().to_string();
    match ipc::client::set_db_dir(cesta.clone()) {
        Ok(()) => match ipc::client::query_db_location() {
            Ok(po) => {
                if po.wanted_dir != cesta {
                    println!("CHYBA: přání se neuložilo ({:?})", po.wanted_dir);
                    fails += 1;
                }
                if !po.pending {
                    println!("CHYBA: čekající přesun se nehlásí");
                    fails += 1;
                }
                // Databáze se NESMÍ hnout dřív, než služba znovu nastartuje.
                if po.current_path != start.current_path {
                    println!("CHYBA: databáze se přesunula za běhu ({})", po.current_path);
                    fails += 1;
                }
            }
            Err(e) => {
                println!("CHYBA: druhý dotaz selhal: {e}");
                fails += 1;
            }
        },
        Err(e) => {
            println!("CHYBA: platnou složku služba nepřijala: {e}");
            fails += 1;
        }
    }

    // Uklidit po sobě: vrátit původní stav, ať brána stroj nemění.
    if let Err(e) = ipc::client::set_db_dir(start.wanted_dir.clone()) {
        println!("CHYBA: návrat na původní nastavení selhal: {e}");
        fails += 1;
    }
    let _ = std::fs::remove_dir(&docasna);
    match ipc::client::query_db_location() {
        Ok(k) if k.wanted_dir == start.wanted_dir => println!("  uklizeno zpět"),
        Ok(k) => {
            println!("CHYBA: zůstalo {:?} místo {:?}", k.wanted_dir, start.wanted_dir);
            fails += 1;
        }
        Err(e) => {
            println!("CHYBA: kontrolní dotaz selhal: {e}");
            fails += 1;
        }
    }

    println!("\nBRÁNA dbcheck: {}", if fails == 0 { "PASS" } else { "FAIL" });
    if fails > 0 {
        std::process::exit(1);
    }
}

/// Členství ve správcích pro tento proces (TRUE jen pro elevovaný token).
fn proces_je_elevovany_spravce() -> bool {
    use windows::Win32::Security::{
        CheckTokenMembership, CreateWellKnownSid, WinBuiltinAdministratorsSid, PSID,
    };
    let mut sid = [0u8; 68];
    let mut len = sid.len() as u32;
    let mut member = windows::core::BOOL(0);
    // SAFETY: buffery vlastníme; None = token vlákna (zde primární).
    unsafe {
        if CreateWellKnownSid(WinBuiltinAdministratorsSid, None, Some(PSID(sid.as_mut_ptr() as _)), &mut len).is_err() {
            return false;
        }
        if CheckTokenMembership(None, PSID(sid.as_mut_ptr() as _), &mut member).is_err() {
            return false;
        }
    }
    member.as_bool()
}
