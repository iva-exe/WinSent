//! validate — validační vrstva, srdce bezpečnosti (SPEC kap. 17).
//!
//! Jediná brána mezi UI a mutacemi systému. Tři tvrdé vlastnosti:
//!
//! - **Samostatná** (17.1): závisí JEN na core-types a win-sys. Nezná
//!   exekutory — rozhoduje *zda*, ne *jak*. Jediný vstup `validate()`.
//! - **Rychlá** (17.2): T0 pár čtení stavu (< 50 ms), T1 plná kontrola.
//!   Čistě on-demand — žádné vlákno, v klidu 0 % CPU.
//! - **Neprůstřelná** (17.3): NIKDY nevěří snapshotu z UI. Každou akci
//!   ověřuje proti živému stavu OS v okamžiku validace. Když si nejsme
//!   jistí, odpověď je Deny.

use core_types::action::Action;

/// Verdikt validace. Žádný exekutor se nesmí spustit bez `Allow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny { reason: String },
}

impl Verdict {
    fn deny(reason: impl Into<String>) -> Verdict {
        Verdict::Deny {
            reason: reason.into(),
        }
    }
}

/// Živý stav OS pro validaci (SPEC 17.3). Drží jen znovupoužitelný
/// buffer pro NtQuery — data se čtou ČERSTVÁ při každé validaci,
/// nikdy se necachují mezi akcemi.
#[derive(Default)]
pub struct LiveContext {
    buf: Vec<u8>,
}

impl LiveContext {
    pub fn new() -> LiveContext {
        LiveContext::default()
    }
}

