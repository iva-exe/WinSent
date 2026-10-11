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

/// Jméno vlastní služby. Winsent se v seznamu startovacích položek
/// objevuje jako každá jiná a jedním kliknutím by si tam šlo vypnout
/// vlastní automatický start.
const SELF_SERVICE: &str = "syswatch";

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
            // Winsent si nesmí vypnout sám sebe. Vlastní služba je
            // v seznamu jako každá jiná a jedno kliknutí by ji přehodilo
            // na ruční spouštění — jenže služba by běžela dál a pravda
            // by vyšla najevo až po restartu počítače. „Bylo to zapnuté
            // a najednou se to nespouští, nevím od kdy" je pak přesně
            // ten příznak, který se nedá vystopovat. Vypnout to jde
            // v Nastavení, kde je u toho napsáno, o co uživatel přijde.
            if source == "service" && name.eq_ignore_ascii_case(SELF_SERVICE) {
                return Verdict::deny(
                    "tohle je sám Winsent — bez automatického startu by po restartu počítače přestal sbírat historii i incidenty. Přestat měřit se dá odinstalací; přepínač v Nastavení řídí jen okno aplikace, ne službu.",
                );
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
            // Antivir se vypíná ve svém nastavení. Zvenčí to buď zastaví
            // jeho sebeobrana, nebo se to povede a počítač po restartu
            // zůstane bez ochrany, o které uživatel myslí, že ji má.
            if let Some(why) = security_startup_reason(id, None, &security_dirs()) {
                return Verdict::deny(format!("„{name}“: {why}"));
            }
            match source {
                // Winlogon hooky se nikdy nepřepínají — jen varují.
                "shell" => Verdict::deny(
                    "položky Winlogon (Userinit/Shell) se nepřepínají — jsou jen k náhledu",
                ),
                "run_user" | "run_machine" | "run_machine32" => {
                    if startup_run_exists(name, source) {
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
            match delete_targets(paths) {
                Ok(_) => Verdict::Allow,
                Err(reason) => Verdict::deny(reason),
            }
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
            // Co Windows samy odinstalovat nedovolí (NoRemove) a co je
            // jen aktualizace jiného programu, se odsud neodebírá — UI
            // tlačítko nenabídne a vrstva to stejně odmítne.
            if let Some(why) = uninstall_lock_reason(name) {
                return Verdict::deny(why);
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
                if let Some(why) = purge_protected_path(dir) {
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

        // ── T1: ukončení procesu (v7, v11). Živá identita (pid +
        // create_time) jako u CheckProc a pak JEDNO pravidlo „co Winsent
        // neukončuje" (`kill_deny_reason`) — totéž, kterým actor-proc
        // prosévá potomky ve stromu.
        Action::KillProc {
            pid, create_time, ..
        } => {
            if *pid == std::process::id() {
                return Verdict::deny("Winsent nemůže ukončit sám sebe");
            }
            let (name, procs) = match live_process(*pid, *create_time, ctx) {
                Ok(v) => v,
                Err(v) => return v,
            };
            let rules = KillRules::fresh();
            if let Some(why) = kill_deny_reason(*pid, &name, &rules) {
                return Verdict::deny(why);
            }
            // Vykreslovací procesy okna Winsentu (WebView2 pod
            // syswatch-ui.exe): jejich konec = prázdné nebo spadlé okno.
            if ancestor_is_winsent(&procs, *pid, &rules) {
                return Verdict::deny(format!(
                    "{name}: patří oknu Winsentu — zavírá se s ním, ne zvlášť"
                ));
            }
            Verdict::Allow
        }

        // ── T1: kontrola živého procesu — ČERSTVÉ čtení OS, žádná
        // cache.
        Action::CheckProc { pid, create_time } => {
            let name = match live_process(*pid, *create_time, ctx) {
                Ok((name, _)) => name,
                Err(v) => return v,
            };
            // Tvrdý seznam jmen NAVÍC k příznakům OS (SPEC v7): některé
            // kritické procesy nemají BreakOnTermination ani PPL podle
            // konfigurace stroje (např. lsass bez RunAsPPL), a přesto je
            // jejich ukončení okamžitý BSOD. Jméno se bere z čerstvého
            // snapshotu, ne z UI.
            if win_sys::procinfo::is_critical_name(&name) {
                return Verdict::deny(format!(
                    "{name} je nezbytný pro chod Windows — ukončení shodí systém"
                ));
            }
            // Třída ochrany ČERSTVĚ z OS, ne z identity cache.
            match win_sys::procinfo::protection(*pid, &name) {
                win_sys::procinfo::Protection::Critical => {
                    Verdict::deny(format!("proces {name} je kritický pro systém"))
                }
                win_sys::procinfo::Protection::Protected => {
                    Verdict::deny(format!("proces {name} je chráněný (PPL)"))
                }
                _ => Verdict::Allow,
            }
        }
    }
}

/// Živý proces s touto identitou (pid + čas vzniku) — jeho jméno
/// a čerstvý snapshot všech procesů. Jinak zamítnutí.
fn live_process(
    pid: u32,
    create_time: i64,
    ctx: &mut LiveContext,
) -> Result<(String, Vec<win_sys::proc::RawProc>), Verdict> {
    let procs = match win_sys::proc::snapshot_processes(&mut ctx.buf) {
        Ok(p) => p,
        // Nejde přečíst stav OS → nejsme si jistí → Deny.
        Err(e) => return Err(Verdict::deny(format!("nelze ověřit stav OS: {e}"))),
    };
    let Some(p) = procs.iter().find(|p| p.pid == pid) else {
        return Err(Verdict::deny(format!("proces {pid} neexistuje")));
    };
    // Recyklace PID: identita je (pid, create_time), ne holý PID.
    if p.create_time != create_time {
        return Err(Verdict::deny(format!(
            "proces {pid} není tentýž (instance nesouhlasí — PID byl recyklován)"
        )));
    }
    let name = p.name.clone();
    Ok((name, procs))
}

/// Je proces Winsent sám (služba nebo okno)?
fn is_winsent(pid: u32, rules: &KillRules) -> bool {
    if pid == std::process::id() {
        return true;
    }
    win_sys::procinfo::image_path(pid)
        .is_some_and(|img| winsent_image(&img.replace('/', "\\").to_ascii_lowercase(), rules))
}

/// Je tahle cesta k image (malými písmeny) binárka Winsentu? Jméno samo
/// nestačí — `syswatch.exe` si může pojmenovat kdokoli; musí ležet
/// tam, odkud Winsent běží.
fn winsent_image(image_lc: &str, rules: &KillRules) -> bool {
    let (dir, exe) = image_lc
        .rsplit_once(char::from(92u8))
        .unwrap_or(("", image_lc));
    OWN_EXES.contains(&exe) && rules.own_dirs.iter().any(|d| d == dir)
}

/// Leží proces ve stromu pod Winsentem? Rodič se uznává jen starší než
/// dítě (recyklované PID, viz actor-proc), řetěz má strop.
fn ancestor_is_winsent(procs: &[win_sys::proc::RawProc], pid: u32, rules: &KillRules) -> bool {
    let mut cur = procs.iter().find(|p| p.pid == pid);
    for _ in 0..16 {
        let Some(p) = cur else { break };
        let rodic = procs
            .iter()
            .find(|q| q.pid == p.parent_pid && q.pid != p.pid && q.create_time <= p.create_time);
        match rodic {
            Some(q) if is_winsent(q.pid, rules) => return true,
            Some(q) => cur = Some(q),
            None => break,
        }
    }
    false
}

// ── Co Winsent NEUKONČUJE (v11) ─────────────────────────────────────
//
// Jedno pravidlo pro kořen ukončení (validace KillProc) i pro každého
// potomka ve stromu (actor-proc). Dřív se kontroloval jen kořen a strom
// se dopočítal podle parent PID — a ten Windows recyklují: proces, který
// zdědil PID dávno mrtvého rodiče csrss.exe, by si ho vzal do stromu.

/// Nástroje Windows, které si uživatel pouští sám a smí je zavřít, i když
/// běží ze složky Windows. Všechno ostatní odtamtud Winsent neukončuje.
pub const USER_TOOLS: &[&str] = &[
    "notepad.exe",
    "mspaint.exe",
    "cmd.exe",
    "powershell.exe",
    "powershell_ise.exe",
    "regedit.exe",
    "mmc.exe",
    "taskmgr.exe",
    "msinfo32.exe",
    "write.exe",
    "charmap.exe",
    "snippingtool.exe",
];

/// Spustitelné soubory Winsentu (služba a okno).
const OWN_EXES: &[&str] = &["syswatch.exe", "syswatch-ui.exe"];

/// Stav OS, o který se pravidlo opírá a který se vyplatí přečíst jednou
/// pro celý strom (Security Center jde přes WMI).
#[derive(Debug, Clone, Default)]
pub struct KillRules {
    /// %SystemRoot% malými písmeny, bez koncového lomítka.
    sysroot: String,
    /// Adresáře bezpečnostních produktů (Security Center).
    security_dirs: Vec<String>,
    /// Kde bydlí Winsent (adresář služby, %ProgramFiles%\Winsent).
    own_dirs: Vec<String>,
}

impl KillRules {
    /// Přečte stav teď. Security Center se drží nejvýš minutu — mění se
    /// jen instalací antiviru a WMI dotaz by jinak zdržel každý kill.
    pub fn fresh() -> KillRules {
        KillRules {
            sysroot: system_root().to_ascii_lowercase(),
            security_dirs: security_dirs(),
            own_dirs: own_dirs(),
        }
    }
}

/// Adresáře bezpečnostních produktů ze Security Center (malými písmeny,
/// bez lomítka na konci), čtené nejvýš minutu zpátky.
pub fn security_dirs() -> Vec<String> {
    win_sys::security::product_dirs_cached(std::time::Duration::from_secs(60))
}

/// Kde leží binárky Winsentu (malými písmeny, bez lomítka): adresář
/// běžící služby (ve vývoji `target\debug`) a instalace v Program Files.
fn own_dirs() -> Vec<String> {
    let mut out = Vec::new();
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_string_lossy().to_ascii_lowercase()))
    {
        out.push(d.trim_end_matches(char::from(92u8)).to_string());
    }
    if let Ok(pf) = std::env::var("ProgramFiles") {
        let d = format!(r"{}\winsent", pf.trim_end_matches(char::from(92u8))).to_ascii_lowercase();
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// Je image součást Windows? Přesně identita `os:windows` (crate
/// identity): cesta pod %SystemRoot% A platný podpis Microsoftu,
/// embedded nebo katalogový — tentýž test `win_sys::trust`.
///
/// Dřív stačila cesta. Jenže doprovodné služby ovladačů bydlí
/// v `System32\DriverStore` (NVIDIA Display Container, Realtek, AMD)
/// a podepisuje je výrobce: UI je podle identity nabídlo k ukončení
/// s varováním „služba" a služba je pak vždycky zamítla s nepravdou
/// „součást Windows". Totéž nepodepsaná binárka v `C:\Windows\Temp`,
/// kam smí zapisovat každý — ta spíš patří vetřelci než Windows.
/// Kritické procesy hlídá už dřív tvrdý seznam jmen,
/// BreakOnTermination a PPL — ty na podpisu nezávisí.
///
/// `lc` = `image` malými písmeny se zpětnými lomítky. Podpis se ověřuje
/// jen pod %SystemRoot% (blokující, jednotky až desítky ms — jen při
/// akci, nikdy v samplovacím cyklu).
fn windows_image(image: &str, lc: &str, rules: &KillRules) -> bool {
    if !under(lc, &rules.sysroot) {
        return false;
    }
    let p = std::path::Path::new(image);
    win_sys::trust::microsoft_signed(p, &win_sys::trust::signer_subject(p))
}

/// Leží `path_lc` (malými písmeny) uvnitř `dir_lc`?
fn under(path_lc: &str, dir_lc: &str) -> bool {
    let dir = dir_lc.trim_end_matches(char::from(92u8));
    !dir.is_empty()
        && path_lc.len() > dir.len()
        && path_lc.starts_with(dir)
        && path_lc.as_bytes()[dir.len()] == b'\\'
}

/// Proč se proces `pid` NESMÍ ukončit (česky, jde rovnou do UI), nebo
/// `None`. Stav se čte čerstvě z OS (ochrana, cesta k image, balíček);
/// identitu (pid + čas vzniku) ověřuje volající.
///
/// Nikdy:
/// - Winsent sám (služba i okno);
/// - jádro, pseudoprocesy a kritické procesy (tvrdý seznam jmen,
///   BreakOnTermination, PP/PPL);
/// - cokoli, o čem nejde zjistit, odkud běží;
/// - bezpečnostní software (Security Center, Defender);
/// - soubory Windows (složka Windows + podpis Microsoftu, viz
///   `windows_image`) mimo `USER_TOOLS`;
/// - systémové aplikace ze Storu.
pub fn kill_deny_reason(pid: u32, name: &str, rules: &KillRules) -> Option<String> {
    const WINDOWS: &str = "součást Windows — Winsent ji neukončuje";
    if pid == std::process::id() {
        return Some("Winsent nemůže ukončit sám sebe".into());
    }
    if pid <= 4 {
        return Some(format!("{name}: jádro systému — {WINDOWS}"));
    }
    if win_sys::procinfo::is_critical_name(name) {
        return Some(format!(
            "{name}: {WINDOWS} (bez ní se zhroutí systém nebo přihlášení)"
        ));
    }
    match win_sys::procinfo::protection(pid, name) {
        win_sys::procinfo::Protection::Critical => {
            return Some(format!(
                "{name}: {WINDOWS} (kritický proces — ukončení shodí systém)"
            ))
        }
        win_sys::procinfo::Protection::Protected => {
            return Some(format!(
                "{name}: chráněný proces (PPL) — Windows jeho ukončení nedovolí"
            ))
        }
        _ => {}
    }
    // Pseudoprocesy (Secure System, Memory Compression…) a cokoli, co
    // se schovává, cestu k souboru nemají. Nevíme = neukončujeme.
    let Some(image) = win_sys::procinfo::image_path(pid) else {
        return Some(format!(
            "{name}: {WINDOWS} (nejde zjistit, odkud proces běží)"
        ));
    };
    let lc = image.replace('/', "\\").to_ascii_lowercase();
    let (_, exe) = lc
        .rsplit_once(char::from(92u8))
        .unwrap_or(("", lc.as_str()));
    if winsent_image(&lc, rules) {
        return Some("Winsent nemůže ukončit sám sebe".into());
    }
    if component_zone(&lc) || rules.security_dirs.iter().any(|d| under(&lc, d)) {
        return Some(format!(
            "{name}: bezpečnostní software — Winsent ho neukončuje (vypíná se v jeho vlastním nastavení)"
        ));
    }
    if !USER_TOOLS.contains(&exe) && windows_image(&image, &lc, rules) {
        return Some(format!("{name}: {WINDOWS}"));
    }
    if win_sys::procinfo::package_family(pid).is_some_and(|f| win_sys::msix::is_system_family(&f)) {
        return Some(format!("{name}: {WINDOWS} (systémová aplikace ze Storu)"));
    }
    None
}

/// Existuje hodnota v Run klíči? (čerstvě, obě architektury)
/// Uživatelské položky se hledají v HKU\<SID>, ne v HKEY_CURRENT_USER —
/// démon běží jako LocalSystem, takže by to byla hive SYSTEMu.
fn startup_run_exists(name: &str, source: &str) -> bool {
    run_value(name, source).is_some()
}

/// Existuje soubor ve Startup složce?
///
/// Uživatelská složka se bere z profilu přihlášeného uživatele, stejně
/// jako ji čte kolektor. Dřív tu bylo `%APPDATA%` — jenže služba běží
/// jako SYSTEM a to se jí rozbalí do vlastního profilu (systemprofile).
/// Soubor tam nikdy nebyl, takže validace přepnutí každé položky
/// z uživatelské složky Po spuštění zamítla jako „neexistuje".
fn startup_folder_exists(name: &str, common: bool) -> bool {
    // Jméno je jen jméno souboru — žádná cesta ven ze složky.
    if name.is_empty() || name.contains(['\\', '/']) || name.contains("..") {
        return false;
    }
    let base = if common {
        std::env::var("ProgramData")
            .ok()
            .map(|p| format!(r"{p}\Microsoft\Windows\Start Menu\Programs\Startup"))
    } else {
        win_sys::consent::user_hives()
            .into_iter()
            .next()
            .and_then(|sid| win_sys::consent::profile_path(&sid))
            .map(|p| format!(r"{p}\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup"))
    };
    base.map(|b| std::path::Path::new(&b).join(name).exists())
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
        // Každý Run klíč má vlastní zdroj (collector-boot), jinak by
        // stejné jméno ve dvou klíčích dalo dvě položky se stejným id.
        "run_user" | "run_machine" | "run_machine32" | "run_once_user" | "run_once_machine" => {
            let cmd = match command {
                Some(c) => c.to_string(),
                None => run_value(name, source)?,
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

        // Jedna definice systémového balíčku pro celou službu (podpis
        // System, záložně prefixy shodné s UI) — win_sys::msix.
        "msix" => win_sys::msix::is_system_family(name)
            .then(|| "součást Windows z Microsoft Storu".to_string()),

        _ => Some("neznámý zdroj — bere se jako systémový".into()),
    }
}

/// Důvod u položky bezpečnostního softwaru — jde rovnou do UI.
pub const SECURITY_STARTUP_REASON: &str =
    "bezpečnostní software — vypíná se v jeho vlastním nastavení";

/// Patří položka po spuštění bezpečnostnímu produktu (antivir, firewall
/// ze Security Center)? Vrací důvod, nebo `None`.
///
/// Stejná logika pro řádek v UI (služba ji pošle jako `system`) i pro
/// zákaz přepnutí. ESET a spol. vypadají zvenčí jako kterákoli služba
/// třetí strany — vlastník souboru Administrators, prostý název — takže
/// je žebřík `system_startup_reason` pustí. Poznat je jde jen podle toho,
/// kam se zaregistrovaly do Security Center.
///
/// `dirs` = `security_dirs()`; `command` jako u `system_startup_reason`.
pub fn security_startup_reason(id: &str, command: Option<&str>, dirs: &[String]) -> Option<String> {
    if dirs.is_empty() {
        return None;
    }
    let p = startup_payload(id, command)?
        .replace('/', "\\")
        .to_ascii_lowercase();
    dirs.iter()
        .any(|d| under(&p, d))
        .then(|| SECURITY_STARTUP_REASON.to_string())
}

/// Varování u položky, kterou vypnout JDE, ale něco důležitého s ní
/// přestane fungovat. Rozhoduje se tady, protože jen služba vidí, co
/// služba doopravdy spouští (ServiceDll, payload v DriverStore).
pub fn startup_caution(id: &str, command: Option<&str>) -> Option<String> {
    const EDGE: &str = "aktualizuje i WebView2, na kterém běží Winsent";
    let (source, name) = id.split_once('|')?;
    let n = name.trim().to_ascii_lowercase();
    match source {
        "service" => {
            match n.as_str() {
                "clicktorunsvc" => return Some("bez ní se Office nespustí".into()),
                "edgeupdate" | "edgeupdatem" => return Some(EDGE.into()),
                "gameinputredistservice" => return Some(
                    "obsluhuje herní ovladače — bez ní nemusí v některých hrách fungovat gamepad"
                        .into(),
                ),
                _ => {}
            }
        }
        // Tentýž aktualizátor Edge, jen jako naplánovaná úloha.
        "task" if n.starts_with(r"\microsoftedgeupdatetaskmachine") => return Some(EDGE.into()),
        _ => {}
    }
    // Doprovodné služby a programy ovladačů (NVIDIA Display Container,
    // Realtek, AMD External Events) bydlí v úložišti ovladačů. Windows
    // je za své nepovažují — a nejsou — ale bez nich přestane fungovat
    // ovládací panel a část funkcí ovladače.
    let p = startup_payload(id, command)?.to_ascii_lowercase();
    p.contains(r"\driverstore\filerepository\")
        .then(|| "součást ovladače — bez ní přestane fungovat část funkcí ovladače".into())
}

/// Soubor, který položka po spuštění doopravdy spouští (čerstvě, když
/// `command` chybí). `None` u zdrojů, kde to nevíme (složky po spuštění,
/// MSIX, Winlogon).
fn startup_payload(id: &str, command: Option<&str>) -> Option<String> {
    let (source, name) = id.split_once('|')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    match source {
        "run_user" | "run_machine" | "run_machine32" | "run_once_user" | "run_once_machine" => {
            let cmd = match command {
                Some(c) => c.to_string(),
                None => run_value(name, source)?,
            };
            payload_of_command(&cmd)
        }
        "task" => {
            let cmd = match command {
                Some(c) => Some(c.to_string()),
                None => win_sys::tasksched::task_payload(name),
            };
            cmd.as_deref().and_then(payload_of_command)
        }
        // U služby rozhoduje registr, ne příkaz z UI: svchost hostí
        // ServiceDll a ta je tím, co se doopravdy spouští.
        "service" if !name.contains(char::from(92u8)) && !name.contains('/') => {
            service_payload(name)
        }
        _ => None,
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
        // Složka nese plné jméno balíčku; is_system_family si z něj
        // rodinu odvodí sama.
        let full = pkg.split('\\').next().unwrap_or("");
        return !win_sys::msix::is_system_family(full);
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
///
/// Hledá se jen v klíči, ze kterého položka podle zdroje pochází — dřív
/// u `run_machine` prošly všechny čtyři klíče a validace tak schválila
/// a posoudila i 32bitovou položku stejného jména.
fn run_value(name: &str, source: &str) -> Option<String> {
    use win_sys::registry::{enum_values, RegKey, HKEY_LOCAL_MACHINE, HKEY_USERS};
    const RUN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
    const RUN_ONCE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce";
    const RUN_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
    let (machine, subs): (bool, &[&str]) = match source {
        "run_machine" => (true, &[RUN]),
        "run_machine32" => (true, &[RUN_32]),
        "run_once_machine" => (true, &[RUN_ONCE]),
        "run_user" => (false, &[RUN]),
        "run_once_user" => (false, &[RUN_ONCE]),
        _ => return None,
    };
    let mut roots: Vec<(RegKey, String)> = Vec::new();
    if machine {
        for s in subs {
            roots.push((HKEY_LOCAL_MACHINE, (*s).to_string()));
        }
    } else {
        for sid in win_sys::consent::user_hives() {
            for s in subs {
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

/// Kořeny Uninstall klíčů: HKLM (64 i 32 bitů) a hive každého
/// přihlášeného uživatele přes HKEY_USERS. HKEY_CURRENT_USER by byla
/// hive SYSTEMu — služba pod ním běží.
fn uninstall_roots() -> Vec<(win_sys::registry::RegKey, String)> {
    use win_sys::registry::{enum_subkeys, HKEY_LOCAL_MACHINE, HKEY_USERS};
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
            // Stejné kořeny jako inventář (collector-inv), ať příznaky
            // dostane každý program, který je v seznamu.
            for w in ["", r"\WOW6432Node"] {
                roots.push((
                    HKEY_USERS,
                    format!(r"{sid}\SOFTWARE{w}\Microsoft\Windows\CurrentVersion\Uninstall"),
                ));
            }
        }
    }
    roots
}

/// Je záznam v Uninstall jen AKTUALIZACÍ jiného programu? Windows ho
/// pozná podle `ParentKeyName` (patří pod jiný záznam) nebo `ReleaseType`;
/// starší instalátory jen připíšou „(KB…)" ke jménu. Odinstalovat ji
/// zvlášť buď nejde, nebo to rozbije program, ke kterému patří.
pub fn is_update_entry(
    display_name: &str,
    parent_key: Option<&str>,
    release_type: Option<&str>,
) -> bool {
    if parent_key.is_some_and(|p| !p.trim().is_empty()) {
        return true;
    }
    if release_type.is_some_and(|r| {
        matches!(
            r.trim().to_ascii_lowercase().as_str(),
            "update" | "security update" | "hotfix" | "service pack"
        )
    }) {
        return true;
    }
    has_kb_tag(display_name)
}

/// „(KB1234567)" kdekoli ve jménu.
fn has_kb_tag(name: &str) -> bool {
    let lc = name.to_ascii_lowercase();
    let mut rest = lc.as_str();
    while let Some(i) = rest.find("(kb") {
        let after = &rest[i + 3..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && after.as_bytes().get(digits) == Some(&b')') {
            return true;
        }
        rest = after;
    }
    false
}

/// Proč se program odsud odinstalovat NESMÍ, nebo `None`. Čte se
/// čerstvě z registru, UI posílá jen jméno.
///
/// - `NoRemove=1`: Windows samy odinstalaci v Nastavení nenabídnou
///   (ESET, doplňky Office) — program se odebírá jinou cestou;
/// - aktualizace jiného programu (`is_update_entry`).
fn uninstall_lock_reason(display_name: &str) -> Option<String> {
    use win_sys::registry::{enum_subkeys, read_string, read_u64};
    let want = display_name.trim().to_ascii_lowercase();
    for (root, base) in uninstall_roots() {
        for sub in enum_subkeys(root, &base) {
            let key = format!("{base}{}{sub}", char::from(92u8));
            let Some(name) = read_string(root, &key, "DisplayName") else {
                continue;
            };
            if name.trim().to_ascii_lowercase() != want {
                continue;
            }
            if read_u64(root, &key, "NoRemove") == Some(1) {
                return Some(format!(
                    "„{}“ odinstalaci nedovoluje (NoRemove) — Windows ji nenabízejí ani v Nastavení; odebírá se instalátorem toho programu",
                    name.trim()
                ));
            }
            let parent = read_string(root, &key, "ParentKeyName");
            let release = read_string(root, &key, "ReleaseType");
            if is_update_entry(&name, parent.as_deref(), release.as_deref()) {
                return Some(format!(
                    "„{}“ je aktualizace jiného programu — odebírá se s ním, ne samostatně",
                    name.trim()
                ));
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

// ── Mazání do koše: co a jak (v11, SPEC 18.2) ───────────────────────
//
// Kontrola je řetězcová, takže má smysl jen nad JEDNOZNAČNOU cestou.
// Dřív ji šlo obejít třemi způsoby a všechny prošly: `..` (výjimka pro
// Temp: `…\Temp\..\..\..\Windows\System32\kernel32.dll`), krátká jména
// 8.3 (`C:\PROGRA~1`) a junction v Tempu mířící kamkoli. Proto nejdřív
// kanonický tvar a teprve potom seznam míst, kam se nesahá.

/// Ověří cesty ke smazání a vrátí jejich KANONICKÝ tvar (plný, dlouhá
/// jména). Smazat se smí jen přesně tenhle tvar — co se ověřilo, to se
/// smaže, nic jiného. První problém = zamítnutí s důvodem.
pub fn delete_targets(paths: &[String]) -> Result<Vec<String>, String> {
    let mista = Mista::ze_systemu();
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let canon = delete_target(p, &mista)?;
        // Kritický držitel (Restart Manager) akci zamyká.
        if let Ok(hs) = win_sys::rm::holders(std::slice::from_ref(&canon)) {
            if let Some(h) = hs
                .iter()
                .find(|h| h.kind == win_sys::rm::HolderKind::Critical)
            {
                return Err(format!(
                    "soubor drží kritický systémový proces {} (pid {})",
                    h.name, h.pid
                ));
            }
        }
        if !out.iter().any(|o: &String| o.eq_ignore_ascii_case(&canon)) {
            out.push(canon);
        }
    }
    Ok(out)
}

/// Jedna cesta: tvar → existence → kanonický tvar → odkazy → jen
/// soubor → chráněná místa.
fn delete_target(raw: &str, mista: &Mista) -> Result<String, String> {
    let p = raw.trim();
    if p.is_empty() {
        return Err("prázdná cesta".into());
    }
    // NUL by v seznamu pro koš (položky dělí právě NUL) udělal z jedné
    // cesty dvě.
    if p.contains('\0') {
        return Err("cesta obsahuje neplatný znak".into());
    }
    // Wildcardy sem nepatří — cíl musí být jednoznačný, ne něco, co se
    // doexpanduje jinde.
    if p.contains('*') || p.contains('?') {
        return Err("zástupné znaky nejsou povolené".into());
    }
    // `\\?\` a `\\.\` vypínají normalizaci cest Win32 (a s ní všechna
    // pravidla níž), UNC by přimělo službu sahat na cizí stroj.
    if p.starts_with(r"\\") || p.starts_with("//") || p.starts_with(r"\??\") {
        return Err(format!(
            "cesty se speciální předponou (\\\\?\\, \\\\.\\, síťové) se nemažou: {p}"
        ));
    }
    let b = p.as_bytes();
    if !(b.len() > 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/'))
    {
        return Err(format!("cesta není absolutní: {p}"));
    }
    // Dvojtečka za písmenem disku = alternativní datový proud souboru.
    if p[2..].contains(':') {
        return Err(format!("alternativní datové proudy se nemažou: {p}"));
    }
    // `.` a `..` (i „...", „. .": Windows z nich koncové tečky a mezery
    // odřežou) — cesta musí vést rovnou, ne oklikou.
    if p[3..]
        .split(['\\', '/'])
        .any(|c| !c.is_empty() && c.trim_end_matches(['.', ' ']).is_empty())
    {
        return Err(format!("cesta obsahuje „.“ nebo „..“: {p}"));
    }
    // ČERSTVÁ kontrola existence (SPEC 17.3) — UI mohlo ukazovat
    // starý stav.
    if !std::path::Path::new(p).exists() {
        return Err(format!("už neexistuje: {p}"));
    }
    let canon = win_sys::fspath::full_long_path(p)
        .ok_or_else(|| format!("cestu nejde jednoznačně ověřit: {p}"))?;
    let cb = canon.as_bytes();
    if !(cb.len() > 3 && cb[0].is_ascii_alphabetic() && cb[1] == b':' && cb[2] == b'\\') {
        return Err(format!("cestu nejde jednoznačně ověřit: {p}"));
    }
    odkaz_v_ceste(&canon)?;
    match std::fs::symlink_metadata(&canon) {
        Ok(m) if m.is_dir() => {
            return Err(format!(
                "složky se nemažou — jen jednotlivé soubory: {canon}"
            ))
        }
        Ok(_) => {}
        Err(_) => return Err(format!("cestu nejde přečíst: {canon}")),
    }
    if let Some(why) = protected_path(&canon, mista) {
        return Err(why);
    }
    Ok(canon)
}

/// Vede cesta přes odkaz (junction, symlink, přípojný bod)? Kontroluje
/// se soubor i každá složka nad ním. Cloudové soubory (OneDrive)
/// a deduplikace jsou reparse pointy taky, ale nikam nepřesměrovávají.
fn odkaz_v_ceste(canon: &str) -> Result<(), String> {
    use win_sys::fspath::{reparse, tag_neodkazuje, Odkaz};
    let mut cur = canon.trim_end_matches(char::from(92u8));
    // Kořen svazku („C:") se nekontroluje — tam cesta začíná.
    while cur.len() > 2 {
        match reparse(cur) {
            Odkaz::Zadny => {}
            Odkaz::Tag(t) if tag_neodkazuje(t) => {}
            Odkaz::Tag(_) => {
                return Err(format!(
                    "cesta vede přes odkaz (junction/symlink), Winsent přes něj nemaže: {cur}"
                ))
            }
            Odkaz::Necitelne => return Err(format!("cestu nejde přečíst: {cur}")),
        }
        match cur.rsplit_once(char::from(92u8)) {
            Some((parent, _)) => cur = parent,
            None => break,
        }
    }
    Ok(())
}

/// Místa, kam se nesahá — přečtená ze systému JEDNOU pro celé mazání.
/// Vše malými písmeny, bez koncového lomítka.
#[derive(Debug, Clone, Default)]
struct Mista {
    sysroot: String,
    programdata: String,
    /// %ProgramFiles%, %ProgramFiles(x86)%, %ProgramW6432% (pro případ,
    /// že neleží v kořeni disku — v kořeni je chytá obecné pravidlo).
    program_files: Vec<String>,
    /// Profily uživatelů z ProfileList.
    profily: Vec<String>,
    /// Kde bydlí Winsent.
    own_dirs: Vec<String>,
}

impl Mista {
    fn ze_systemu() -> Mista {
        use win_sys::registry::{enum_subkeys, read_string, HKEY_LOCAL_MACHINE};
        let env_lc = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim_end_matches(char::from(92u8)).to_ascii_lowercase())
                .filter(|v| !v.is_empty())
        };
        const PL: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
        let profily = enum_subkeys(HKEY_LOCAL_MACHINE, PL)
            .into_iter()
            .filter_map(|sid| {
                read_string(
                    HKEY_LOCAL_MACHINE,
                    &format!(r"{PL}\{sid}"),
                    "ProfileImagePath",
                )
            })
            .map(|p| p.trim_end_matches(char::from(92u8)).to_ascii_lowercase())
            .filter(|p| p.len() > 3)
            .collect();
        Mista {
            sysroot: system_root().to_ascii_lowercase(),
            programdata: env_lc("ProgramData").unwrap_or_else(|| r"c:\programdata".into()),
            program_files: ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
                .iter()
                .filter_map(|k| env_lc(k))
                .collect(),
            profily,
            own_dirs: own_dirs(),
        }
    }
}

/// Složky v kořeni KAŽDÉHO svazku, do kterých se nesahá, s důvodem.
const ROOT_DIRS: &[(&str, &str)] = &[
    (
        "program files",
        "nainstalované programy — odebírají se odinstalací",
    ),
    (
        "program files (x86)",
        "nainstalované programy — odebírají se odinstalací",
    ),
    (
        "windowsapps",
        "aplikace ze Storu — odebírají se v Nastavení",
    ),
    ("efi", "zavaděč systému"),
    ("boot", "zavaděč systému"),
    ("recovery", "prostředí pro obnovení Windows"),
    (
        "config.msi",
        "rozpracovaná instalace — spravuje Instalační služba Windows",
    ),
    (
        "system volume information",
        "body obnovení a stínové kopie — spravuje Ochrana systému",
    ),
    ("$recycle.bin", "koš — místo uvolní vysypání koše"),
    (
        "$windows.~bt",
        "instalační soubory Windows — uklidí je Vyčištění disku",
    ),
    (
        "$windows.~ws",
        "instalační soubory Windows — uklidí je Vyčištění disku",
    ),
    (
        "$getcurrent",
        "instalační soubory Windows — uklidí je Vyčištění disku",
    ),
    (
        "windows.old",
        "předchozí Windows — uklidí je Úložiště nebo Vyčištění disku",
    ),
    (
        "$winreagent",
        "aktualizace prostředí pro obnovení — uklidí je Vyčištění disku",
    ),
    (
        "$sysreset",
        "soubory Obnovení počítače — uklidí je Vyčištění disku",
    ),
    ("esd", "instalační soubory Windows pro Obnovení počítače"),
    ("perflogs", "systémová složka Windows"),
    (
        "msocache",
        "instalační mezipaměť Office — bez ní nepůjde Office opravit",
    ),
];

/// Systémové soubory přímo v kořeni svazku.
const ROOT_FILES: &[&str] = &[
    "pagefile.sys",
    "swapfile.sys",
    "hiberfil.sys",
    "bootmgr",
    "bootnxt",
    "boottel.dat",
    "bootsect.bak",
    "$winre_backup_partition.marker",
];

/// Kde se nemaže, i když to je obyčejný soubor (SPEC 18.2). `canon` je
/// KANONICKÁ cesta (`delete_target`); vrací důvod zamítnutí, nebo `None`.
///
/// Nadmnožina toho, co UI označí jako součást Windows, spravované
/// Windows, kontejner a data programu: UI je zdvořilost, tohle je
/// poslední slovo — do pipe může psát kterýkoli přihlášený uživatel.
fn protected_path(canon: &str, m: &Mista) -> Option<String> {
    let p = canon
        .replace('/', "\\")
        .trim_end_matches(char::from(92u8))
        .to_ascii_lowercase();
    // Kořen svazku („c:", „c:\") — smazat disk nelze ani omylem.
    if p.len() <= 3 {
        return Some("kořen disku nelze smazat".into());
    }
    let jmeno = p.rsplit(char::from(92u8)).next().unwrap_or_default();
    // Části za „x:\".
    let casti: Vec<&str> = p[3..].split(char::from(92u8)).collect();

    if under(&p, &m.sysroot) {
        return Some(format!(
            "soubor patří Windows — ve složce Windows Winsent nic nemaže: {canon}"
        ));
    }
    if let Some((_, why)) = ROOT_DIRS.iter().find(|(d, _)| casti[0] == *d) {
        if casti.len() > 1 {
            return Some(format!("{why}: {canon}"));
        }
    }
    if m.program_files.iter().any(|pf| under(&p, pf)) {
        return Some(format!(
            "nainstalované programy — odebírají se odinstalací: {canon}"
        ));
    }
    if casti.len() == 1 && (ROOT_FILES.contains(&jmeno) || jmeno.starts_with("dumpstack.log")) {
        return Some(format!("systémový soubor v kořeni disku: {canon}"));
    }
    let pd = &m.programdata;
    for (sub, why) in [
        (
            "package cache",
            "mezipaměť instalátorů — bez ní nepůjdou programy opravit ani odinstalovat",
        ),
        ("microsoft", "data Windows a Microsoft Defenderu"),
        ("syswatch", "data Winsentu"),
        (r"nvidia corporation\drs", "nastavení ovladače NVIDIA"),
        (
            r"nvidia\ngx\models",
            "modely DLSS — spravuje ovladač NVIDIA",
        ),
    ] {
        if under(&p, &format!(r"{pd}\{sub}")) {
            return Some(format!("{why}: {canon}"));
        }
    }
    if m.own_dirs.iter().any(|d| under(&p, d)) {
        return Some(format!("Winsent sám sebe nemaže: {canon}"));
    }
    // Registr uživatele: hive a její protokoly.
    if jmeno.starts_with("ntuser.dat") || jmeno.starts_with("usrclass.dat") {
        return Some(format!(
            "registr uživatele — bez něj se profil nenačte: {canon}"
        ));
    }
    if matches!(jmeno, "syswatch.db" | "syswatch.db-wal" | "syswatch.db-shm") {
        return Some(format!("databáze Winsentu: {canon}"));
    }
    // Ovladač mimo Windows patří programu, který ho nainstaloval.
    if jmeno.ends_with(".sys") {
        return Some(format!(
            "ovladač — odebírá se odinstalací programu, ne smazáním: {canon}"
        ));
    }
    // Profily: soubory přímo v kořeni profilu (ntuser.ini, …) a šablona
    // pro nové účty.
    let rodic = p
        .rsplit_once(char::from(92u8))
        .map(|(r, _)| r)
        .unwrap_or_default();
    let v_koreni_profilu =
        m.profily.iter().any(|pr| pr == rodic) || (casti.len() <= 3 && casti[0] == "users");
    if v_koreni_profilu {
        return Some(format!(
            "soubor přímo ve složce profilu patří Windows: {canon}"
        ));
    }
    if casti.len() > 2 && casti[0] == "users" && casti[1] == "default" {
        return Some(format!("šablona profilu pro nové účty: {canon}"));
    }
    // Nainstalované hry ze Steamu se odebírají ve Steamu.
    if p.contains(r"\steamapps\common\") {
        return Some(format!(
            "nainstalovaná hra — odebírá se odinstalací ve Steamu: {canon}"
        ));
    }
    None
}

/// Složky, které úklid po programu (PurgeGhost) nikdy neodstraní, ani
/// prázdné. Na rozdíl od mazání souborů tu jde o vlastní prázdné složky
/// programů pod Program Files — ty smazat smí; nesmí jen samotné
/// systémové kořeny.
fn purge_protected_path(path: &str) -> Option<String> {
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
    fn startup_vlastni_sluzba_denied() {
        // Jedno kliknutí by Winsentu vyplo vlastní automatický start.
        // Služba by běžela dál, takže by se to poznalo až po restartu
        // počítače — a to je přesně ten druh závady, který se nedá
        // vystopovat zpětně.
        let mut ctx = LiveContext::new();
        for id in ["service|syswatch", "service|SysWatch"] {
            let v = validate(
                &Action::StartupToggle {
                    id: id.into(),
                    on: false,
                },
                &mut ctx,
            );
            assert!(matches!(v, Verdict::Deny { .. }), "{id} musí být zamítnut");
        }
    }
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
        use win_sys::procinfo::is_critical_name;
        for n in [
            "lsass.exe",
            "csrss.exe",
            "WinLogon.exe",
            "System",
            "Secure System",
        ] {
            assert!(is_critical_name(n), "{n} musí být na tvrdém seznamu");
        }
        for n in ["notepad.exe", "chrome.exe", ""] {
            assert!(!is_critical_name(n), "{n} na seznamu být nemá");
        }
    }

    // ── Kill (v11): co Winsent neukončuje ──

    /// Živý proces podle jména (první nalezený) — (pid, jméno).
    fn zivy(jmeno: &str) -> Option<(u32, i64, String)> {
        let mut buf = Vec::new();
        win_sys::proc::snapshot_processes(&mut buf)
            .ok()?
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(jmeno))
            .map(|p| (p.pid, p.create_time, p.name))
    }

    // Procesy Windows mimo seznam kritických jmen (explorer, svchost,
    // dwm…) se neukončují — dřív prošly všechny kromě čtrnácti.
    #[test]
    fn kill_windows_procesu_zamitnut() {
        let rules = KillRules::fresh();
        let mut ctx = LiveContext::new();
        for jm in [
            "explorer.exe",
            "svchost.exe",
            "dwm.exe",
            "sihost.exe",
            "ctfmon.exe",
        ] {
            let Some((pid, ct, name)) = zivy(jm) else {
                continue;
            };
            assert!(
                kill_deny_reason(pid, &name, &rules).is_some(),
                "{name} ({pid}) se nesmí dát ukončit"
            );
            let v = validate(
                &Action::KillProc {
                    pid,
                    create_time: ct,
                    tree: false,
                },
                &mut ctx,
            );
            assert!(matches!(v, Verdict::Deny { .. }), "{name}: {v:?}");
        }
    }

    // Uživatelský nástroj ze složky Windows ukončit JDE (allow-list) —
    // a obyčejný program mimo Windows taky.
    #[test]
    fn kill_uzivatelsky_nastroj_povolen() {
        use std::os::windows::process::CommandExt;
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let exe = std::path::Path::new(&windir).join(r"System32\cmd.exe");
        // CREATE_NO_WINDOW: test nesmí na ploše probliknout konzolí.
        let Ok(mut child) = std::process::Command::new(&exe)
            .args(["/c", "ping", "-n", "20", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
        else {
            return;
        };
        let rules = KillRules::fresh();
        let cmd = kill_deny_reason(child.id(), "cmd.exe", &rules);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(cmd, None, "cmd.exe je na allow-listu");
        // PING.EXE ze System32 na allow-listu není.
        let ping = std::path::Path::new(&windir).join(r"System32\PING.EXE");
        if let Ok(mut c) = std::process::Command::new(&ping)
            .args(["-n", "20", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
        {
            let v = kill_deny_reason(c.id(), "PING.EXE", &rules);
            let _ = c.kill();
            let _ = c.wait();
            assert!(v.is_some(), "PING.EXE ze System32 se neukončuje");
        }
    }

    #[test]
    fn kill_pseudoprocesy_a_jadro_zamitnuty() {
        let rules = KillRules::fresh();
        for jm in ["Registry", "Memory Compression", "Secure System", "System"] {
            if let Some((pid, _, name)) = zivy(jm) {
                assert!(kill_deny_reason(pid, &name, &rules).is_some(), "{name}");
            }
        }
        assert!(kill_deny_reason(0, "Idle", &rules).is_some());
        assert!(kill_deny_reason(std::process::id(), "x.exe", &rules).is_some());
    }

    // „Součást Windows" = složka Windows A podpis Microsoftu, stejně jako
    // identita os:windows, podle které UI ukončení vůbec nenabídne.
    // Cizí nebo nepodepsaná binárka ve složce Windows tím pravidlem
    // neprojde — UI ji nabídne (u služby s varováním) a služba to nesmí
    // zamítnout s nepravdou „součást Windows".
    #[test]
    fn kill_windows_jen_s_podpisem_microsoftu() {
        let windir = system_root();
        let lc = |p: &std::path::Path| p.to_string_lossy().to_ascii_lowercase();

        // Náhradní „složka Windows" v tempu: kopii systémové binárky
        // katalog Windows pozná podle otisku, nepodepsanou ne.
        let koren = std::env::temp_dir().join(format!("ws-kill-sysroot-{}", std::process::id()));
        std::fs::create_dir_all(&koren).expect("adresář");
        let rules = KillRules {
            sysroot: lc(&koren),
            ..KillRules::default()
        };
        let ms = koren.join("svchost.exe");
        let zkopirovano = std::fs::copy(format!(r"{windir}\System32\svchost.exe"), &ms).is_ok();
        let cizi = koren.join("x.exe");
        std::fs::write(&cizi, b"MZ neni to program").expect("zapsat");
        let ms_win = zkopirovano && windows_image(&ms.to_string_lossy(), &lc(&ms), &rules);
        let cizi_win = windows_image(&cizi.to_string_lossy(), &lc(&cizi), &rules);
        // Mimo (náhradní) složku Windows se podpis ani nezkoumá.
        let svchost = format!(r"{windir}\System32\svchost.exe");
        let venku = windows_image(&svchost, &svchost.to_ascii_lowercase(), &rules);
        let _ = std::fs::remove_dir_all(&koren);
        assert!(!zkopirovano || ms_win, "soubor Windows podepsaný Microsoftem");
        assert!(!cizi_win, "nepodepsaná binárka ve složce Windows");
        assert!(!venku, "mimo složku Windows");

        // Skutečný DriverStore: NVIDIA Display Container podepisuje
        // výrobce (na stroji bez NVIDIA se přeskočí).
        let rules = KillRules {
            sysroot: windir.to_ascii_lowercase(),
            ..KillRules::default()
        };
        let repo = std::path::Path::new(&windir).join(r"System32\DriverStore\FileRepository");
        let nv = std::fs::read_dir(&repo)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("nv"))
            .map(|e| e.path().join(r"Display.NvContainer\NVDisplay.Container.exe"))
            .find(|p| p.exists());
        if let Some(nv) = nv {
            assert!(
                !windows_image(&nv.to_string_lossy(), &lc(&nv), &rules),
                "{} není soubor Windows",
                nv.display()
            );
        }
        // A živý proces: když jde přečíst, odkud běží, nesmí dostat
        // „součást Windows".
        if let Some((pid, _, name)) = zivy("NVDisplay.Container.exe") {
            let v = kill_deny_reason(pid, &name, &KillRules::fresh());
            assert_ne!(
                v.as_deref(),
                Some(format!("{name}: součást Windows — Winsent ji neukončuje").as_str()),
            );
        }
    }

    // Bezpečnostní software a vlastní binárky Winsentu podle cesty.
    #[test]
    fn kill_pravidla_podle_cesty() {
        let rules = KillRules {
            sysroot: r"c:\windows".into(),
            security_dirs: vec![r"c:\program files\eset\eset security".into()],
            own_dirs: vec![r"c:\program files\winsent".into()],
        };
        assert!(under(
            r"c:\program files\eset\eset security\ekrn.exe",
            &rules.security_dirs[0]
        ));
        assert!(!under(
            r"c:\program files\eset\eset security2\x.exe",
            &rules.security_dirs[0]
        ));
        assert!(!under(
            r"c:\program files\eset\eset security",
            &rules.security_dirs[0]
        ));
        assert!(component_zone(
            r"c:\programdata\microsoft\windows defender\platform\4.18\msmpeng.exe"
        ));
        assert!(USER_TOOLS.contains(&"notepad.exe") && !USER_TOOLS.contains(&"conhost.exe"));
        // Winsent jen podle jména A místa.
        assert!(winsent_image(
            r"c:\program files\winsent\syswatch-ui.exe",
            &rules
        ));
        assert!(winsent_image(
            r"c:\program files\winsent\syswatch.exe",
            &rules
        ));
        assert!(!winsent_image(r"c:\users\x\downloads\syswatch.exe", &rules));
        assert!(!winsent_image(r"c:\program files\winsent\jiny.exe", &rules));
    }

    // ── Mazání (v11): kanonický tvar a pokusy o obejití ──

    fn mista_testovaci() -> Mista {
        Mista {
            sysroot: r"c:\windows".into(),
            programdata: r"c:\programdata".into(),
            program_files: vec![r"c:\program files".into(), r"c:\program files (x86)".into()],
            profily: vec![r"c:\users\iva".into(), r"d:\profily\pepa".into()],
            own_dirs: vec![r"c:\program files\winsent".into()],
        }
    }

    #[test]
    fn chranena_mista_zamitnuta() {
        let m = mista_testovaci();
        for p in [
            r"C:\Windows\explorer.exe",
            r"C:\Windows\Installer\abc.msi",
            r"C:\Windows\Temp\x.tmp",
            r"C:\Program Files\NVIDIA Corporation\Installer2\a.dll",
            r"D:\Program Files (x86)\Hra\hra.exe",
            r"D:\WindowsApps\Foo_1.0_x64__abc\app.exe",
            r"C:\Program Files\WindowsApps\Foo\app.exe",
            r"C:\ProgramData\Package Cache\{GUID}\vc_redist.exe",
            r"C:\ProgramData\Microsoft\Search\Data\Applications\Windows\Windows.edb",
            r"C:\ProgramData\syswatch\syswatch.db",
            r"E:\data\syswatch.db-wal",
            r"C:\ProgramData\NVIDIA Corporation\Drs\nvdrsdb0.bin",
            r"C:\pagefile.sys",
            r"D:\hiberfil.sys",
            r"C:\DumpStack.log.tmp",
            r"C:\bootTel.dat",
            r"C:\$WINRE_BACKUP_PARTITION.MARKER",
            r"C:\EFI\Microsoft\Boot\bootmgfw.efi",
            r"C:\Recovery\WindowsRE\Winre.wim",
            r"C:\Config.Msi\123.rbf",
            r"C:\System Volume Information\{3808876b}",
            r"D:\$Recycle.Bin\S-1-5-18\$R1.txt",
            r"C:\$WINDOWS.~BT\Sources\install.esd",
            r"C:\Windows.old\Users\a.txt",
            r"C:\$WinREAgent\x.tmp",
            r"C:\ESD\Windows\x.esd",
            r"C:\MSOCache\All Users\x.cab",
            r"C:\Users\IVA\NTUSER.DAT",
            r"C:\Users\IVA\ntuser.dat.LOG1",
            r"C:\Users\IVA\AppData\Local\Microsoft\Windows\UsrClass.dat",
            r"C:\Users\IVA\ntuser.ini",
            r"C:\Users\desktop.ini",
            r"D:\Profily\Pepa\soubor.txt",
            r"C:\Users\Default\NTUSER.DAT",
            r"C:\Users\Default\AppData\Local\x.txt",
            r"D:\SteamLibrary\steamapps\common\Hra\hra.pak",
            r"C:\Program Files\Winsent\syswatch.exe",
            r"D:\Ovladace\vgk.sys",
        ] {
            assert!(protected_path(p, &m).is_some(), "{p} musí být chráněné");
        }
    }

    #[test]
    fn bezne_soubory_projdou() {
        let m = mista_testovaci();
        for p in [
            r"C:\Users\IVA\Downloads\setup.exe",
            r"C:\Users\IVA\Documents\a.pdf",
            r"C:\Users\IVA\AppData\Local\Temp\x.tmp",
            r"C:\Users\IVA\AppData\Local\NVIDIA\DXCache\abc.nvph",
            r"D:\Hry\obraz.iso",
            r"D:\Windows\stara-instalace.txt",
            r"C:\ProgramData\NVIDIA Corporation\Downloader\x.exe",
        ] {
            assert_eq!(protected_path(p, &m), None, "{p} chráněné být nemá");
        }
    }

    #[test]
    fn tvar_cesty_bez_obejiti() {
        let m = Mista::ze_systemu();
        let tmp = std::env::temp_dir();
        let f = tmp.join("winsent-validate-tvar.tmp");
        std::fs::write(&f, b"x").expect("zapsat testovací soubor");
        let fs = f.to_string_lossy().into_owned();
        let sysroot = system_root();
        let kernel = format!(r"{sysroot}\System32\kernel32.dll");
        // `..` z Tempu do System32 — dřív to prošlo výjimkou pro \temp\.
        let pres_tecky = format!(
            r"{}\..\..\..\..\..\..\..\{}",
            tmp.to_string_lossy().trim_end_matches('\\'),
            &kernel[3..]
        );
        for p in [
            pres_tecky,
            format!(r"\\?\{fs}"),
            format!(r"\\?\{kernel}"),
            format!(r"\\.\{fs}"),
            format!(r"\\localhost\C$\{}", &fs[3..]),
            format!(r"{fs}:stream"),
            format!(r"{fs}\."),
            format!(
                r"{}\.\winsent-validate-tvar.tmp",
                tmp.to_string_lossy().trim_end_matches('\\')
            ),
            format!(r"{}\...\x", tmp.to_string_lossy().trim_end_matches('\\')),
            "C:relativni.txt".to_string(),
            "C:\\a\0C:\\Windows\\win.ini".to_string(),
        ] {
            assert!(delete_target(&p, &m).is_err(), "{p} mělo být zamítnuto");
        }
        // Složka se nemaže, i když je v Tempu.
        assert!(delete_target(&tmp.to_string_lossy(), &m).is_err(), "složka");
        // Krátké jméno se rozvine — ověřuje a vrací se dlouhý tvar.
        let canon = delete_target(&fs, &m).expect("běžný temp soubor projde");
        if let Some(short) = win_sys::fspath::short_path(&fs) {
            let z_kratke = delete_target(&short, &m).expect("krátké jméno téhož souboru");
            assert!(
                z_kratke.eq_ignore_ascii_case(&canon),
                "{short} → {z_kratke} ≠ {canon}"
            );
        }
        let _ = std::fs::remove_file(&f);
    }

    // 8.3 jméno souboru v Program Files musí skončit stejně jako dlouhé.
    #[test]
    fn kratke_jmeno_neobejde_program_files() {
        let m = Mista::ze_systemu();
        let Ok(pf) = std::env::var("ProgramFiles") else {
            return;
        };
        // První soubor ve dvou úrovních pod Program Files.
        let soubor = std::fs::read_dir(&pf).ok().and_then(|rd| {
            rd.flatten().filter(|e| e.path().is_dir()).find_map(|d| {
                std::fs::read_dir(d.path())
                    .ok()?
                    .flatten()
                    .find(|e| e.path().is_file())
                    .map(|e| e.path().to_string_lossy().into_owned())
            })
        });
        let Some(soubor) = soubor else { return };
        assert!(delete_target(&soubor, &m).is_err(), "{soubor}");
        if let Some(short) = win_sys::fspath::short_path(&soubor) {
            assert!(delete_target(&short, &m).is_err(), "8.3 {short} prošlo");
        }
    }

    // Junction v Tempu: cokoli za ním se nemaže, ať míří kamkoli.
    #[test]
    fn junction_v_ceste_zamitnut() {
        use std::os::windows::process::CommandExt;
        let m = Mista::ze_systemu();
        let tmp = std::env::temp_dir();
        let id = std::process::id();
        let cil = tmp.join(format!("winsent-jcil-{id}"));
        let odkaz = tmp.join(format!("winsent-jodkaz-{id}"));
        let _ = std::fs::create_dir_all(&cil);
        std::fs::write(cil.join("soubor.txt"), b"x").expect("zapsat");
        let ok = std::process::Command::new("cmd")
            .arg("/c")
            .arg("mklink")
            .arg("/J")
            .arg(&odkaz)
            .arg(&cil)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let pres = odkaz.join("soubor.txt").to_string_lossy().into_owned();
        let primo = cil.join("soubor.txt").to_string_lossy().into_owned();
        let r_pres = ok.then(|| delete_target(&pres, &m));
        let r_primo = delete_target(&primo, &m);
        // Úklid: remove_dir na junction odstraní jen odkaz, ne cíl.
        let _ = std::fs::remove_dir(&odkaz);
        let _ = std::fs::remove_file(cil.join("soubor.txt"));
        let _ = std::fs::remove_dir(&cil);
        if let Some(r) = r_pres {
            assert!(r.is_err(), "cesta přes junction prošla: {r:?}");
        }
        assert!(r_primo.is_ok(), "přímá cesta má projít: {r_primo:?}");
    }

    // ── Odinstalace a po spuštění (v11) ──

    #[test]
    fn aktualizace_se_poznaji() {
        assert!(is_update_entry(
            "Security Update for Microsoft Office 2007 (KB2596754) 32-Bit Edition",
            None,
            None
        ));
        assert!(is_update_entry("Cokoli", Some("ENTERPRISE"), None));
        assert!(is_update_entry("Cokoli", None, Some("Security Update")));
        assert!(is_update_entry("Cokoli", None, Some("hotfix")));
        assert!(!is_update_entry(
            "Microsoft Visual C++ 2015 Redistributable (x64)",
            None,
            None
        ));
        assert!(!is_update_entry(
            "Hra (KB edice)",
            Some("  "),
            Some("Product")
        ));
        assert!(!is_update_entry("Program (kb)", None, None));
    }

    #[test]
    fn upozorneni_po_spusteni() {
        assert!(startup_caution("service|ClickToRunSvc", None).is_some());
        assert!(startup_caution("service|edgeupdate", None).is_some());
        assert!(startup_caution("service|edgeupdatem", None).is_some());
        assert!(startup_caution("service|GameInputRedistService", None).is_some());
        assert!(startup_caution(r"task|\MicrosoftEdgeUpdateTaskMachineCore", None).is_some());
        assert!(startup_caution(
            "run_machine|RtkAudUService",
            Some(r#""C:\Windows\System32\DriverStore\FileRepository\hdx.inf_amd64_1\RtkAudUService64.exe" -background"#),
        )
        .is_some());
        assert!(startup_caution("run_user|Discord", Some(r"C:\Apps\Discord\Update.exe")).is_none());
    }

    #[test]
    fn bezpecnostni_software_po_spusteni() {
        let dirs = vec![r"c:\program files\eset\eset security".to_string()];
        assert_eq!(
            security_startup_reason(
                "run_machine|egui",
                Some(r#""C:\Program Files\ESET\ESET Security\ecmds.exe" /run /hide /proxy"#),
                &dirs
            )
            .as_deref(),
            Some(SECURITY_STARTUP_REASON)
        );
        assert!(security_startup_reason(
            "run_machine|jiny",
            Some(r#""C:\Program Files\ESET\Jiny\x.exe""#),
            &dirs
        )
        .is_none());
        assert!(security_startup_reason("run_machine|egui", Some("x.exe"), &[]).is_none());
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

/// Co o programu říká jeho záznam v Uninstall klíčích.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UninstallInfo {
    /// Binárka odinstalátoru; `None` = záznam bez odinstalačního příkazu.
    pub exe: Option<String>,
    /// `NoRemove=1` — Windows odinstalaci nenabízejí.
    pub no_remove: bool,
    /// Aktualizace jiného programu (`is_update_entry`).
    pub is_update: bool,
}

/// Mapa `DisplayName (malými) → UninstallInfo`, přečtená JEDNÍM
/// průchodem Uninstall klíči.
///
/// `uninstall_command` prochází celý registr při každém volání, takže
/// pro celý inventář (stovky aplikací) by to byla kvadratická práce.
/// Tohle je tentýž zdroj, jen přečtený najednou. Víc záznamů téhož
/// jména se slučuje: příznak platí, když ho má kterýkoli z nich.
pub fn uninstall_index() -> std::collections::HashMap<String, UninstallInfo> {
    use win_sys::registry::{enum_subkeys, read_string, read_u64};
    let mut out: std::collections::HashMap<String, UninstallInfo> =
        std::collections::HashMap::new();
    for (root, base) in uninstall_roots() {
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
            let parent = read_string(root, &key, "ParentKeyName");
            let release = read_string(root, &key, "ReleaseType");
            let info = out.entry(name.trim().to_ascii_lowercase()).or_default();
            if info.exe.is_none() {
                info.exe = exe;
            }
            info.no_remove |= read_u64(root, &key, "NoRemove") == Some(1);
            info.is_update |= is_update_entry(&name, parent.as_deref(), release.as_deref());
        }
    }
    out
}
