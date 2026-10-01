//! collector-boot — startup položky, čtení 6 backendů (SPEC kap. 7).
//!
//! „Co startuje s Windows a kdo to spouští." Zdroje: Run klíče,
//! Startup složky, Task Scheduler, služby, MSIX startup tasks a shell
//! rozšíření (Userinit/Shell — jen varování, nikdy nepřepínat).
//!
//! Stav zapnuto/vypnuto se čte ze `StartupApproved` (tak to dělá
//! i Správce úloh). Tahle crate JEN ČTE — zápis patří actor-toggle
//! za validační vrstvou.

use core_types::config::Config;

/// Chyby této crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
}

/// Stav kolektoru (čtení je bezstavové).
pub struct State;

pub fn init(_cfg: &Config) -> Result<State, Error> {
    Ok(State)
}

pub fn tick(_state: &mut State) -> Result<(), Error> {
    Ok(())
}

pub fn shutdown(_state: State) {}

/// Odkud položka pochází (rozhoduje i o způsobu přepnutí).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// HKCU\…\Run
    RunUser,
    /// HKLM\…\Run
    RunMachine,
    /// HKLM\SOFTWARE\WOW6432Node\…\Run — 32bitové položky. Windows jim
    /// vedou stav v `StartupApproved\Run32`, ne v `…\Run`.
    RunMachine32,
    /// HKCU\…\RunOnce — spustí se jednou a Windows hodnotu smažou.
    /// StartupApproved pro ně neexistuje, přepínat nejdou.
    RunOnceUser,
    /// HKLM\…\RunOnce (viz `RunOnceUser`).
    RunOnceMachine,
    /// Startup složka uživatele
    FolderUser,
    /// Startup složka pro všechny
    FolderCommon,
    Task,
    Service,
    Msix,
    /// Winlogon Userinit/Shell — JEN čtení (varování).
    Shell,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::RunUser => "run_user",
            Source::RunMachine => "run_machine",
            Source::RunMachine32 => "run_machine32",
            Source::RunOnceUser => "run_once_user",
            Source::RunOnceMachine => "run_once_machine",
            Source::FolderUser => "folder_user",
            Source::FolderCommon => "folder_common",
            Source::Task => "task",
            Source::Service => "service",
            Source::Msix => "msix",
            Source::Shell => "shell",
        }
    }

    /// Zpět z řetězce (id položky přichází z UI). Vlastní funkce místo
    /// `FromStr` — chybějící varianta není chyba, jen None.
    pub fn parse(s: &str) -> Option<Source> {
        Some(match s {
            "run_user" => Source::RunUser,
            "run_machine" => Source::RunMachine,
            "run_machine32" => Source::RunMachine32,
            "run_once_user" => Source::RunOnceUser,
            "run_once_machine" => Source::RunOnceMachine,
            "folder_user" => Source::FolderUser,
            "folder_common" => Source::FolderCommon,
            "task" => Source::Task,
            "service" => Source::Service,
            "msix" => Source::Msix,
            "shell" => Source::Shell,
            _ => return None,
        })
    }

    /// Jde položku přepínat? Shell rozšíření zásadně ne, RunOnce taky
    /// ne — Windows u něj StartupApproved nečtou, přepnutí by nic
    /// nezměnilo.
    pub fn toggleable(&self) -> bool {
        !matches!(
            self,
            Source::Shell | Source::RunOnceUser | Source::RunOnceMachine
        )
    }
}

/// Jedna startup položka.
#[derive(Debug, Clone)]
pub struct BootItem {
    /// Stabilní klíč pro přepnutí: `{source}|{name}`.
    pub id: String,
    pub name: String,
    pub source: Source,
    /// Příkaz / cesta k binárce.
    pub command: String,
    /// Nastartuje se položka automaticky (u služby typ spuštění).
    pub enabled: bool,
    /// Běží služba právě teď? `None` u všeho, co službou není.
    pub running: Option<bool>,
    /// Cesta k .exe (pro spárování s aplikací a ikonou).
    pub exe_path: Option<String>,
}