/// Jediný vstupní bod vrstvy (SPEC kap. 17.1). Neexistuje druhá cesta,
/// jak akci schválit.
pub fn validate(action: &Action, ctx: &mut LiveContext) -> Verdict {
    match action {
        // ── T0: testovací přepínač — odlehčená validace (17.2):
        // cíl známý? zápis vratný? (vratný z definice — in-memory)
        Action::TestToggle { key, .. } => {
            if key.trim().is_empty() {
                return Verdict::deny("prázdný klíč přepínače");
            }
            if !key.starts_with("test:") {
                return Verdict::deny(format!("neznámý přepínač „{key}“ — povolen jen test:*"));
            }
            Verdict::Allow
        }

        // ── T1: testovací operace — cíl musí existovat; `fake:*`
        // simuluje neexistující cíl, `critical:*` chráněný.
        Action::TestOp { target, .. } => {
            if target.starts_with("fake:") {
                return Verdict::deny(format!("cíl „{target}“ neexistuje"));
            }
            if target.starts_with("critical:") {
                return Verdict::deny(format!("cíl „{target}“ je kritický — akce zamčena"));
            }
            if target.trim().is_empty() {
                return Verdict::deny("prázdný cíl");
            }
            Verdict::Allow
        }

        // ── T0: startup položka (v6, SPEC 7 + 17.5): zdroj musí být
        // známý a přepínatelný, položka musí EXISTOVAT teď (čerstvé
        // čtení registru/služeb — ne to, co ukazuje UI). Vrstva zná
        // jen tvar id, ne kolektor (izolace 17.1).
        Action::StartupToggle { id, .. } => {
            let Some((source, name)) = id.split_once('|') else {
                return Verdict::deny("neplatný identifikátor položky");
            };
            if name.trim().is_empty() {
                return Verdict::deny("prázdný název položky");
            }
            // Co patří Windows, se nepřepíná. NIKDY. Klasifikace běží
            // ZNOVU a nezávisle na tom, co ukázalo UI (SPEC 17.3):
            // `command: None` znamená „přečti si příkaz sám čerstvě".
            // UI ty položky ve výchozím stavu vůbec nezobrazí, ale
            // spolehnout se na to nesmíme — do pipe může poslat příkaz
            // kterýkoli přihlášený uživatel, takže vrstva je poslední
            // slovo, ne zdvořilost vůči UI.
            if let Some(why) = system_startup_reason(id, None, None) {
                return Verdict::deny(format!(
                    "„{name}“ patří Windows ({why}) — startovací položky systému Winsent nepřepíná"
                ));
            }
            match source {
                // Winlogon hooky se nikdy nepřepínají — jen varují.
                "shell" => Verdict::deny(
                    "položky Winlogon (Userinit/Shell) se nepřepínají — jsou jen k náhledu",
                ),
                "run_user" | "run_machine" => {
                    if startup_run_exists(name, source == "run_machine") {
                        Verdict::Allow
                    } else {
                        Verdict::deny(format!("položka „{name}“ v Run klíči neexistuje"))
                    }
                }
                "folder_user" | "folder_common" => {
                    if startup_folder_exists(name, source == "folder_common") {
                        Verdict::Allow
                    } else {
                        Verdict::deny(format!("soubor „{name}“ ve složce po spuštění neexistuje"))
                    }
                }
                "task" => {
                    if win_sys::tasksched::task_enabled(name).is_some() {
                        Verdict::Allow
                    } else {
                        Verdict::deny(format!("naplánovaná úloha „{name}“ neexistuje"))
                    }
                }
                "service" => match service_start_type(name) {
                    None => Verdict::deny(format!("služba „{name}“ neexistuje")),
                    // 0/1 = boot/system driver, 4 = disabled — na ty nesaháme.
                    Some(t) if t < 2 => {
                        Verdict::deny(format!("„{name}“ je systémový ovladač — akce zamčena"))
                    }
                    Some(4) => Verdict::deny(format!("služba „{name}“ je zakázaná správcem")),
                    Some(_) => Verdict::Allow,
                },
                other => Verdict::deny(format!("neznámý zdroj startup položky „{other}“")),
            }
        }

        // ── T1: mazání do koše (v8, SPEC 18.2). Nejpřísnější validace
        // v projektu — smazaný soubor jde sice vrátit z koše, ale
        // rozbitý systém ne.
        Action::DeleteFiles { paths } => {
            if paths.is_empty() {
                return Verdict::deny("nebyla vybrána žádná cesta");
            }
            if paths.len() > 500 {
                return Verdict::deny("příliš mnoho položek najednou (max 500)");
            }
            for path in paths {
                let p = path.trim();
                if p.is_empty() {
                    return Verdict::deny("prázdná cesta");
                }
                // Relativní cesty a wildcardy sem nepatří — cíl musí
                // být jednoznačný, ne něco, co se doexpanduje jinde.
                if p.contains('*') || p.contains('?') {
                    return Verdict::deny("zástupné znaky nejsou povolené");
                }
                let path_buf = std::path::Path::new(p);
                if !path_buf.is_absolute() {
                    return Verdict::deny(format!("cesta není absolutní: {p}"));
                }
                // ČERSTVÁ kontrola existence (SPEC 17.3) — UI mohlo
                // ukazovat starý stav.
                if !path_buf.exists() {
                    return Verdict::deny(format!("už neexistuje: {p}"));
                }
                if let Some(reason) = protected_path(p) {
                    return Verdict::deny(reason);
                }
                // Kritický držitel (Restart Manager) akci zamyká.
                if let Ok(hs) = win_sys::rm::holders(std::slice::from_ref(&path.clone())) {
                    if let Some(h) = hs
                        .iter()
                        .find(|h| h.kind == win_sys::rm::HolderKind::Critical)
                    {
                        return Verdict::deny(format!(
                            "soubor drží kritický systémový proces {} (pid {})",
                            h.name, h.pid
                        ));
                    }
                }
            }
            Verdict::Allow
        }

        // ── T1: odinstalace oficiálním odinstalátorem (v8, SPEC 5.3).
        // Příkaz se čte ČERSTVĚ z registru — UI ho neposílá, takže ho
        // nejde podvrhnout; ověřuje se i existence spouštěné binárky.
        Action::UninstallApp { identity_key } => {
            let Some(name) = identity_key.strip_prefix("app:") else {
                return Verdict::deny(
                    "odinstalovat jde jen klasicky nainstalovaný program (ne Store aplikaci)",
                );
            };
            if name.trim().is_empty() {
                return Verdict::deny("prázdný identifikátor aplikace");
            }
            match uninstall_command(name) {
                None => Verdict::deny(format!(
                    "„{name}“ nemá v registru odinstalační příkaz — odinstalovat ho odsud nelze"
                )),
                Some(cmd) => {
                    // Binárka odinstalátoru musí existovat teď.
                    match exe_of_command(&cmd) {
                        Some(exe) if std::path::Path::new(&exe).exists() => Verdict::Allow,
                        Some(exe) => Verdict::deny(format!(
                            "odinstalátor na disku není: {exe} — zbyl jen záznam v registru"
                        )),
                        None => Verdict::deny(format!("nečitelný odinstalační příkaz: {cmd}")),
                    }
                }
            }
        }


        // ── T1: úklid záznamu po programu, který na disku není (v10).
        // Nejde o odinstalaci: odinstalátor tu není co spustit, zbyl
        // jen klíč v registru. Proto se ověřuje ČERSTVĚ a přísně —
        // stačí, aby po programu na disku cokoliv zbylo, a zamítáme.
        Action::PurgeGhost { identity_key } => {
            let Some(name) = identity_key.strip_prefix("app:") else {
                return Verdict::deny(
                    "z registru se uklízí jen klasicky nainstalovaný program (ne Store aplikace)",
                );
            };
            if name.trim().is_empty() {
                return Verdict::deny("prázdný identifikátor aplikace");
            }
            let Some(g) = ghost_entry(name) else {
                return Verdict::deny(format!("„{name}“ v registru není — není co uklízet"));
            };
            // Odinstalátor na disku = program tam pořád je. Tohle není
            // náhradní cesta k odinstalaci.
            if let Some(exe) = g.uninstall_exe.as_deref() {
                if std::path::Path::new(exe).exists() {
                    return Verdict::deny(format!(
                        "odinstalátor na disku je ({exe}) — použij Odinstalovat, ne úklid registru"
                    ));
                }
            }
            for dir in &g.dirs {
                if let Some(why) = protected_path(dir) {
                    return Verdict::deny(why);
                }
                match dir_state(dir) {
                    DirState::Missing | DirState::Empty => {}
                    DirState::HasFiles => {
                        return Verdict::deny(format!(
                            "ve složce „{dir}“ pořád něco je — z registru se uklízí jen po programu, který na disku není"
                        ))
                    }
                    DirState::Unreadable => {
                        return Verdict::deny(format!("do složky „{dir}“ nevidíme — radši nic"))
                    }
                }
            }
            Verdict::Allow
        }

        // ── T1: ukončení procesu (v7). Stejná kontrola jako CheckProc
        // + zákaz sebevraždy: démon nesmí zabít sám sebe (přišli
        // bychom o monitoring i o auditní zápis výsledku).
        Action::KillProc {
            pid, create_time, ..
        } => {
            if *pid == std::process::id() {
                return Verdict::deny("Winsent nemůže ukončit sám sebe");
            }
            if *pid <= 4 {
                return Verdict::deny("jádro systému (pid 0/4) nelze ukončit");
            }
            validate(
                &Action::CheckProc {
                    pid: *pid,
                    create_time: *create_time,
                },
                ctx,
            )
        }

        // ── T1: kontrola živého procesu — ČERSTVÉ čtení OS, žádná
        // cache. Sdílený základ pro kill.
        Action::CheckProc { pid, create_time } => {
            let procs = match win_sys::proc::snapshot_processes(&mut ctx.buf) {
                Ok(p) => p,
                // Nejde přečíst stav OS → nejsme si jistí → Deny.
                Err(e) => return Verdict::deny(format!("nelze ověřit stav OS: {e}")),
            };
            let Some(p) = procs.iter().find(|p| p.pid == *pid) else {
                return Verdict::deny(format!("proces {pid} neexistuje"));
            };
            // Recyklace PID: identita je (pid, create_time), ne holý PID.
            if p.create_time != *create_time {
                return Verdict::deny(format!(
                    "proces {pid} není tentýž (instance nesouhlasí — PID byl recyklován)"
                ));
            }
            // Tvrdý seznam jmen NAVÍC k příznakům OS (SPEC v7): některé
            // kritické procesy nemají BreakOnTermination ani PPL podle
            // konfigurace stroje (např. lsass bez RunAsPPL), a přesto je
            // jejich ukončení okamžitý BSOD. Jméno se bere z čerstvého
            // snapshotu, ne z UI.
            if is_critical_name(&p.name) {
                return Verdict::deny(format!(
                    "{} je nezbytný pro chod Windows — ukončení shodí systém",
                    p.name
                ));
            }
            // Třída ochrany ČERSTVĚ z OS, ne z identity cache.
            match win_sys::procinfo::protection(*pid, &p.name) {
                win_sys::procinfo::Protection::Critical => {
                    Verdict::deny(format!("proces {} je kritický pro systém", p.name))
                }
                win_sys::procinfo::Protection::Protected => {
                    Verdict::deny(format!("proces {} je chráněný (PPL)", p.name))
                }
                _ => Verdict::Allow,
            }
        }
    }
}

