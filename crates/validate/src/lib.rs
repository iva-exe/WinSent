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
fn startup_run_exists(name: &str, machine: bool) -> bool {
    use win_sys::registry::{enum_values, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let root = if machine {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    };
    const SUBS: &[&str] = &[
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
    ];
    SUBS.iter().any(|sub| {
        enum_values(root, sub)
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    })
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