/// Přečte všechny startup položky ze všech backendů.
pub fn scan() -> Vec<BootItem> {
    let mut out = Vec::new();
    run_keys(&mut out);
    startup_folders(&mut out);
    tasks(&mut out);
    services(&mut out);
    shell_hooks(&mut out);
    dedup_ids(&mut out);
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

/// Pojistka: id musí být v seznamu jedinečné. UI ho používá jako klíč
/// keyed `{#each}` a Svelte na duplicitě v produkčním buildu vyhodí
/// výjimku — celá stránka Po spuštění (i dlaždice na Home) se pak
/// přestala vykreslovat. Run klíče už jedinečnost zaručují zdrojem,
/// tohle je jen pro případ, že ji jiný backend někdy poruší. Vyhrává
/// první výskyt, přepnutí podle id stejně cílí jen na jednu položku.
fn dedup_ids(items: &mut Vec<BootItem>) {
    let mut videno = std::collections::HashSet::new();
    items.retain(|i| videno.insert(i.id.clone()));
}
/// Hive přihlášeného uživatele pro čtení i zápis uživatelských položek.
///
/// Služba běží jako SYSTEM, takže její `HKEY_CURRENT_USER` je hive
/// SYSTEMU — uživatelské Run položky ani jejich StartupApproved v něm
/// nejsou. Bez tohohle kroku sekce Po spuštění neukázala ani jednu
/// položku, kterou si člověk sám nainstaloval (Steam, Discord, Epic),
/// a přepnutí by zapsalo do cizí hive, takže by se navenek „povedlo"
/// a nezměnilo nic. Na běžném stroji je hive právě jeden.
pub fn user_hive() -> Option<String> {
    win_sys::consent::user_hives().into_iter().next()
}

/// Backend 1: Run klíče (HKLM + hive uživatele, Wow6432Node, Run i RunOnce).
fn run_keys(out: &mut Vec<BootItem>) {
    use win_sys::registry::{enum_values, HKEY_LOCAL_MACHINE, HKEY_USERS};
    // Každý klíč má vlastní zdroj, a tedy i vlastní id `{zdroj}|{jméno}`.
    // Dřív měly HKLM Run, RunOnce i WOW6432Node\Run společné
    // `run_machine` (a uživatelské Run s RunOnce `run_user`): stejné
    // jméno hodnoty ve dvou klíčích (32 + 64bitová Java, updater v Run
    // i RunOnce) dalo dvě položky se stejným id a UI na tom spadlo.
    // Vlastní zdroj zároveň určuje správný StartupApproved klíč.
    const RUN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
    const RUN_ONCE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce";
    const RUN_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
    let mut roots: Vec<(win_sys::registry::RegKey, String, Source)> = vec![
        (HKEY_LOCAL_MACHINE, RUN.to_string(), Source::RunMachine),
        (
            HKEY_LOCAL_MACHINE,
            RUN_ONCE.to_string(),
            Source::RunOnceMachine,
        ),
        (HKEY_LOCAL_MACHINE, RUN_32.to_string(), Source::RunMachine32),
    ];
    // Wow6432Node existuje jen pod HKLM.
    if let Some(sid) = user_hive() {
        roots.push((HKEY_USERS, format!(r"{sid}\{RUN}"), Source::RunUser));
        roots.push((
            HKEY_USERS,
            format!(r"{sid}\{RUN_ONCE}"),
            Source::RunOnceUser,
        ));
    }
    for (root, sub, source) in roots {
        let once = matches!(source, Source::RunOnceUser | Source::RunOnceMachine);
        for (name, cmd) in enum_values(root, &sub) {
            let enabled = approved_state(source, &name).unwrap_or(true);
            out.push(BootItem {
                id: format!("{}|{name}", source.as_str()),
                name: if once {
                    format!("{name} (jednorázově)")
                } else {
                    name.clone()
                },
                source,
                exe_path: exe_from_command(&cmd),
                command: cmd,
                enabled,
                running: None,
            });
        }
    }
}

/// Backend 2: Startup složky (uživatelská + společná).
fn startup_folders(out: &mut Vec<BootItem>) {
    // %APPDATA% by se SYSTEMU rozbalilo do jeho vlastního profilu —
    // složka po spuštění skutečného uživatele se bere z ProfileList.
    let user = user_hive()
        .and_then(|sid| win_sys::consent::profile_path(&sid))
        .map(|p| {
            (
                format!(r"{p}\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup"),
                Source::FolderUser,
            )
        });
    let common = std::env::var("ProgramData").ok().map(|p| {
        (
            format!(r"{p}\Microsoft\Windows\Start Menu\Programs\Startup"),
            Source::FolderCommon,
        )
    });
    for (dir, source) in [user, common].into_iter().flatten() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let enabled = approved_state(source, &name).unwrap_or(true);
            out.push(BootItem {
                id: format!("{}|{name}", source.as_str()),
                name: p
                    .file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| name.clone()),
                source,
                command: p.to_string_lossy().into_owned(),
                exe_path: None,
                enabled,
                running: None,
            });
        }
    }
}

