//! WinsentSetup — jediný soubor, který dostane tester.
//!
//! Spuštění bez parametrů = okno s tlačítkem Nainstalovat:
//!   1. stáhne z repozitáře verzi a obě binárky
//!   2. porovná s nainstalovanou verzí (stejná → jen prověří službu)
//!   3. zastaví službu, přepíše soubory, zaregistruje a spustí ji
//!   4. udělá zástupce a záznam v Programech a funkcích
//!
//! `WinsentSetup.exe /uninstall` = odeber všechno.
//! `WinsentSetup.exe /quiet`     = okno, které se spustí i zavře samo
//!                                 (tudy jde aktualizace z aplikace).
//! `WinsentSetup.exe /headless`  = bez okna, výpis do konzole (skripty).
//!
//! Manifest si vynutí práva správce, takže se tester nemusí trefovat
//! do „spustit jako správce" — Windows se zeptají samy.
//!
//! Podsystém je „windows", ne „console": jinak by u grafického
//! instalátoru bliklo černé okno. V headless režimu se konzole rodiče
//! připojí ručně, aby výpis měl kam jít.
#![windows_subsystem = "windows"]

mod gui;
mod http;
mod service;
mod shell;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Odkud se berou binárky. Veřejný repozitář, takže tester
/// nepotřebuje účet ani přístup.
const RAW_HOST: &str = "raw.githubusercontent.com";
const API_HOST: &str = "api.github.com";
const REPO: &str = "iva-exe/WinSent";

const FILES: &[&str] = &["syswatch.exe", "syswatch-ui.exe"];

/// Kroky instalace tak, jak je vidí uživatel v okně.
const INSTALL_STEPS: &[&str] = &[
    "Zjišťuji aktuální verzi",
    "Stahuji soubory",
    "Zastavuji službu",
    "Zapisuji soubory",
    "Registruji a spouštím službu",
    "Zástupce a záznam v systému",
];

const UNINSTALL_STEPS: &[&str] = &[
    "Zavírám aplikaci a zastavuji službu",
    "Odebírám službu",
    "Uklízím soubory a zástupce",
];

/// Hlášení průběhu. Okno i konzole dostávají totéž — jen to jinak
/// ukazují, takže se logika instalace nemusí ptát, kde zrovna běží.
trait Report {
    /// Začal krok `idx`, popisek pod pruhem je `status`.
    fn step(&mut self, idx: usize, status: &str);
    /// Jen změna řádku pod pruhem (stahování, opakované pokusy).
    fn status(&mut self, status: &str);
    /// 0.0–1.0, nebo `None` pro neurčitý pruh.
    fn progress(&mut self, p: Option<f32>);
}

/// Hlášení do okna.
struct GuiReport {
    state: gui::Shared,
    note: gui::Notifier,
}

impl Report for GuiReport {
    fn step(&mut self, idx: usize, status: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.step(idx, status);
        }
        self.note.tick();
    }
    fn status(&mut self, status: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.status = status.into();
        }
        self.note.tick();
    }
    fn progress(&mut self, p: Option<f32>) {
        if let Ok(mut s) = self.state.lock() {
            s.progress = p;
        }
        self.note.tick();
    }
}

/// Hlášení do konzole (headless).
struct ConsoleReport;

impl Report for ConsoleReport {
    fn step(&mut self, idx: usize, status: &str) {
        println!("  [{}] {}", idx + 1, status);
    }
    fn status(&mut self, status: &str) {
        println!("      {status}");
    }
    fn progress(&mut self, _p: Option<f32>) {}
}

fn main() {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .map(|a| a.to_ascii_lowercase())
        .collect();
    let has = |names: &[&str]| args.iter().any(|a| names.contains(&a.as_str()));

    let uninstall = has(&["/uninstall", "--uninstall", "/u"]);
    let headless = has(&["/headless", "--headless"]);
    // Tichý režim = okno, které se spustí i zavře samo. Tudy chodí
    // aktualizace z aplikace: uživatel klikl v aplikaci, takže se ho
    // nemá cenu ptát znovu — ale vidět, co se děje, chce.
    let quiet = has(&["/quiet", "--quiet", "/q", "/s", "/silent"]);

    if headless {
        // Podsystém je „windows", takže vlastní konzoli nemáme —
        // připojíme se k té, ze které nás spustili.
        attach_console();
        println!("  Winsent — {}", if uninstall { "odinstalace" } else { "instalace" });
        let mut rep = ConsoleReport;
        let r = if uninstall {
            do_uninstall(&mut rep)
        } else {
            do_install(&mut rep)
        };
        match r {
            Ok(msg) => {
                println!("\n  {msg}");
                std::process::exit(0);
            }
            Err(e) => {
                println!("\n  CHYBA: {e}");
                std::process::exit(1);
            }
        }
    }

    let (title, subtitle, steps, primary) = if uninstall {
        (
            "Odebrat Winsent",
            "monitor a správa Windows",
            UNINSTALL_STEPS,
            "Odebrat",
        )
    } else {
        (
            "Winsent",
            "monitor a správa Windows",
            INSTALL_STEPS,
            "Nainstalovat",
        )
    };
    let state: gui::Shared = Arc::new(Mutex::new(gui::State::new(
        title, subtitle, steps, primary,
    )));

    let action: gui::Action = Arc::new(move |st: gui::Shared, note: gui::Notifier| {
        let mut rep = GuiReport {
            state: Arc::clone(&st),
            note,
        };
        let r = if uninstall {
            do_uninstall(&mut rep)
        } else {
            do_install(&mut rep)
        };
        if let Ok(mut s) = st.lock() {
            match r {
                Ok(msg) => s.finish(&msg),
                Err(e) => s.fail(&e),
            }
        }
        note.tick();
    });

    gui::run(state, action, quiet, quiet);
}

