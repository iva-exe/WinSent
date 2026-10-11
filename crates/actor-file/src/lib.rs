//! actor-file — bezpečné mazání souborů (v8, SPEC kap. 18.2).
//!
//! Exekutor: JEN *jak*, nikdy *zda* — spouští se výhradně po
//! `Verdict::Allow` z validate/. Jediná povolená cesta odstranění je
//! **koš** (`FOF_ALLOWUNDO`), takže i „smazané" jde vrátit.
//!
//! Od v11 samotný přesun do koše NEDĚLÁ služba, ale UI v relaci
//! uživatele (`win_sys::recycle`, Tauri příkaz `delete_to_recycle`).
//! Služba běží jako SYSTEM a soubory od ní končily v koši SYSTEMu
//! (`$Recycle.Bin\S-1-5-18`), kam uživatel nevidí — slib „jde vrátit
//! z koše" tak nebyl pravda. Tady zůstává plán a ověření.
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
        steps.push(PlanStep {
            description: format!("přesunout do tvého koše: {p}"),
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

/// Fáze 4 — OVĚŘENÍ: cesty už na disku nejsou. Čte se ZNOVU ze
/// souborového systému; nikdy se netvářit, že akce prošla.
pub fn verify_paths(paths: &[String]) -> bool {
    paths.iter().all(|p| !std::path::Path::new(p).exists())
}

/// `verify_paths` nad akcí.
pub fn verify(action: &Action) -> bool {
    let Action::DeleteFiles { paths } = action else {
        return false;
    };
    verify_paths(paths)
}

/// Jak mazání vrátit — pro auditní sloupec `reversible`. Počítá se až
/// z výsledku: `v_kosi` = položky, které podle Windows skončily v koši.
/// Nula = nic se do koše nedostalo (zrušeno, selhalo, nebo smazáno
/// natrvalo) a historie nemá co slibovat.
pub fn undo_hint(v_kosi: usize) -> Option<String> {
    (v_kosi > 0).then(|| format!("obnovit z koše ({v_kosi} položek)"))
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
        assert_eq!(undo_hint(1).as_deref(), Some("obnovit z koše (1 položek)"));
        assert_eq!(undo_hint(0), None, "bez položek v koši nic neslibovat");
    }

    // Existující soubor = neověřeno.
    #[test]
    fn verify_existing_path_fails() {
        let f = std::env::temp_dir().join("winsent-actor-file-verify.tmp");
        std::fs::write(&f, b"x").expect("zapsat");
        let ok = verify_paths(&[f.to_string_lossy().into_owned()]);
        let _ = std::fs::remove_file(&f);
        assert!(!ok, "soubor pořád existuje");
    }
}