/// Backend 3: Task Scheduler (logon/boot triggery).
fn tasks(out: &mut Vec<BootItem>) {
    // COM je inicializované volajícím vláknem (svc).
    let Ok(tasks) = win_sys::tasksched::startup_tasks() else {
        return;
    };
    for t in tasks {
        out.push(BootItem {
            id: format!("task|{}", t.path),
            name: t.name,
            source: Source::Task,
            command: t.command.clone().unwrap_or_else(|| t.path.clone()),
            exe_path: t.command,
            enabled: t.enabled,
            running: None,
        });
    }
}

/// Backend 4: služby s automatickým startem.
fn services(out: &mut Vec<BootItem>) {
    let Ok(svcs) = win_sys::services::auto_services() else {
        return;
    };
    for s in svcs {
        // Cesta k binárce služby pro spárování s aplikací.
        let key = format!(r"SYSTEM\CurrentControlSet\Services\{}", s.name);
        let image = win_sys::registry::read_string(
            win_sys::registry::HKEY_LOCAL_MACHINE,
            &key,
            "ImagePath",
        );
        out.push(BootItem {
            id: format!("service|{}", s.name),
            name: s.display_name,
            source: Source::Service,
            exe_path: image.as_deref().and_then(exe_from_command),
            command: image.unwrap_or_else(|| s.name.clone()),
            enabled: s.auto_start,
            running: Some(s.running),
        });
    }
}

/// Backend 6: Winlogon Userinit/Shell — JEN čtení. Nestandardní
/// hodnota je varovný signál (běžná: `userinit.exe,` / `explorer.exe`).
fn shell_hooks(out: &mut Vec<BootItem>) {
    use win_sys::registry::{read_string, HKEY_LOCAL_MACHINE};
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon";
    for value in ["Userinit", "Shell"] {
        let Some(v) = read_string(HKEY_LOCAL_MACHINE, KEY, value) else {
            continue;
        };
        let lc = v.to_lowercase();
        let standard = match value {
            "Userinit" => lc.trim_end_matches(',').ends_with("userinit.exe"),
            _ => lc.trim() == "explorer.exe",
        };
        if standard {
            continue; // běžný stav není položka k zobrazení
        }
        out.push(BootItem {
            id: format!("shell|{value}"),
            name: format!("Winlogon {value} (nestandardní!)"),
            source: Source::Shell,
            exe_path: exe_from_command(&v),
            command: v,
            enabled: true,
            running: None,
        });
    }
}

/// Stav ze `StartupApproved` (SPEC kap. 7): byte[0] & 0x01 = zakázáno.
/// Chybí-li hodnota, položka je povolená.
pub fn approved_state(source: Source, name: &str) -> Option<bool> {
    use win_sys::registry::{read_binary, HKEY_LOCAL_MACHINE, HKEY_USERS};
    let (machine, sub) = approved_key(source)?;
    // Uživatelské položky mají StartupApproved ve své hive, ne v hive
    // SYSTEMU, pod kterým služba běží.
    let data = if machine {
        read_binary(HKEY_LOCAL_MACHINE, sub, name)?
    } else {
        let sid = user_hive()?;
        read_binary(HKEY_USERS, &format!(r"{sid}\{sub}"), name)?
    };
    Some(data.first().map(|b| b & 0x01 == 0).unwrap_or(true))
}