/// Připojí konzoli rodiče, aby měl headless výpis kam jít.
fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: selhání (spuštěno bez konzole) se ignoruje — výpis pak
    // prostě nikam nejde, což je u headless z plochy v pořádku.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

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

fn mb(bytes: usize) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

fn do_install(rep: &mut dyn Report) -> Result<String, String> {
    let dir = install_dir();

    // ── 1. Jaká verze je v repu ────────────────────────────────────
    rep.step(0, "ptám se GitHubu na nejnovější verzi…");
    let sha = latest_commit()?;
    let base = format!("/{REPO}/{sha}/release");
    let version =
        http::get(RAW_HOST, &format!("{base}/version.txt"), |_| {}).map_err(|e| format!("{e}"))?;
    let version = String::from_utf8_lossy(&version).trim().to_string();
    if version.is_empty() {
        return Err("server vrátil prázdnou verzi".into());
    }

    // Shodná verze sama o sobě neznamená, že aplikace funguje: binárky
    // mohly zmizet, služba mohla zůstat zastavená. Za „nainstalováno"
    // se považuje až verze + obě binárky na disku.
    let files_ok = FILES.iter().all(|n| dir.join(n).is_file());
    match installed_version() {
        Some(v) if v == version && files_ok => {
            // Nic se nestahuje, ale služba se prověří a v případě
            // potřeby nastartuje. Tím je „spusť instalátor znovu"
            // jediná rada, kterou tester kdy potřebuje — opraví to
            // i zastavenou službu, kterou by jinak nikdo nezvedl.
            rep.step(4, "verze sedí — kontroluji službu…");
            service::install(&dir.join("syswatch.exe"))?;
            service::start_and_wait()?;
            launch_ui(&dir);
            return Ok(format!(
                "Winsent {v} je aktuální a služba běží. Najdeš ho v nabídce Start."
            ));
        }
        Some(v) if v == version => rep.status(&format!("verze {v} sedí, ale chybí soubory")),
        Some(v) => rep.status(&format!("nainstalováno {v} → aktualizuji na {version}")),
        None => rep.status(&format!("instaluji {version}")),
    }

    // ── 2. Stažení do paměti ───────────────────────────────────────
    // Nejdřív se stáhne všechno, teprve pak se sahá na disk: když
    // spadne síť uprostřed, zůstane funkční stará instalace.
    rep.step(1, "stahuji…");
    let mut payload = Vec::new();
    for (i, name) in FILES.iter().enumerate() {
        let name = *name;
        // Průběžné hlášení po 256 KiB — každý blok zvlášť by okno
        // budil stokrát za sekundu bez jediné nové informace.
        let mut last = 0usize;
        let data = http::get(RAW_HOST, &format!("{base}/{name}"), |n| {
            if n >= last + 256 * 1024 {
                last = n;
                rep.status(&format!("{name} — staženo {}", mb(n)));
            }
        })
        .map_err(|e| format!("{name}: {e}"))?;
        // Každý PE soubor začíná „MZ". Když dorazí HTML s chybou nebo
        // půlka souboru, pozná se to tady — ne až při spuštění.
        if data.len() < 100_000 || &data[..2] != b"MZ" {
            return Err(format!(
                "{name} se stáhl poškozený ({} B) — zkus to za chvíli znovu",
                data.len()
            ));
        }
        rep.status(&format!("{name} — {}", mb(data.len())));
        rep.progress(Some((i + 1) as f32 / FILES.len() as f32 * 0.5));
        payload.push((name, data));
    }

    // ── 3. Zápis na disk ───────────────────────────────────────────
    rep.step(2, "zastavuji službu a zavírám okno aplikace…");
    rep.progress(None);
    service::stop_and_wait()?;
    // Běžící okno aplikace drží vlastní .exe zamčený.
    let _ = std::process::Command::new("taskkill.exe")
        .args(["/IM", "syswatch-ui.exe", "/F"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Od téhle chvíle je systém rozestavěný: služba stojí a soubory
    // se přepisují. Cokoliv od teď selže, nesmí uživatele nechat bez
    // monitoringu — proto se výsledek zachytí a služba se v každém
    // případě vrátí do běhu.
    rep.step(3, "zapisuji do Program Files…");
    let outcome = write_payload(&dir, &payload, &version);

    let total_kb = match outcome {
        Ok(kb) => kb,
        Err(e) => {
            rep.status("instalace selhala — vracím službu do běhu…");
            let back = service::install(&dir.join("syswatch.exe"))
                .and_then(|()| service::start_and_wait());
            return Err(match back {
                Ok(()) => format!("{e}\nSlužbu jsem vrátil do běhu, monitoring jede dál."),
                Err(e2) => format!(
                    "{e}\nNavíc se nepodařilo nastartovat službu ({e2}) — zkus to znovu."
                ),
            });
        }
    };

    // ── 4. Služba a zápisy do systému ──────────────────────────────
    rep.step(4, "registruji a spouštím službu…");
    rep.progress(Some(0.8));
    service::install(&dir.join("syswatch.exe"))?;
    service::start_and_wait()?;

    rep.step(5, "zástupce v nabídce Start…");
    rep.progress(Some(0.95));
    if let Err(e) = shell::create_shortcut(&dir.join("syswatch-ui.exe"), &start_menu_lnk()) {
        rep.status(&format!("zástupce se nepodařilo vytvořit: {e}"));
    }
    if let Err(e) = shell::register_uninstall(&dir.join("WinsentSetup.exe"), &dir, &version, total_kb)
    {
        rep.status(&format!("záznam v Programech a funkcích: {e}"));
    }

    launch_ui(&dir);

    Ok(format!(
        "Hotovo — Winsent {version} je nainstalovaný a služba běží.\n\
         Aplikace se otevřela; najdeš ji taky v nabídce Start."
    ))
}

/// Přepíše binárky a verzi. Vrací celkovou velikost v KiB pro záznam
/// v Programech a funkcích.
fn write_payload(
    dir: &std::path::Path,
    payload: &[(&str, Vec<u8>)],
    version: &str,
) -> Result<u32, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("nelze vytvořit {}: {e}", dir.display()))?;
    let mut total_kb = 0u32;
    for (name, data) in payload {
        write_retry(&dir.join(name), data)?;
        total_kb += (data.len() / 1024) as u32;
    }
    // Verze se zapisuje až nakonec: kdyby zápis binárek selhal,
    // zůstane u nich sedět stará verze a příští spuštění instalátoru
    // je opraví. Nová verze u starých souborů by opravu zablokovala.
    write_retry(&dir.join("version.txt"), version.as_bytes())?;

    // Instalátor si uloží kopii vedle aplikace — odinstalace přes
    // Programy a funkce pak funguje i po smazání staženého souboru.
    let setup_dst = dir.join("WinsentSetup.exe");
    if let Ok(me) = std::env::current_exe() {
        if me != setup_dst {
            let _ = std::fs::copy(&me, &setup_dst);
        }
    }
    Ok(total_kb)
}

