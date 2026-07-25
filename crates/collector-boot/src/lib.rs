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
    /// HKLM\…\Run (+ Wow6432Node)
    RunMachine,
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
            "folder_user" => Source::FolderUser,
            "folder_common" => Source::FolderCommon,
            "task" => Source::Task,
            "service" => Source::Service,
            "msix" => Source::Msix,
            "shell" => Source::Shell,
            _ => return None,
        })
    }

    /// Jde položku přepínat? Shell rozšíření zásadně ne.
    pub fn toggleable(&self) -> bool {
        !matches!(self, Source::Shell)
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
    pub enabled: bool,
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
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

/// Backend 1: Run klíče (HKLM/HKCU + Wow6432Node, Run i RunOnce).
fn run_keys(out: &mut Vec<BootItem>) {
    use win_sys::registry::{enum_values, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    const RUNS: &[(&str, bool)] = &[
        (r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run", false),
        (r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce", true),
        (
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
            false,
        ),
    ];
    for (root, machine) in [(HKEY_CURRENT_USER, false), (HKEY_LOCAL_MACHINE, true)] {
        for (sub, once) in RUNS {
            // Wow6432Node existuje jen pod HKLM.
            if sub.contains("WOW6432Node") && !machine {
                continue;
            }
            for (name, cmd) in enum_values(root, sub) {
                let source = if machine {
                    Source::RunMachine
                } else {
                    Source::RunUser
                };
                let enabled = approved_state(source, &name).unwrap_or(true);
                out.push(BootItem {
                    id: format!("{}|{name}", source.as_str()),
                    name: if *once {
                        format!("{name} (jednorázově)")
                    } else {
                        name.clone()
                    },
                    source,
                    exe_path: exe_from_command(&cmd),
                    command: cmd,
                    enabled,
                });
            }
        }
    }
}

/// Backend 2: Startup složky (uživatelská + společná).
fn startup_folders(out: &mut Vec<BootItem>) {
    let user = std::env::var("APPDATA").ok().map(|a| {
        (
            format!(r"{a}\Microsoft\Windows\Start Menu\Programs\Startup"),
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
        });
    }
}

/// Stav ze `StartupApproved` (SPEC kap. 7): byte[0] & 0x01 = zakázáno.
/// Chybí-li hodnota, položka je povolená.
pub fn approved_state(source: Source, name: &str) -> Option<bool> {
    use win_sys::registry::{read_binary, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let (machine, sub) = approved_key(source)?;
    let root = if machine {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    };
    let data = read_binary(root, sub, name)?;
    Some(data.first().map(|b| b & 0x01 == 0).unwrap_or(true))
}

/// Kde leží StartupApproved pro daný zdroj: (HKLM?, podklíč).
pub fn approved_key(source: Source) -> Option<(bool, &'static str)> {
    match source {
        Source::RunUser => Some((
            false,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
        )),
        Source::RunMachine => Some((
            true,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
        )),
        Source::FolderUser | Source::FolderCommon => Some((
            false,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
        )),
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
    }

    #[test]
    fn source_roundtrip() {
        for s in [
            Source::RunUser,
            Source::RunMachine,
            Source::FolderUser,
            Source::Task,
            Source::Service,
            Source::Shell,
        ] {
            assert_eq!(Source::parse(s.as_str()), Some(s));
        }
    }
}
