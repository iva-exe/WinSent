//! WinsentSetup — jediný soubor, který dostane tester.
//!
//! Spuštění bez parametrů = nainstaluj nebo aktualizuj:
//!   1. stáhne z repozitáře verzi a obě binárky
//!   2. porovná s nainstalovanou verzí (stejná → nic nedělá)
//!   3. zastaví službu, přepíše soubory, zaregistruje a spustí ji
//!   4. udělá zástupce a záznam v Programech a funkcích
//!
//! `WinsentSetup.exe /uninstall` = odeber všechno.
//!
//! Manifest si vynutí práva správce, takže se tester nemusí trefovat
//! do „spustit jako správce" — Windows se zeptají samy.

mod http;
mod service;
mod shell;

use std::io::Write;
use std::path::PathBuf;

/// Odkud se berou binárky. Veřejný repozitář, takže tester
/// nepotřebuje účet ani přístup.
const RAW_HOST: &str = "raw.githubusercontent.com";
const API_HOST: &str = "api.github.com";
const REPO: &str = "iva-exe/WinSent";

const FILES: &[&str] = &["syswatch.exe", "syswatch-ui.exe"];

/// Zjistí commit, na kterém repozitář právě stojí.
///
/// Proč tahle oklika: `raw.githubusercontent.com` drží soubory
/// v CDN cache 5 minut a **ignoruje query parametry**, takže se
/// stará verze nedá „obejít" přidáním `?t=`. Horší než zpoždění je
/// ale míchání — cache může vydat starou `version.txt` k novým
/// binárkám a instalace by tiše skončila s nesouhlasnými soubory.
///
/// Adresa s konkrétním commitem je naproti tomu neměnná: nový commit
/// = jiná cesta = žádná stará cache. Všechny soubory se pak stahují
/// z jednoho a téhož commitu.
fn latest_commit() -> Result<String, String> {
    let body = http::get(API_HOST, &format!("/repos/{REPO}/commits/main"), |_| {})
        .map_err(|e| format!("{e}"))?;
    let text = String::from_utf8_lossy(&body);
    // Odpověď začíná {"sha":"…"} — první výskyt je commit, který
    // hledáme. Kvůli jednomu poli se JSON knihovna tahat nemusí.
    let pos = text
        .find("\"sha\":\"")
        .ok_or("odpověď GitHubu neobsahuje commit")?;
    let sha: String = text[pos + 7..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if sha.len() < 7 {
        return Err("GitHub vrátil neplatný commit".into());
    }
    Ok(sha)
}

fn main() {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .map(|a| a.to_ascii_lowercase())
        .collect();
    let has = |names: &[&str]| args.iter().any(|a| names.contains(&a.as_str()));

    let uninstall = has(&["/uninstall", "--uninstall", "/u"]);
    // Tichý režim nečeká na Enter — pro automatizované nasazení
    // a pro testy, které by se jinak zasekly na prázdném vstupu.
    let quiet = has(&["/quiet", "--quiet", "/q", "/s", "/silent"]);

    println!("  Winsent — instalace\n");
    let result = if uninstall {
        do_uninstall()
    } else {
        do_install()
    };

    let code = match result {
        Ok(msg) => {
            println!("\n  {msg}");
            0
        }
        Err(e) => {
            println!("\n  CHYBA: {e}");
            println!("\n  Nic se nezměnilo. Když chyba trvá, pošli tenhle výpis vydavateli.");
            1
        }
    };
    if !quiet {
        pause();
    }
    std::process::exit(code);
}

/// Konzolové okno instalátoru zmizí spolu s procesem — bez tohohle
/// by tester neviděl ani úspěch, ani chybu.
fn pause() {
    println!("\n  Stiskni Enter pro zavření…");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
}

fn install_dir() -> PathBuf {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    PathBuf::from(pf).join("Winsent")
}

fn start_menu_lnk() -> PathBuf {
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs\Winsent.lnk")
}

/// Verze, kterou má nainstalovaná kopie (soubor vedle binárek).
fn installed_version() -> Option<String> {
    std::fs::read_to_string(install_dir().join("version.txt"))
        .ok()
        .map(|s| s.trim().to_string())
}

fn do_install() -> Result<String, String> {
    let dir = install_dir();

    // ── 1. Jaká verze je v repu ────────────────────────────────────
    println!("  Zjišťuji aktuální verzi…");
    let sha = latest_commit()?;
    let base = format!("/{REPO}/{sha}/release");
    let version =
        http::get(RAW_HOST, &format!("{base}/version.txt"), |_| {}).map_err(|e| format!("{e}"))?;
    let version = String::from_utf8_lossy(&version).trim().to_string();
    if version.is_empty() {
        return Err("server vrátil prázdnou verzi".into());
    }
    println!("  Nejnovější verze: {version}");

    match installed_version() {
        Some(v) if v == version => {
            return Ok(format!("Už máš nejnovější verzi ({v}). Nic se nemění."));
        }
        Some(v) => println!("  Nainstalováno: {v} → aktualizuji"),
        None => println!("  Zatím nenainstalováno → instaluji"),
    }

    // ── 2. Stažení do paměti ───────────────────────────────────────
    // Nejdřív se stáhne všechno, teprve pak se sahá na disk: když
    // spadne síť uprostřed, zůstane funkční stará instalace.
    let mut payload = Vec::new();
    for name in FILES {
        print!("  Stahuji {name} … ");
        let _ = std::io::stdout().flush();
        let data = http::get(RAW_HOST, &format!("{base}/{name}"), |_| {})
            .map_err(|e| format!("{name}: {e}"))?;
        // Každý PE soubor začíná „MZ". Když dorazí HTML s chybou nebo
        // půlka souboru, pozná se to tady — ne až při spuštění.
        if data.len() < 100_000 || &data[..2] != b"MZ" {
            return Err(format!(
                "{name} se stáhl poškozený ({} B) — zkus to za chvíli znovu",
                data.len()
            ));
        }
        println!("{:.1} MB", data.len() as f64 / 1e6);
        payload.push((*name, data));
    }

    // ── 3. Zápis na disk ───────────────────────────────────────────
    println!("  Zastavuji službu…");
    service::stop_and_wait()?;
    // Běžící okno aplikace drží vlastní .exe zamčený.
    let _ = std::process::Command::new("taskkill.exe")
        .args(["/IM", "syswatch-ui.exe", "/F"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(500));

    std::fs::create_dir_all(&dir).map_err(|e| format!("nelze vytvořit {}: {e}", dir.display()))?;
    let mut total_kb = 0u32;
    for (name, data) in &payload {
        let path = dir.join(name);
        std::fs::write(&path, data).map_err(|e| format!("nelze zapsat {}: {e}", path.display()))?;
        total_kb += (data.len() / 1024) as u32;
    }
    std::fs::write(dir.join("version.txt"), &version)
        .map_err(|e| format!("nelze zapsat verzi: {e}"))?;

    // Instalátor si uloží kopii vedle aplikace — odinstalace přes
    // Programy a funkce pak funguje i po smazání staženého souboru.
    let setup_dst = dir.join("WinsentSetup.exe");
    if let Ok(me) = std::env::current_exe() {
        if me != setup_dst {
            let _ = std::fs::copy(&me, &setup_dst);
        }
    }

    // ── 4. Služba a zápisy do systému ──────────────────────────────
    println!("  Registruji službu…");
    service::install(&dir.join("syswatch.exe"))?;
    println!("  Spouštím službu…");
    service::start_and_wait()?;

    if let Err(e) = shell::create_shortcut(&dir.join("syswatch-ui.exe"), &start_menu_lnk()) {
        println!("  (zástupce se nepodařilo vytvořit: {e})");
    }
    if let Err(e) = shell::register_uninstall(&setup_dst, &dir, &version, total_kb) {
        println!("  (záznam v Programech a funkcích: {e})");
    }

    // ── 5. Spuštění aplikace ───────────────────────────────────────
    // Instalátor běží jako správce, ale aplikace musí běžet pod
    // běžným uživatelem (SPEC 2.1). Explorer je spuštěný pod ním,
    // takže spuštění „přes něj" práva vrátí na normální úroveň.
    let _ = std::process::Command::new("explorer.exe")
        .arg(dir.join("syswatch-ui.exe"))
        .spawn();

    Ok(format!(
        "Hotovo — Winsent {version} nainstalován.\n  \
         Najdeš ho v nabídce Start. Aktualizuje se spuštěním tohohle souboru znovu."
    ))
}

fn do_uninstall() -> Result<String, String> {
    println!("  Odebírám Winsent…");

    let _ = std::process::Command::new("taskkill.exe")
        .args(["/IM", "syswatch-ui.exe", "/F"])
        .output();
    service::stop_and_wait()?;
    service::uninstall()?;

    // ETW sessions přežijí konec procesu — bez tohohle by po
    // odinstalaci zůstaly viset v systému.
    for s in ["syswatch-rt", "syswatch-blackbox"] {
        let _ = std::process::Command::new("logman.exe")
            .args(["stop", s, "-ets"])
            .output();
    }

    let _ = std::fs::remove_file(start_menu_lnk());
    shell::unregister_uninstall();

    // Vlastní .exe smazat nejde, dokud běží — smaže se po restartu.
    let dir = install_dir();
    for name in FILES.iter().chain(["version.txt"].iter()) {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let _ = std::fs::remove_dir(&dir);

    let data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    let data = PathBuf::from(data).join("syswatch");
    Ok(format!(
        "Winsent odebrán.\n  \
         Nasbíraná data zůstala v {} — jsou tvoje, smaž je ručně, pokud je nechceš.",
        data.display()
    ))
}