/// Procesy, jejichž ukončení systém neustojí — nezávisle na tom, co
/// hlásí příznaky OS (BreakOnTermination/PPL nejsou zapnuté všude).
/// Poslední záchranná brzda; kontroluje se před jakýmkoli voláním.
const CRITICAL_NAMES: &[&str] = &[
    "system",
    "registry",
    "idle",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "memory compression",
    "memcompression",
    "ntoskrnl.exe",
    "securesystem",
];

/// Je jméno procesu na tvrdém seznamu kritických?
fn is_critical_name(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    CRITICAL_NAMES.iter().any(|c| *c == n)
}

/// Existuje hodnota v Run klíči? (čerstvě, obě architektury)
/// Uživatelské položky se hledají v HKU\<SID>, ne v HKEY_CURRENT_USER —
/// démon běží jako LocalSystem, takže by to byla hive SYSTEMu.
fn startup_run_exists(name: &str, machine: bool) -> bool {
    run_value(name, machine).is_some()
}

/// Existuje soubor ve Startup složce?
fn startup_folder_exists(name: &str, common: bool) -> bool {
    let base = if common {
        std::env::var("ProgramData").ok()
    } else {
        std::env::var("APPDATA").ok()
    };
    base.map(|b| {
        std::path::Path::new(&format!(
            r"{b}\Microsoft\Windows\Start Menu\Programs\Startup\{name}"
        ))
        .exists()
    })
    .unwrap_or(false)
}

/// Start typ služby z registru (2 = auto, 3 = ruční, 4 = zakázáno).
fn service_start_type(name: &str) -> Option<u64> {
    // Jméno služby nesmí obsahovat cestu — jinak by šlo číst cizí klíče.
    if name.contains('\\') || name.contains('/') {
        return None;
    }
    win_sys::registry::read_u64(
        win_sys::registry::HKEY_LOCAL_MACHINE,
        &format!(r"SYSTEM\CurrentControlSet\Services\{name}"),
        "Start",
    )
}

// ── JEDNA definice „položka po spuštění patří Windows" ─────────────
//
// Rozhoduje se tady, ve vrstvě, a to ze dvou důvodů. Za prvé UI vlastníka
// souboru z WebView nepřečte, takže by muselo hádat podle cesty — a cesta
// lže v obou směrech (ovladač HP v System32, NVIDIA v DriverStore, hry
// ve WindowsApps, OneDrive v profilu uživatele). Za druhé by dvě pravidla
// znamenala přepínač, který skončí odmítnutím. Verdikt se proto počítá
// jednou a do UI CESTUJE jako pole `system` na StartupRow.
//
// `pub` ze stejného důvodu jako `uninstall_command`: obě strany musí
// vidět TOTÉŽ rozhodnutí, žádná cesta okolo vrstvy.

/// Memo vlastníků na dobu JEDNOHO skenu: stovky služeb sdílejí tentýž
/// svchost.exe, unikátních cest je zlomek.
pub type OwnerMemo = std::collections::HashMap<String, Option<bool>>;

/// Rodiny MSIX balíčků, které jsou součástí systému. Drží se shodné se
/// `SYS_FAMILIES` v `crates/ui/src/lib/mandatory.js`.
const SYSTEM_PACKAGE_PREFIXES: &[&str] = &[
    "microsoftwindows.",
    "microsoft.windows.",
    "windows.",
    "microsoft.aad.",
    "microsoft.accountscontrol",
    "microsoft.lockapp",
    "microsoft.sechealthui",
    "microsoft.win32webviewhost",
];

/// Proměnné, které ukazují do profilu uživatele. Démon běží jako
/// LocalSystem, takže by se mu rozbalily na `…\config\systemprofile\…`,
/// tedy dovnitř Windows. Bez téhle zkratky by zmizel OneDrive.
const USER_VARS: &[&str] = &[
    "%localappdata%",
    "%appdata%",
    "%userprofile%",
    "%homepath%",
    "%onedrive%",
];

/// Součásti Windows, které se aktualizují MIMO servisní stack, takže je
/// nevlastní TrustedInstaller a neleží ve %SystemRoot%. Bez tohohle
/// seznamu by Microsoft Defender vyšel jako aplikace třetí strany
/// a nabídli bychom u antiviru přepínač, který Windows stejně odmítnou.
const COMPONENT_ZONES: &[&str] = &[
    r"\program files\windows defender\",
    r"\program files (x86)\windows defender\",
    r"\programdata\microsoft\windows defender\",
    r"\program files\windows defender advanced threat protection\",
];

