//! actor-app — odinstalace aplikace (v8, SPEC kap. 5.3).
//!
//! Nikdy nemažeme instalaci sami: spustí se **oficiální odinstalátor**
//! aplikace. Co po něm zbyde, se uživateli jen UKÁŽE (mapa souborů
//! proti disku) — smazání zbytků je samostatné rozhodnutí přes
//! actor-file (koš), ne automatický krok.
//!
//! ⚠ ODINSTALÁTOR SE NIKDY NESPOUŠTÍ Z TÉTO CRATE ANI ZE SLUŽBY.
//! Služba běží jako SYSTEM v session 0 (izolovaná neviditelná plocha).
//! Odinstalátor odtud by:
//!   • neměl kam vykreslit dialogy — uživatel je nemůže odkliknout,
//!   • běžel pod SYSTEM, takže `HKEY_CURRENT_USER` je hive SYSTEMu
//!     a instalace „jen pro mě" by se odinstalovala z cizího registru,
//!   • dostal práva, se kterými nepočítá — pozorovaný následek byl
//!     zásek systému a rozbité audio (Overwolf aplikace).
//! Spuštění proto dělá UI proces, který běží pod přihlášeným
//! uživatelem; služba akci jen validuje a auditje.

use core_types::action::{Action, PlanStep};

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
