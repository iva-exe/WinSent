//! actor-app — odinstalace aplikace (v8, SPEC kap. 5.3).
//!
//! Nikdy nemažeme instalaci sami: spustí se **oficiální odinstalátor**
//! aplikace a počká se, až doběhne. Co po něm zbyde, se uživateli jen
//! UKÁŽE (mapa souborů proti disku) — smazání zbytků je samostatné
//! rozhodnutí přes actor-file (koš), ne automatický krok.
//!
//! Exekutor: JEN *jak*, nikdy *zda* — spouští se výhradně po
//! `Verdict::Allow` z validate/, který příkaz sám čerstvě přečetl
//! z registru (UI ho nemůže podvrhnout).

use core_types::action::{Action, PlanStep};

/// Chyby exekutoru.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("odinstalátor nelze spustit: {0}")]
    Spawn(String),
}

/// Kolik nejdéle čekáme na doběhnutí odinstalátoru. Uživatel v něm
/// klikáním prochází dialogy — proto velkoryse, ale ne donekonečna.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Fáze 1 — PLÁN: co se spustí a co bude následovat. Nic nemění.
pub fn plan(action: &Action) -> Vec<PlanStep> {
    let Action::UninstallApp { identity_key } = action else {
        return Vec::new();
    };
    let name = identity_key.strip_prefix("app:").unwrap_or(identity_key);
    let cmd = validate::uninstall_command(name);
    let mut steps = vec![PlanStep {
        description: match &cmd {
            Some(c) => format!("spustit oficiální odinstalátor: {c}"),
            None => "odinstalátor se nepodařilo najít".into(),
        },
        // Odinstalace se nevrací — případné zbytky ano (koš).
        reversible: false,
    }];
    steps.push(PlanStep {
        description: "projít okno odinstalátoru (dialogy potvrzuješ ty)".into(),
        reversible: false,
    });
    steps.push(PlanStep {
        description: "po dokončení Winsent porovná mapu souborů s diskem a ukáže, co zbylo".into(),
        reversible: true,
    });
    steps
}

/// Výsledek provedení.
#[derive(Debug, Clone)]
pub struct UninstallOutcome {
    pub ok: bool,
    pub detail: String,
}

/// Fáze 3 — PROVEDENÍ: spustí odinstalátor a počká na jeho konec.
/// Odinstalátor běží s právy služby (SYSTEM) a své okno si řídí sám.
pub fn execute(action: &Action) -> UninstallOutcome {
    let Action::UninstallApp { identity_key } = action else {
        return UninstallOutcome {
            ok: false,
            detail: "actor-app neumí tuto akci".into(),
        };
    };
    let name = identity_key.strip_prefix("app:").unwrap_or(identity_key);
    let Some(cmd) = validate::uninstall_command(name) else {
        return UninstallOutcome {
            ok: false,
            detail: "odinstalační příkaz zmizel mezi validací a provedením".into(),
        };
    };

    // Příkaz může být „cesta.exe /S" — exe zvlášť, zbytek argumenty.
    let Some(exe) = validate::exe_of_command(&cmd) else {
        return UninstallOutcome {
            ok: false,
            detail: format!("nečitelný příkaz: {cmd}"),
        };
    };
    let rest = cmd
        .trim()
        .trim_start_matches('"')
        .strip_prefix(exe.as_str())
        .unwrap_or("")
        .trim_start_matches('"')
        .trim()
        .to_string();

    let mut command = std::process::Command::new(&exe);
    if !rest.is_empty() {
        // split_whitespace stačí — argumenty odinstalátorů jsou
        // přepínače typu /S, /quiet, /X{GUID}.
        command.args(rest.split_whitespace());
    }
    let child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            return UninstallOutcome {
                ok: false,
                detail: format!("spuštění selhalo: {e}"),
            }
        }
    };
    let pid = child.id();
    match wait_with_timeout(child) {
        Some(status) => UninstallOutcome {
            ok: true,
            detail: format!("odinstalátor (pid {pid}) skončil: {status}"),
        },
        None => UninstallOutcome {
            // Timeout není úspěch — uživatel možná odinstalaci zrušil
            // nebo okno nechal otevřené. Netvrdíme, že je hotovo.
            ok: false,
            detail: format!("odinstalátor (pid {pid}) neskončil do 15 minut"),
        },
    }
}

/// Počká na konec procesu s časovým limitem (std::process nemá
/// wait_timeout; pollujeme try_wait).
fn wait_with_timeout(mut child: std::process::Child) -> Option<std::process::ExitStatus> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if start.elapsed() > TIMEOUT {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Fáze 4 — OVĚŘENÍ: aplikace už v registru není. Čte se ZNOVU;
/// nikdy se netvářit, že odinstalace proběhla.
pub fn verify(action: &Action) -> bool {
    let Action::UninstallApp { identity_key } = action else {
        return false;
    };
    let name = identity_key.strip_prefix("app:").unwrap_or(identity_key);
    validate::uninstall_command(name).is_none()
}

/// Zbytky po odinstalaci: cesty z mapy souborů, které na disku pořád
/// jsou. Volá se PO odinstalaci — čistě čtecí, nic nemaže.
pub fn leftovers(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| {
            // Registry větve se takhle kontrolovat nedají.
            !p.starts_with("HK") && std::path::Path::new(p).exists()
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Plán vždy vysvětlí, že dialogy odklikává uživatel.
    #[test]
    fn plan_mentions_official_uninstaller() {
        let a = Action::UninstallApp {
            identity_key: "app:neexistujici aplikace xyz".into(),
        };
        let steps = plan(&a);
        assert_eq!(steps.len(), 3);
        assert!(steps[2].description.contains("zbylo"));
    }

    // Zbytky: existující cesty ano, registry větve a smazané ne.
    #[test]
    fn leftovers_filters_paths() {
        let tmp = std::env::temp_dir().join("winsent-actor-app-test.tmp");
        std::fs::write(&tmp, b"x").expect("zapsat");
        let paths = vec![
            tmp.to_string_lossy().into_owned(),
            r"C:\neexistuje-xyz\a.txt".into(),
            r"HKLM\SOFTWARE\Neco".into(),
        ];
        let left = leftovers(&paths);
        assert_eq!(left.len(), 1);
        assert!(left[0].contains("winsent-actor-app-test"));
        let _ = std::fs::remove_file(&tmp);
    }

    // Neexistující aplikace: ověření hlásí „už není" (nic k odinstalaci).
    #[test]
    fn verify_missing_app_is_gone() {
        let a = Action::UninstallApp {
            identity_key: "app:rozhodne neexistujici aplikace 12345".into(),
        };
        assert!(verify(&a));
    }
}