/// Patří položka po spuštění Windows? Vrací DŮVOD (česky, jde rovnou do
/// UI), nebo `None` u položky třetí strany.
///
/// `command` — už přečtený příkaz, když ho volající má (sken v démonu);
/// `None` znamená „přečti si ho sám ČERSTVĚ" a používá ho validační
/// vrstva, která snapshotu z UI věřit nesmí (SPEC 17.3).
/// `memo` — cache vlastníků na dobu jednoho skenu; validace jedné
/// položky ji nepotřebuje (`None`).
pub fn system_startup_reason(
    id: &str,
    command: Option<&str>,
    mut memo: Option<&mut OwnerMemo>,
) -> Option<String> {
    let (source, name) = id.split_once('|')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    match source {
        // Winlogon se zobrazuje JEN v nestandardním stavu — tedy právě
        // když systému nepatří. Skrýt alarm by bylo horší než řádek
        // navíc; přepnutí zakazuje vlastní větev ve `validate`.
        "shell" => None,

        // Do složek po spuštění si Windows nic nedává (desktop.ini
        // kolektor přeskakuje). Cokoliv tam je, je cizí.
        "folder_user" | "folder_common" => None,

        // Run klíče. Windows tu svoje věci prakticky nemá, takže riziko
        // je opačné — schovat něco cizího.
        "run_user" | "run_machine" => {
            let cmd = match command {
                Some(c) => c.to_string(),
                None => run_value(name, source == "run_machine")?,
            };
            if per_user_command(&cmd) {
                return None;
            }
            let payload = payload_of_command(&cmd)?;
            payload_verdict(&payload, &mut memo, "spouští ji součást Windows")
        }

        "task" => {
            // Cesta úlohy musí být kotvená v kořeni. Plánovač bere
            // i „Microsoft\Windows\…" bez úvodního lomítka a
            // `IRegisteredTask::Path` takový tvar vrátí nezměněný —
            // prefixové pravidlo by šlo obejít jedním smazaným znakem.
            if !name.starts_with(char::from(92u8)) {
                return Some("cesta úlohy není kotvená — bere se jako systémová".into());
            }
            // \Microsoft\Windows\ je vyhrazený jmenný prostor OS.
            // Samotné \Microsoft\ ne — Office ani EdgeUpdate nejsou
            // Windows a přepínat je jde.
            if name.to_ascii_lowercase().starts_with(r"\microsoft\windows\") {
                return Some("naplánovaná úloha Windows".into());
            }
            let cmd = match command {
                Some(c) => Some(c.to_string()),
                None => win_sys::tasksched::task_payload(name),
            };
            match cmd.as_deref().and_then(payload_of_command) {
                Some(p) => payload_verdict(&p, &mut memo, "naplánovaná úloha Windows"),
                // COM handler, kterému se nedohledala knihovna. Vypnutí
                // MsCtfMonitor rozbije psaní a IME — když nevíme, zamykáme.
                None => Some("úloha bez čitelné binárky — bere se jako systémová".into()),
            }
        }

        "service" => {
            // Jméno služby nesmí obsahovat cestu — jinak by šlo číst cizí klíče.
            if name.contains(char::from(92u8)) || name.contains('/') {
                return Some("neplatné jméno služby".into());
            }
            if service_start_type(name).is_some_and(|t| t < 2) {
                return Some("ovladač zaváděný při startu systému".into());
            }
            if service_display_is_mui(name) {
                return Some("služba Windows".into());
            }
            match service_payload(name) {
                Some(p) => payload_verdict(&p, &mut memo, "služba Windows"),
                // Parameters nejde přečíst → nevíme → systém.
                None => Some("službu nejde přečíst — bere se jako systémová".into()),
            }
        }

        "msix" => {
            let fam = name.to_ascii_lowercase();
            SYSTEM_PACKAGE_PREFIXES
                .iter()
                .any(|f| fam.starts_with(f))
                .then(|| "součást Windows z Microsoft Storu".to_string())
        }

        _ => Some("neznámý zdroj — bere se jako systémový".into()),
    }
}

/// Společný závěr žebříku: zóna vendora → cizí, součást mimo servisní
/// stack → systém, TrustedInstaller → systém, nečitelný vlastník →
/// rozhoduje umístění.
fn payload_verdict(payload: &str, memo: &mut Option<&mut OwnerMemo>, why: &str) -> Option<String> {
    if vendor_zone(payload) {
        return None;
    }
    if component_zone(payload) {
        return Some(why.to_string());
    }
    match owner_is_trusted_installer(payload, memo) {
        Some(true) => Some(why.to_string()),
        Some(false) => None,
        // Soubor na disku není (duch po odinstalaci) nebo je nečitelný.
        // Uvnitř Windows to bereme jako systém, mimo jako cizí — jinak
        // by zmizeli právě ti duchové, které chce uživatel vyhodit.
        None if under_system_root(payload) => {
            Some(format!("{why} — soubor nečitelný, ale leží ve Windows"))
        }
        None => None,
    }
}

/// Vlastník payloadu, s memem na dobu skenu.
fn owner_is_trusted_installer(path: &str, memo: &mut Option<&mut OwnerMemo>) -> Option<bool> {
    let key = path.to_ascii_lowercase();
    if let Some(m) = memo.as_deref_mut() {
        if let Some(v) = m.get(&key) {
            return *v;
        }
    }
    let v = win_sys::sysowner::owned_by_trusted_installer(path);
    if let Some(m) = memo.as_deref_mut() {
        m.insert(key, v);
    }
    v
}