/// Zápis s několika pokusy.
///
/// Správce služeb hlásí „zastaveno" ve chvíli, kdy to služba ohlásí —
/// proces ale ještě chvíli dobíhá a drží vlastní .exe zamčený. Jedno
/// selhání by přitom znamenalo přerušenou instalaci se zastavenou
/// službou, takže pár sekund trpělivosti je tu jednoznačně levnější.
fn write_retry(path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..20 {
        match std::fs::write(path, data) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
    Err(format!("nelze zapsat {}: {last}", path.display()))
}

/// Spustí aplikaci pod přihlášeným uživatelem.
///
/// Instalátor běží jako správce, ale aplikace musí běžet pod běžným
/// uživatelem (SPEC 2.1). Explorer je spuštěný pod ním, takže
/// spuštění „přes něj" práva vrátí na normální úroveň.
fn launch_ui(dir: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(dir.join("syswatch-ui.exe"))
        .spawn();
}

fn do_uninstall(rep: &mut dyn Report) -> Result<String, String> {
    rep.step(0, "zavírám aplikaci a zastavuji službu…");
    let _ = std::process::Command::new("taskkill.exe")
        .args(["/IM", "syswatch-ui.exe", "/F"])
        .output();
    service::stop_and_wait()?;

    rep.step(1, "odebírám službu…");
    rep.progress(Some(0.4));
    service::uninstall()?;

    // ETW sessions přežijí konec procesu — bez tohohle by po
    // odinstalaci zůstaly viset v systému.
    for s in ["syswatch-rt", "syswatch-blackbox"] {
        let _ = std::process::Command::new("logman.exe")
            .args(["stop", s, "-ets"])
            .output();
    }

    rep.step(2, "uklízím soubory a zástupce…");
    rep.progress(Some(0.8));
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
        "Winsent je odebraný.\n\
         Nasbíraná data zůstala v {} — jsou tvoje, smaž je ručně, pokud je nechceš.",
        data.display()
    ))
}
