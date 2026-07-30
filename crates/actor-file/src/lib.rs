//! actor-file — bezpečné mazání souborů (v8, SPEC kap. 18.2).
//!
//! Exekutor: JEN *jak*, nikdy *zda* — spouští se výhradně po
//! `Verdict::Allow` z validate/. Jediná povolená cesta odstranění je
//! **koš** (`FOF_ALLOWUNDO`), takže i „smazané" jde vrátit.
//!
//! Zakázané vzory, které se tu ZÁMĚRNĚ neimplementují (SPEC 18):
//! force delete, `DUPLICATE_CLOSE_SOURCE` (kradení cizích handlů)
//! ani mazání přes zavírání handlů jiných procesů.

use core_types::action::{Action, PlanStep};

/// Chyby exekutoru.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
}

/// Fáze 1 — PLÁN: co se smaže a kdo to drží. Nic nemění.
pub fn plan(action: &Action) -> Vec<PlanStep> {
    let Action::DeleteFiles { paths } = action else {
        return Vec::new();
    };
    let mut steps = Vec::new();
    for p in paths {
        let dir = std::path::Path::new(p).is_dir();
        steps.push(PlanStep {
            description: format!(
                "přesunout {} do koše: {p}",
                if dir { "složku" } else { "soubor" }
            ),
            // Z koše jde soubor vrátit — proto vratné.
            reversible: true,
        });
    }
    // Držitelé jsou informace pro rozhodnutí, ne krok akce.
    if let Ok(holders) = win_sys::rm::holders(paths) {
        for h in holders {
            steps.push(PlanStep {
                description: format!(
                    "pozor: soubor právě používá {} (pid {}{})",
                    h.name,
                    h.pid,
                    h.service
                        .as_deref()
                        .map(|s| format!(", služba {s}"))
                        .unwrap_or_default()
                ),
                reversible: true,
            });
        }
    }
    steps
}

/// Výsledek provedení.
#[derive(Debug, Clone)]
pub struct DeleteOutcome {
    pub ok: bool,
    pub detail: String,
}

/// Fáze 3 — PROVEDENÍ: přesun do koše. Buď projde vše, nebo se hlásí
/// chyba — částečný úspěch se nezamlčuje.
pub fn execute(action: &Action) -> DeleteOutcome {
    let Action::DeleteFiles { paths } = action else {
        return DeleteOutcome {
            ok: false,
            detail: "actor-file neumí tuto akci".into(),
        };
    };
    match win_sys::recycle::to_recycle_bin(paths) {
        Ok(()) => DeleteOutcome {
            ok: true,
            detail: format!("přesunuto do koše: {} položek", paths.len()),
        },
        Err(e) => DeleteOutcome {
            ok: false,
            detail: format!("přesun do koše selhal: {e}"),
        },
    }
}

/// Fáze 4 — OVĚŘENÍ: cesty už na disku nejsou. Čte se ZNOVU ze
/// souborového systému; nikdy se netvářit, že akce prošla.
pub fn verify(action: &Action) -> bool {
    let Action::DeleteFiles { paths } = action else {
        return false;
    };
    paths.iter().all(|p| !std::path::Path::new(p).exists())
}

/// Jak akci vrátit — pro auditní sloupec `reversible`.
pub fn undo_hint(action: &Action) -> Option<String> {
    match action {
        Action::DeleteFiles { paths } => Some(format!("obnovit z koše ({} položek)", paths.len())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Plán označí kroky jako vratné (koš) a popíše každou cestu.
    #[test]
    fn plan_is_reversible() {
        let a = Action::DeleteFiles {
            paths: vec![r"C:\tmp\a.txt".into(), r"C:\tmp\b.txt".into()],
        };
        let steps = plan(&a);
        assert!(steps.len() >= 2);
        assert!(steps.iter().all(|s| s.reversible), "koš je vratný");
        assert!(steps[0].description.contains("koše"));
    }

    // Ověření na neexistujících cestách hlásí úspěch (soubor je pryč).
    #[test]
    fn verify_gone_paths() {
        let a = Action::DeleteFiles {
            paths: vec![r"C:\rozhodne-neexistuje-xyz\a.txt".into()],
        };
        assert!(verify(&a));
    }

    // Skutečné smazání do koše a ověření (běží na reálném FS).
    #[test]
    fn deletes_temp_file_to_recycle_bin() {
        let f = std::env::temp_dir().join("winsent-actor-file-test.tmp");
        std::fs::write(&f, b"test").expect("zapsat");
        let a = Action::DeleteFiles {
            paths: vec![f.to_string_lossy().into_owned()],
        };
        let out = execute(&a);
        assert!(out.ok, "mazání selhalo: {}", out.detail);
        assert!(verify(&a), "soubor po smazání pořád existuje");
    }
}