/// Zóny, kde vlastnictví TrustedInstaller NEZNAMENÁ NIC: soubory tam
/// pokládá servisní stack Windows, ale patří vendorovi.
fn vendor_zone(path: &str) -> bool {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    if p.contains(r"\driverstore\filerepository\") {
        return true;
    }
    if let Some(i) = p.find(r"\windowsapps\") {
        let pkg = &p[i + r"\windowsapps\".len()..];
        let fam = pkg.split('\\').next().unwrap_or("");
        return !SYSTEM_PACKAGE_PREFIXES.iter().any(|f| fam.starts_with(f));
    }
    false
}

/// Součást Windows, která bydlí mimo %SystemRoot% a aktualizuje se
/// vlastním kanálem (Defender). Vlastník ani umístění ji neprozradí.
fn component_zone(path: &str) -> bool {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    COMPONENT_ZONES.iter().any(|z| p.contains(z))
}

/// Kořen Windows (bez lomítka na konci).
fn system_root() -> String {
    std::env::var("SystemRoot")
        .unwrap_or_else(|_| r"C:\Windows".into())
        .trim_end_matches(char::from(92u8))
        .to_string()
}

/// Leží cesta pod %SystemRoot%?
fn under_system_root(path: &str) -> bool {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    p.starts_with(&format!("{}\\", system_root().to_ascii_lowercase()))
}

/// Míří příkaz do profilu uživatele?
fn per_user_command(cmd: &str) -> bool {
    let lc = cmd.to_ascii_lowercase();
    USER_VARS.iter().any(|v| lc.contains(v))
        || lc.contains(r"\users\")
        || lc.contains(r"\config\systemprofile\")
}

/// Rozbalí `%VAR%`. Neznámou proměnnou nechá být — volající pak pozná,
/// že se cesta nerozřešila (zbylo v ní `%`).
fn expand_vars(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let key = &after[..end];
                match std::env::var(key) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(key);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Soubor, který příkazová řádka doopravdy spouští: uvozovky, NT prefix
/// `\??\`, `%VAR%`, a u generických hostitelů (`rundll32`, `regsvr32`)
/// modul z argumentů — sám rundll32.exe vlastní TrustedInstaller vždycky,
/// takže by prohlásil za Windows cokoliv, co ho zneužije.
/// `None` = příkazu nerozumíme.
fn payload_of_command(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return None;
    }
    let (first, rest) = if let Some(tail) = cmd.strip_prefix('"') {
        let end = tail.find('"')?;
        (&tail[..end], &tail[end + 1..])
    } else {
        let lc = cmd.to_ascii_lowercase();
        let end = [".exe", ".dll", ".sys"]
            .iter()
            .filter_map(|e| lc.find(e).map(|i| i + e.len()))
            .min()?;
        (&cmd[..end], &cmd[end..])
    };
    let first = first.trim();
    let first = expand_vars(first.strip_prefix(r"\??\").unwrap_or(first));
    let lc = first.to_ascii_lowercase();
    let host = lc.ends_with(r"\rundll32.exe")
        || lc.ends_with(r"\regsvr32.exe")
        || lc == "rundll32.exe"
        || lc == "regsvr32.exe";
    if host {
        let dll = rest
            .split(|c: char| c.is_whitespace() || c == ',')
            .map(|t| t.trim_matches('"'))
            .find(|t| t.to_ascii_lowercase().ends_with(".dll"))?;
        let dll = expand_vars(dll);
        if dll.contains('%') {
            return None;
        }
        // Holé jméno bez cesty hledá Windows v System32.
        return Some(if dll.contains(char::from(92u8)) {
            dll
        } else {
            format!(r"{}\System32\{dll}", system_root())
        });
    }
    (!first.contains('%') && !first.trim().is_empty()).then_some(first)
}

/// Hodnota z Run/RunOnce klíče, ČERSTVĚ. HKCU se záměrně neptáme:
/// démon běží jako LocalSystem, takže by to byla hive SYSTEMu, ne
/// přihlášeného uživatele — uživatelské položky se hledají v HKU\<SID>,
/// stejně jako v `uninstall_command`.
fn run_value(name: &str, machine: bool) -> Option<String> {
    use win_sys::registry::{enum_values, RegKey, HKEY_LOCAL_MACHINE, HKEY_USERS};
    const SUBS: &[&str] = &[
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce",
    ];
    let mut roots: Vec<(RegKey, String)> = Vec::new();
    if machine {
        for s in SUBS {
            roots.push((HKEY_LOCAL_MACHINE, (*s).to_string()));
        }
    } else {
        for sid in win_sys::consent::user_hives() {
            for s in SUBS {
                roots.push((HKEY_USERS, format!(r"{sid}\{s}")));
            }
        }
    }
    for (root, sub) in roots {
        if let Some((_, v)) = enum_values(root, &sub)
            .into_iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            return Some(v);
        }
    }
    None
}

/// Soubor, který služba doopravdy spouští. U svchostu je to
/// `Parameters\ServiceDll`, ne hostitel: stovky služeb sdílejí tentýž
/// svchost.exe, takže testovat hostitele znamená prohlásit tiskový
/// démon HP za součást Windows.
fn service_payload(name: &str) -> Option<String> {
    use win_sys::registry::{read_string, HKEY_LOCAL_MACHINE as HKLM};
    let base = format!(r"SYSTEM\CurrentControlSet\Services\{name}");
    let host = payload_of_command(&service_image_path(name)?)?;
    if !host.to_ascii_lowercase().ends_with(r"\svchost.exe") {
        return Some(host);
    }
    // Instance na uživatele (`CDPUserSvc_8427c`) mají Parameters jen na
    // šablonovém klíči bez suffixu `_<hex>`; ten se liší stroj od stroje.
    let mut keys = vec![base];
    if let Some((tpl, _)) = name.rsplit_once('_') {
        keys.push(format!(r"SYSTEM\CurrentControlSet\Services\{tpl}"));
    }
    for k in keys {
        for (sub, value) in [(format!(r"{k}\Parameters"), "ServiceDll"), (k, "ServiceDll")] {
            let Some(dll) = read_string(HKLM, &sub, value) else {
                continue;
            };
            let dll = expand_vars(dll.trim());
            if !dll.trim().is_empty() && !dll.contains('%') {
                return Some(dll);
            }
        }
    }
    None
}

/// ImagePath služby, čerstvě z registru.
fn service_image_path(name: &str) -> Option<String> {
    win_sys::registry::read_string(
        win_sys::registry::HKEY_LOCAL_MACHINE,
        &format!(r"SYSTEM\CurrentControlSet\Services\{name}"),
        "ImagePath",
    )
}

/// Má služba MUI-nepřímý název nebo popis? Takhle se registruje
/// komponenta Windows (`@%SystemRoot%\system32\schedsvc.dll,-100`);
/// třetí strany tam mají prostý text („ESET Service"). Cesta musí mířit
/// do Windows NEBO do zóny součásti mimo servisní stack — právě tak se
/// hlásí Microsoft Defender (`@%ProgramFiles%\Windows Defender\…`).
fn service_display_is_mui(name: &str) -> bool {
    use win_sys::registry::{read_string, HKEY_LOCAL_MACHINE as HKLM};
    let mut keys = vec![format!(r"SYSTEM\CurrentControlSet\Services\{name}")];
    if let Some((tpl, _)) = name.rsplit_once('_') {
        keys.push(format!(r"SYSTEM\CurrentControlSet\Services\{tpl}"));
    }
    for k in &keys {
        for v in ["DisplayName", "Description"] {
            let Some(s) = read_string(HKLM, k, v) else {
                continue;
            };
            let Some(rest) = s.trim().strip_prefix('@') else {
                continue;
            };
            let path = rest.rsplit_once(',').map(|(p, _)| p).unwrap_or(rest);
            let path = expand_vars(path.trim());
            if vendor_zone(&path) {
                continue;
            }
            if under_system_root(&path) || component_zone(&path) {
                return true;
            }
        }
    }
    false
}

/// Odinstalační příkaz aplikace z registru (čerstvě, dle DisplayName).
/// Preferuje tichou variantu. `pub` — používá ho i exekutor, aby obě
/// strany viděly TÝŽ příkaz (žádná cesta okolo vrstvy).
pub fn uninstall_command(display_name: &str) -> Option<String> {
    use win_sys::registry::{enum_subkeys, read_string, read_u64, HKEY_LOCAL_MACHINE, HKEY_USERS};
    let want = display_name.trim().to_ascii_lowercase();

    // POZOR: služba běží jako SYSTEM, takže HKEY_CURRENT_USER je hive
    // SYSTEMu — ne přihlášeného uživatele. Aplikace instalované „jen
    // pro mě" se proto hledají v HKU\<SID> reálných uživatelů.
    let mut roots: Vec<(win_sys::registry::RegKey, String)> = vec![
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall".to_string(),
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall".to_string(),
        ),
    ];
    for sid in enum_subkeys(HKEY_USERS, "") {
        if sid.starts_with("S-1-5-21") && !sid.ends_with("_Classes") {
            roots.push((
                HKEY_USERS,
                format!(r"{sid}\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
            ));
        }
    }

    for (root, base) in roots {
        let base = base.as_str();
        for sub in enum_subkeys(root, base) {
            let key = format!("{base}\\{sub}");
            let Some(name) = read_string(root, &key, "DisplayName") else {
                continue;
            };
            if name.trim().to_ascii_lowercase() != want {
                continue;
            }
            // Systémové komponenty se odsud neodinstalovávají.
            if read_u64(root, &key, "SystemComponent") == Some(1) {
                return None;
            }
            if let Some(q) = read_string(root, &key, "QuietUninstallString") {
                if !q.trim().is_empty() {
                    return Some(q);
                }
            }
            if let Some(u) = read_string(root, &key, "UninstallString") {
                if !u.trim().is_empty() {
                    return Some(u);
                }
            }
        }
    }
    None
}


/// Záznam v registru po programu — kde leží a co po něm zbylo.
/// `pub`, protože totéž musí vidět i exekutor: co vrstva povolila
/// smazat, to se smaže, a nic jiného (žádná cesta okolo vrstvy).
#[derive(Debug, Clone)]
pub struct GhostEntry {
    /// Kořen a podklíč v registru (`HKLM\…\Uninstall\{GUID}`).
    pub root: win_sys::registry::RegKey,
    pub key: String,
    /// Binárka odinstalátoru, pokud je v záznamu uvedená.
    pub uninstall_exe: Option<String>,
    /// Složky, které záznam uvádí jako svoje (InstallLocation a adresář
    /// odinstalátoru). Smí se odstranit jen prázdné.
    pub dirs: Vec<String>,
}

/// Najde záznam v Uninstall klíčích podle DisplayName. Čte se čerstvě —
/// UI posílá jen jméno, nikdy cestu do registru.
pub fn ghost_entry(display_name: &str) -> Option<GhostEntry> {
    use win_sys::registry::{enum_subkeys, read_string, read_u64, HKEY_LOCAL_MACHINE, HKEY_USERS};
    let want = display_name.trim().to_ascii_lowercase();

    // Služba běží jako SYSTEM, takže HKEY_CURRENT_USER je hive SYSTEMu.
    // Instalace „jen pro mě" se hledají v HKU\<SID> reálných uživatelů.
    let mut roots: Vec<(win_sys::registry::RegKey, String)> = vec![
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall".to_string(),
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall".to_string(),
        ),
    ];
    for sid in enum_subkeys(HKEY_USERS, "") {
        if sid.starts_with("S-1-5-21") && !sid.ends_with("_Classes") {
            roots.push((
                HKEY_USERS,
                format!(r"{sid}\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
            ));
        }
    }

    for (root, base) in roots {
        for sub in enum_subkeys(root, &base) {
            let key = format!("{base}\\{sub}");
            let Some(name) = read_string(root, &key, "DisplayName") else {
                continue;
            };
            if name.trim().to_ascii_lowercase() != want {
                continue;
            }
            // Systémové komponenty se z registru neuklízejí.
            if read_u64(root, &key, "SystemComponent") == Some(1) {
                return None;
            }
            let uninstall_exe = read_string(root, &key, "UninstallString")
                .as_deref()
                .and_then(exe_of_command);
            let mut dirs: Vec<String> = Vec::new();
            if let Some(loc) = read_string(root, &key, "InstallLocation") {
                let loc = loc.trim().trim_matches('"').trim_end_matches(char::from(92u8));
                if !loc.is_empty() {
                    dirs.push(loc.to_string());
                }
            }
            // Adresář odinstalátoru bere v úvahu jen tehdy, když je pod
            // Program Files nebo v profilu — jinak by to mohl být
            // sdílený adresář (System32, Temp) a ten se nemaže.
            if let Some(exe) = uninstall_exe.as_deref() {
                if let Some(dir) = std::path::Path::new(exe).parent() {
                    let d = dir.to_string_lossy().to_string();
                    if own_install_dir(&d) && !dirs.iter().any(|x| x.eq_ignore_ascii_case(&d)) {
                        dirs.push(d);
                    }
                }
            }
            return Some(GhostEntry {
                root,
                key,
                uninstall_exe,
                dirs,
            });
        }
    }
    None
}

/// Vypadá cesta jako VLASTNÍ adresář programu? Sdílené adresáře (kořen
/// Program Files, Temp, System32) se nikdy nemažou ani prázdné.
fn own_install_dir(path: &str) -> bool {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    let p = p.trim_end_matches(char::from(92u8));
    // Musí být aspoň o dvě úrovně hlouběji než kořen svazku, jinak je to
    // něco jako `C:\Program Files` a mazat to nesmíme.
    let depth = p.matches(char::from(92u8)).count();
    if depth < 2 {
        return false;
    }
    const SHARED: &[&str] = &[
        r"\windows",
        r"\program files",
        r"\program files (x86)",
        r"\programdata",
        r"\users",
    ];
    !SHARED.iter().any(|s| p == format!("c:{s}"))
}

/// Stav složky pro úklid: chybí / je prázdná (i po stromu) / něco v ní
/// je / nejde přečíst.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirState {
    Missing,
    Empty,
    HasFiles,
    Unreadable,
}

/// Prázdná = nemá ANI JEDEN soubor, ať je zanoření jakékoli. Prázdné
/// podsložky se počítají za prázdno. Hloubka je omezená, aby zacyklený
/// junction point nedržel vlákno navěky.
pub fn dir_state(path: &str) -> DirState {
    fn walk(p: &std::path::Path, depth: u32) -> DirState {
        if depth > 12 {
            return DirState::Unreadable;
        }
        let Ok(rd) = std::fs::read_dir(p) else {
            return DirState::Unreadable;
        };
        let state = DirState::Empty;
        for e in rd {
            let Ok(e) = e else {
                return DirState::Unreadable;
            };
            let Ok(ft) = e.file_type() else {
                return DirState::Unreadable;
            };
            // Symlink ani junction se nesleduje — cíl patří někomu jinému.
            if ft.is_symlink() || ft.is_file() {
                return DirState::HasFiles;
            }
            match walk(&e.path(), depth + 1) {
                DirState::Empty | DirState::Missing => {}
                other => return other,
            }
        }
        state
    }
    let p = std::path::Path::new(path);
    match p.try_exists() {
        Ok(false) => DirState::Missing,
        Ok(true) if p.is_dir() => walk(p, 0),
        // Cesta existuje, ale není to složka → je to soubor.
        Ok(true) => DirState::HasFiles,
        Err(_) => DirState::Unreadable,
    }
}

/// Cesta k .exe z příkazové řádky (s uvozovkami i bez).
pub fn exe_of_command(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    let path = if let Some(rest) = cmd.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        let lc = cmd.to_ascii_lowercase();
        let end = lc.find(".exe")? + 4;
        &cmd[..end]
    };
    (!path.trim().is_empty()).then(|| path.to_string())
}

/// Cesty, které se NIKDY nesmí mazat (SPEC 18.2). Vrací důvod
/// zamítnutí, nebo None když je cesta v pořádku.
fn protected_path(path: &str) -> Option<String> {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    let p = p.trim_end_matches('\\').to_string();

    // Kořen svazku („C:", „C:\") — smazat disk nelze ani omylem.
    if p.len() <= 3 && p.contains(':') {
        return Some("kořen disku nelze smazat".into());
    }

    let sysroot = std::env::var("SystemRoot")
        .unwrap_or_else(|_| r"C:\Windows".into())
        .to_ascii_lowercase();
    let sysroot = sysroot.trim_end_matches('\\').to_string();

    // Samotné systémové adresáře (ne jejich obsah v temp).
    const SYSTEM_DIRS: &[&str] = &[
        "\\windows",
        "\\system32",
        "\\syswow64",
        "\\winsxs",
        "\\boot",
        "\\perflogs",
        "\\recovery",
        "\\program files",
        "\\program files (x86)",
        "\\programdata",
        "\\users",
        "\\$recycle.bin",
        "\\system volume information",
    ];
    for d in SYSTEM_DIRS {
        // Přesná shoda adresáře, ne prefix cesty pod ním: mazat
        // C:\Program Files\Něco\soubor.txt je legitimní, mazat celé
        // C:\Program Files ne.
        if p.ends_with(d) && p.matches('\\').count() <= 2 {
            return Some(format!("systémový adresář nelze smazat: {path}"));
        }
    }

    // Cokoliv PŘÍMO ve Windows\System32 a spol. — tam se maže jen
    // přes Windows Update, ne přes nás. Výjimka: temp adresáře.
    let in_temp = p.contains("\\temp\\") || p.contains("\\inetcache\\");
    if !in_temp
        && (p.starts_with(&format!("{sysroot}\\system32"))
            || p.starts_with(&format!("{sysroot}\\syswow64"))
            || p.starts_with(&format!("{sysroot}\\winsxs")))
    {
        return Some(format!("systémový soubor Windows nelze smazat: {path}"));
    }

    // Profil uživatele jako celek (C:\Users\Jmeno) — ne jednotlivé
    // soubory v něm.
    if p.matches('\\').count() == 2 && p.contains("\\users\\") {
        return Some("celý uživatelský profil nelze smazat".into());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Cesty selhání se testují víc než cesty úspěchu (brána v5) ──

    #[test]
    fn startup_shell_hook_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::StartupToggle {
                id: "shell|Userinit".into(),
                on: false,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }), "Winlogon je zamčený");
    }

    #[test]
    fn startup_unknown_source_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::StartupToggle {
                id: "vymyslene|neco".into(),
                on: true,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    #[test]
    fn startup_malformed_id_denied() {
        let mut ctx = LiveContext::new();
        for id in ["bez-oddelovace", "run_user|", "run_user|   "] {
            let v = validate(
                &Action::StartupToggle {
                    id: id.into(),
                    on: true,
                },
                &mut ctx,
            );
            assert!(matches!(v, Verdict::Deny { .. }), "{id} musí být zamítnut");
        }
    }

    #[test]
    fn startup_nonexistent_item_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::StartupToggle {
                id: "run_user|UrciteNeexistujiciPolozka123".into(),
                on: false,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    // Živá data: kritický systémový ovladač nelze přepnout.
    #[test]
    fn startup_boot_driver_denied() {
        let mut ctx = LiveContext::new();
        // disk.sys = Start 0 (boot driver) na každém Windows.
        let v = validate(
            &Action::StartupToggle {
                id: "service|disk".into(),
                on: false,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }), "boot driver je zamčený");
    }

    // Brána v7: kill kritického procesu musí být zamítnutý PŘED
    // jakýmkoli voláním (System, pid 4).
    #[test]
    fn kill_critical_denied() {
        let mut ctx = LiveContext::new();
        let ct = {
            let procs = win_sys::proc::snapshot_processes(&mut ctx.buf).expect("snapshot");
            procs
                .iter()
                .find(|p| p.pid == 4)
                .expect("System")
                .create_time
        };
        let v = validate(
            &Action::KillProc {
                pid: 4,
                create_time: ct,
                tree: false,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    // Brána v7: kritická jména jsou zamčená i bez příznaků OS
    // (lsass bez RunAsPPL, csrss bez BreakOnTermination…).
    #[test]
    fn critical_names_locked() {
        for n in ["lsass.exe", "csrss.exe", "WinLogon.exe", "System"] {
            assert!(is_critical_name(n), "{n} musí být na tvrdém seznamu");
        }
        for n in ["notepad.exe", "chrome.exe", ""] {
            assert!(!is_critical_name(n), "{n} na seznamu být nemá");
        }
    }

    // Démon nesmí zabít sám sebe.
    #[test]
    fn kill_self_denied() {
        let mut ctx = LiveContext::new();
        let pid = std::process::id();
        let ct = {
            let procs = win_sys::proc::snapshot_processes(&mut ctx.buf).expect("snapshot");
            procs
                .iter()
                .find(|p| p.pid == pid)
                .expect("self")
                .create_time
        };
        let v = validate(
            &Action::KillProc {
                pid,
                create_time: ct,
                tree: false,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    // Recyklovaný PID: kill se špatnou instancí zamítnut.
    #[test]
    fn kill_wrong_instance_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::KillProc {
                pid: std::process::id(),
                create_time: 999,
                tree: true,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    // Brána v8: systémové cesty jsou zamčené PŘED jakýmkoli mazáním.
    #[test]
    fn delete_system_paths_denied() {
        let mut ctx = LiveContext::new();
        let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        for p in [
            r"C:\".to_string(),
            r"C:\Windows".to_string(),
            r"C:\Program Files".to_string(),
            r"C:\Users".to_string(),
            format!(r"{sysroot}\System32"),
            format!(r"{sysroot}\System32\kernel32.dll"),
        ] {
            let v = validate(
                &Action::DeleteFiles {
                    paths: vec![p.clone()],
                },
                &mut ctx,
            );
            assert!(
                matches!(v, Verdict::Deny { .. }),
                "mazání {p} musí být zamítnuto, ale prošlo"
            );
        }
    }

    // Neexistující cesta, wildcard i relativní cesta = zamítnuto.
    #[test]
    fn delete_bad_targets_denied() {
        let mut ctx = LiveContext::new();
        for p in [
            r"C:\rozhodne-neexistujici-slozka-xyz\a.txt",
            r"C:\Users\*\Documents",
            r"relativni\cesta.txt",
            "",
        ] {
            let v = validate(
                &Action::DeleteFiles {
                    paths: vec![p.to_string()],
                },
                &mut ctx,
            );
            assert!(matches!(v, Verdict::Deny { .. }), "{p} mělo být zamítnuto");
        }
    }

    // Běžný soubor v temp adresáři projde (to je smysl úklidu).
    #[test]
    fn delete_temp_file_allowed() {
        let mut ctx = LiveContext::new();
        let f = std::env::temp_dir().join("winsent-validate-test.tmp");
        std::fs::write(&f, b"x").expect("zapsat testovací soubor");
        let v = validate(
            &Action::DeleteFiles {
                paths: vec![f.to_string_lossy().into_owned()],
            },
            &mut ctx,
        );
        let _ = std::fs::remove_file(&f);
        assert!(matches!(v, Verdict::Allow), "běžný temp soubor má projít");
    }

    // Ochrana proti podvrženému jménu služby s cestou.
    #[test]
    fn startup_service_path_traversal_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::StartupToggle {
                id: r"service|..\..\Foo".into(),
                on: false,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    #[test]
    fn toggle_unknown_key_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::TestToggle {
                key: "startup:foo".into(),
                on: true,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    #[test]
    fn toggle_empty_key_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::TestToggle {
                key: "  ".into(),
                on: false,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    #[test]
    fn toggle_test_key_allowed() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::TestToggle {
                key: "test:demo".into(),
                on: true,
            },
            &mut ctx,
        );
        assert_eq!(v, Verdict::Allow);
    }

    #[test]
    fn op_fake_target_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::TestOp {
                target: "fake:missing".into(),
                fail_at: None,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    #[test]
    fn op_critical_target_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::TestOp {
                target: "critical:core".into(),
                fail_at: None,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    // Živá data: neexistující PID musí být zamítnut.
    #[test]
    fn proc_nonexistent_denied() {
        let mut ctx = LiveContext::new();
        let v = validate(
            &Action::CheckProc {
                pid: 4_000_000_001,
                create_time: 1,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    // Živá data: recyklovaný PID (špatný create_time) zamítnut.
    #[test]
    fn proc_wrong_instance_denied() {
        let mut ctx = LiveContext::new();
        let pid = std::process::id();
        let v = validate(
            &Action::CheckProc {
                pid,
                create_time: 12345, // určitě nesedí
            },
            &mut ctx,
        );
        match v {
            Verdict::Deny { reason } => assert!(reason.contains("recyklován"), "{reason}"),
            Verdict::Allow => panic!("špatná instance musí být zamítnuta"),
        }
    }

    // Živá data: kritický systémový proces (System, pid 4) zamítnut.
    #[test]
    fn proc_critical_denied() {
        let mut ctx = LiveContext::new();
        let procs = win_sys::proc::snapshot_processes(&mut ctx.buf).expect("snapshot");
        let sys = procs.iter().find(|p| p.pid == 4).expect("System pid 4");
        let (pid, ct) = (sys.pid, sys.create_time);
        let v = validate(
            &Action::CheckProc {
                pid,
                create_time: ct,
            },
            &mut ctx,
        );
        assert!(matches!(v, Verdict::Deny { .. }), "System musí být zamčený");
    }

    // Živá data: vlastní (uživatelský) proces projde.
    #[test]
    fn proc_own_allowed() {
        let mut ctx = LiveContext::new();
        let pid = std::process::id();
        let ct = {
            let procs = win_sys::proc::snapshot_processes(&mut ctx.buf).expect("snapshot");
            procs
                .iter()
                .find(|p| p.pid == pid)
                .expect("vlastní proces")
                .create_time
        };
        let v = validate(
            &Action::CheckProc {
                pid,
                create_time: ct,
            },
            &mut ctx,
        );
        assert_eq!(v, Verdict::Allow);
    }
}

/// Mapa `DisplayName (malými) → binárka odinstalátoru`, přečtená JEDNÍM
/// průchodem Uninstall klíči.
///
/// `uninstall_command` prochází celý registr při každém volání, takže
/// pro celý inventář (stovky aplikací) by to byla kvadratická práce.
/// Tohle je tentýž zdroj, jen přečtený najednou.
pub fn uninstall_exe_index() -> std::collections::HashMap<String, Option<String>> {
    use win_sys::registry::{enum_subkeys, read_string, read_u64, HKEY_LOCAL_MACHINE, HKEY_USERS};
    let mut out = std::collections::HashMap::new();

    let mut roots: Vec<(win_sys::registry::RegKey, String)> = vec![
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall".to_string(),
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall".to_string(),
        ),
    ];
    for sid in enum_subkeys(HKEY_USERS, "") {
        if sid.starts_with("S-1-5-21") && !sid.ends_with("_Classes") {
            roots.push((
                HKEY_USERS,
                format!(r"{sid}\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
            ));
        }
    }
    for (root, base) in roots {
        for sub in enum_subkeys(root, &base) {
            let key = format!("{base}{}{sub}", char::from(92u8));
            let Some(name) = read_string(root, &key, "DisplayName") else {
                continue;
            };
            if read_u64(root, &key, "SystemComponent") == Some(1) {
                continue;
            }
            let exe = read_string(root, &key, "QuietUninstallString")
                .filter(|s| !s.trim().is_empty())
                .or_else(|| read_string(root, &key, "UninstallString"))
                .as_deref()
                .and_then(exe_of_command);
            out.insert(name.trim().to_ascii_lowercase(), exe);
        }
    }
    out
}