/// Kde leží StartupApproved pro daný zdroj: (HKLM?, podklíč).
///
/// Mapování odpovídá tomu, co čte Explorer a Správce úloh. Dřív šly
/// 32bitové položky z WOW6432Node do `…\Run` (Windows je vedou v
/// `…\Run32`) a společná složka Po spuštění do hive uživatele (Windows
/// ji vedou v HKLM). Zobrazený stav pak neodpovídal skutečnosti
/// a přepnutí zapsalo hodnotu, kterou Windows ignorují — ověření ji
/// přečetlo zpátky ze stejného místa a ohlásilo úspěch, položka ale
/// startovala dál. Zápis (actor-toggle) bere klíč odsud, ať se čtení
/// a zápis nemohou znovu rozejít.
pub fn approved_key(source: Source) -> Option<(bool, &'static str)> {
    const BASE_RUN: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const BASE_RUN32: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32";
    const BASE_FOLDER: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";
    match source {
        Source::RunUser => Some((false, BASE_RUN)),
        Source::RunMachine => Some((true, BASE_RUN)),
        Source::RunMachine32 => Some((true, BASE_RUN32)),
        Source::FolderUser => Some((false, BASE_FOLDER)),
        Source::FolderCommon => Some((true, BASE_FOLDER)),
        _ => None,
    }
}

/// Cesta k .exe z příkazové řádky (uvozovky i holá cesta).
pub fn exe_from_command(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    let path = if let Some(rest) = cmd.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        let lc = cmd.to_ascii_lowercase();
        let end = lc.find(".exe")? + 4;
        &cmd[..end]
    };
    let expanded = expand_env(path);
    expanded
        .to_lowercase()
        .ends_with(".exe")
        .then_some(expanded)
}

/// Expanze `%VAR%` v cestě.
fn expand_env(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_parsing() {
        assert_eq!(
            exe_from_command(r#""C:\App\a.exe" --run"#).as_deref(),
            Some(r"C:\App\a.exe")
        );
        assert_eq!(
            exe_from_command(r"C:\App\b.exe /silent").as_deref(),
            Some(r"C:\App\b.exe")
        );
        assert!(exe_from_command("rundll32 shell32.dll,Foo").is_none());
    }

    #[test]
    fn shell_source_is_not_toggleable() {
        assert!(!Source::Shell.toggleable());
        assert!(Source::RunUser.toggleable());
        assert!(Source::RunMachine32.toggleable());
        assert!(!Source::RunOnceUser.toggleable());
        assert!(!Source::RunOnceMachine.toggleable());
    }

    #[test]
    fn source_roundtrip() {
        for s in [
            Source::RunUser,
            Source::RunMachine,
            Source::RunMachine32,
            Source::RunOnceUser,
            Source::RunOnceMachine,
            Source::FolderUser,
            Source::FolderCommon,
            Source::Task,
            Source::Service,
            Source::Msix,
            Source::Shell,
        ] {
            assert_eq!(Source::parse(s.as_str()), Some(s));
        }
    }

    // StartupApproved tam, kde ho čtou Windows: 32bitové Run v Run32,
    // společná složka v HKLM, RunOnce nikde.
    #[test]
    fn approved_key_matches_windows() {
        let (m, k) = approved_key(Source::RunMachine32).unwrap();
        assert!(m && k.ends_with(r"StartupApproved\Run32"));
        let (m, k) = approved_key(Source::RunMachine).unwrap();
        assert!(m && k.ends_with(r"StartupApproved\Run"));
        let (m, k) = approved_key(Source::FolderCommon).unwrap();
        assert!(m && k.ends_with(r"StartupApproved\StartupFolder"));
        let (m, _) = approved_key(Source::FolderUser).unwrap();
        assert!(!m);
        assert!(approved_key(Source::RunOnceUser).is_none());
        assert!(approved_key(Source::RunOnceMachine).is_none());
    }

    // Duplicitní id se ze seznamu vyřadí, první výskyt zůstane.
    #[test]
    fn duplicate_ids_are_dropped() {
        let item = |id: &str, cmd: &str| BootItem {
            id: id.into(),
            name: "x".into(),
            source: Source::RunMachine,
            command: cmd.into(),
            enabled: true,
            running: None,
            exe_path: None,
        };
        let mut v = vec![
            item("run_machine|Java", "a"),
            item("run_machine32|Java", "b"),
            item("run_machine|Java", "c"),
        ];
        dedup_ids(&mut v);
        let cmds: Vec<&str> = v.iter().map(|i| i.command.as_str()).collect();
        assert_eq!(cmds, ["a", "b"]);
    }
}
