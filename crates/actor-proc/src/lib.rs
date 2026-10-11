//! actor-proc — ukončování procesů (v7, SPEC kap. 17, 18.4).
//!
//! Exekutor: JEN *jak*, nikdy *zda* — spouští se výhradně po
//! `Verdict::Allow` z validate/. Plán ukazuje, kdo všechno padne
//! (strom potomků z čerstvého snapshotu), provedení jde odspodu
//! nahoru (potomci první, ať nezůstanou sirotci).
//!
//! Kill je NEVRATNÝ — proto T1 s potvrzením. Žádný rollback
//! neexistuje, o to důkladnější je fáze ověření.
//!
//! Strom (v11) se počítá JEDNOU, při plánu, a uživatel potvrzuje přesně
//! ten seznam. Provedení ho nepočítá znovu: mezi plánem a potvrzením
//! mohl vzniknout nový potomek nebo se recyklovat PID a ukončilo by se
//! něco, co v plánu nebylo. Každý potomek prochází stejným pravidlem
//! jako kořen (`validate::kill_deny_reason`); co Winsent neukončuje
//! (třeba conhost konzolového programu), se ze stromu vyřadí a v plánu
//! se napíše, že zůstane.

use core_types::action::{Action, PlanStep};

/// Chyby exekutoru.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
}

/// Jeden proces v plánu ukončení — identita je (pid, create_time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Victim {
    pub pid: u32,
    /// Čas vzniku (FILETIME) — podle něj se pozná recyklovaný PID.
    pub create_time: i64,
    pub name: String,
    /// Hloubka pod cílem (0 = samotný cíl).
    pub depth: u32,
}

/// Potomek, který se NEukončí, a proč.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub pid: u32,
    pub name: String,
    pub reason: String,
}

/// Strom k ukončení: oběti v pořadí od cíle dolů a vyřazení potomci.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KillTree {
    pub victims: Vec<Victim>,
    pub kept: Vec<Kept>,
}

/// Postaví strom z čerstvého snapshotu (SPEC 17.3 — živý stav).
///
/// - Cíl musí být TÁ instance (pid + `create_time`), jinak je strom prázdný.
/// - Potomek je jen proces, jehož rodič je ve stromu A který vznikl až po
///   něm (`child.create_time >= parent.create_time`). Windows PID
///   recyklují a parent PID se neuklízí: csrss.exe, jehož původní rodič
///   dávno skončil, by jinak vypadal jako dítě kteréhokoli procesu, který
///   ten PID dostal později.
/// - Každý potomek projde `validate::kill_deny_reason`; zamítnutý se
///   neukončí a do jeho podstromu se nejde (žije dál, jeho děti jsou
///   jeho věc).
pub fn build(pid: u32, create_time: i64, tree: bool) -> KillTree {
    let mut buf = Vec::new();
    let Ok(procs) = win_sys::proc::snapshot_processes(&mut buf) else {
        return KillTree::default();
    };
    let Some(target) = procs
        .iter()
        .find(|x| x.pid == pid && x.create_time == create_time)
    else {
        return KillTree::default();
    };
    let mut out = KillTree {
        victims: vec![Victim {
            pid,
            create_time,
            name: target.name.clone(),
            depth: 0,
        }],
        kept: Vec::new(),
    };
    if !tree {
        return out;
    }
    let rules = validate::KillRules::fresh();
    // Po vrstvách; (pid, čas vzniku) rodičů v aktuální vrstvě.
    let mut frontier: Vec<(u32, i64)> = vec![(pid, create_time)];
    let mut depth = 1;
    while !frontier.is_empty() && depth <= 8 {
        let mut next = Vec::new();
        for p in &procs {
            if p.pid == p.parent_pid
                || out.victims.iter().any(|v| v.pid == p.pid)
                || out.kept.iter().any(|k| k.pid == p.pid)
            {
                continue;
            }
            // Rodič ve stromu a STARŠÍ než dítě — jinak je to cizí proces,
            // který jen zdědil recyklovaný PID.
            let je_dite = frontier
                .iter()
                .any(|(fp, fct)| *fp == p.parent_pid && p.create_time >= *fct);
            if !je_dite {
                continue;
            }
            if let Some(reason) = validate::kill_deny_reason(p.pid, &p.name, &rules) {
                out.kept.push(Kept {
                    pid: p.pid,
                    name: p.name.clone(),
                    reason,
                });
                continue;
            }
            out.victims.push(Victim {
                pid: p.pid,
                create_time: p.create_time,
                name: p.name.clone(),
                depth,
            });
            next.push((p.pid, p.create_time));
        }
        frontier = next;
        depth += 1;
    }
    out
}

/// Fáze 1 — PLÁN: co se ukončí, co zůstane a co to znamená. Nic nemění.
pub fn steps(tree: &KillTree, pid: u32) -> Vec<PlanStep> {
    let mut steps: Vec<PlanStep> = tree
        .victims
        .iter()
        .rev() // potomci první — přesně v pořadí provedení
        .map(|v| PlanStep {
            description: if v.depth == 0 {
                format!("ukončit {} (pid {})", v.name, v.pid)
            } else {
                format!("ukončit potomka {} (pid {})", v.name, v.pid)
            },
            // Ukončený proces se nevrátí — žádný krok není vratný.
            reversible: false,
        })
        .collect();
    if steps.is_empty() {
        steps.push(PlanStep {
            description: format!("proces {pid} už neběží"),
            reversible: false,
        });
    }
    if !tree.kept.is_empty() {
        // Informace, ne krok: tyhle procesy Winsent nechá být. Většinou
        // skončí samy s rodičem (conhost konzolového programu).
        let seznam: Vec<String> = tree
            .kept
            .iter()
            .map(|k| format!("{} (pid {})", k.name, k.pid))
            .collect();
        steps.push(PlanStep {
            description: format!(
                "ponechány: {} — Winsent je neukončuje, obvykle skončí s rodičem",
                seznam.join(", ")
            ),
            reversible: true,
        });
    }
    steps
}

/// Plán pro akci (strom + kroky). Zmrazený strom si drží orchestrátor
/// a při provedení ho předá `execute_frozen`.
pub fn plan(action: &Action) -> (KillTree, Vec<PlanStep>) {
    match action {
        Action::KillProc {
            pid,
            create_time,
            tree,
        } => {
            let t = build(*pid, *create_time, *tree);
            let s = steps(&t, *pid);
            (t, s)
        }
        _ => (KillTree::default(), Vec::new()),
    }
}

/// Výsledek provedení.
#[derive(Debug, Clone)]
pub struct KillOutcome {
    pub ok: bool,
    pub killed: Vec<u32>,
    pub detail: String,
}

/// Fáze 3 — PROVEDENÍ zmrazeného stromu: potomci první, pak cíl.
///
/// Ukončuje se JEN to, co uživatel potvrdil, a jen pokud pod tím PID
/// pořád běží tatáž instance (čas vzniku se ověřuje na handle, kterým se
/// ukončuje). Každý potomek ještě jednou projde pravidlem — mezi plánem
/// a potvrzením si proces mohl sám zapnout kritičnost. Selhání jednoho
/// kroku akci nezastaví (potomek mohl mezitím sám skončit), ale hlásí se.
pub fn execute_frozen(victims: &[Victim]) -> KillOutcome {
    let Some(root) = victims.iter().find(|v| v.depth == 0) else {
        return KillOutcome {
            ok: false,
            killed: Vec::new(),
            detail: "plán neobsahuje cíl".into(),
        };
    };
    let rules = validate::KillRules::fresh();
    let mut killed = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = Vec::new();
    for v in victims.iter().rev() {
        // Kořen už ověřila validační vrstva těsně před provedením.
        if v.depth > 0 {
            if let Some(why) = validate::kill_deny_reason(v.pid, &v.name, &rules) {
                skipped.push(format!("{} ({}): {why}", v.name, v.pid));
                continue;
            }
        }
        match win_sys::procinfo::terminate_instance(v.pid, v.create_time) {
            Ok(true) => killed.push(v.pid),
            // Mezitím skončil a PID má někdo jiný (nebo nikdo).
            Ok(false) => skipped.push(format!("{} ({}): už neběží", v.name, v.pid)),
            Err(e) => failed.push(format!("{} ({}): {e}", v.name, v.pid)),
        }
    }
    // Cíl MUSÍ skončit, jinak akce neuspěla.
    let ok = killed.contains(&root.pid);
    let mut detail = format!("ukončeno {} procesů", killed.len());
    if !skipped.is_empty() {
        detail.push_str(&format!(", vynecháno: {}", skipped.join("; ")));
    }
    if !failed.is_empty() {
        detail.push_str(&format!(", neúspěšné: {}", failed.join("; ")));
    }
    KillOutcome { ok, killed, detail }
}

/// Fáze 4 — OVĚŘENÍ: cíl (tatáž instance) už mezi procesy není. Čte se
/// ZNOVU z OS; nikdy se netvářit, že akce prošla.
///
/// TerminateProcess je asynchronní — objekt procesu chvíli přežívá,
/// než jádro dokončí úklid. Proto se ověřuje opakovaně do 3 s; teprve
/// pak je neúspěch skutečný neúspěch.
pub fn verify(action: &Action) -> bool {
    let Action::KillProc {
        pid, create_time, ..
    } = action
    else {
        return false;
    };
    let mut buf = Vec::new();
    for attempt in 0..15 {
        match win_sys::proc::snapshot_processes(&mut buf) {
            Ok(procs) => {
                // Pod tím PID smí běžet jiný proces (recyklace) — rozhoduje
                // instance, ne číslo.
                if !procs
                    .iter()
                    .any(|p| p.pid == *pid && p.create_time == *create_time)
                {
                    return true;
                }
            }
            // Nejde přečíst stav → tvrdit úspěch by bylo lhaní.
            Err(_) => return false,
        }
        if attempt < 14 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vlastni_cas_vzniku() -> i64 {
        let mut buf = Vec::new();
        win_sys::proc::snapshot_processes(&mut buf)
            .expect("snapshot")
            .into_iter()
            .find(|p| p.pid == std::process::id())
            .expect("vlastní proces")
            .create_time
    }

    // Vlastní proces je vždy v seznamu obětí (bez stromu právě jeden).
    #[test]
    fn victims_include_target() {
        let t = build(std::process::id(), vlastni_cas_vzniku(), false);
        assert_eq!(t.victims.len(), 1);
        assert_eq!(t.victims[0].pid, std::process::id());
        assert_eq!(t.victims[0].depth, 0);
    }

    // Jiná instance (špatný čas vzniku) = prázdný strom, nic k ukončení.
    #[test]
    fn jina_instance_prazdny_strom() {
        let t = build(std::process::id(), 12345, true);
        assert!(t.victims.is_empty());
        let s = steps(&t, std::process::id());
        assert!(s[0].description.contains("už neběží"));
    }

    // Potomci: vlastní dítě (PING.EXE ze System32) do stromu jako oběť
    // nepatří — Winsent ho neukončuje — a v plánu je jako ponechaný.
    #[test]
    fn potomek_ze_slozky_windows_ponechan() {
        use std::os::windows::process::CommandExt;
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let ping = std::path::Path::new(&windir).join(r"System32\PING.EXE");
        // CREATE_NO_WINDOW: test nesmí na ploše probliknout konzolí.
        let Ok(mut child) = std::process::Command::new(&ping)
            .args(["-n", "20", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
        else {
            return;
        };
        let t = build(std::process::id(), vlastni_cas_vzniku(), true);
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            !t.victims.iter().any(|v| v.pid == child.id()),
            "PING.EXE ze System32 se ukončit nesmí"
        );
        assert!(
            t.kept.iter().any(|k| k.pid == child.id()),
            "chybí mezi ponechanými"
        );
        let s = steps(&t, std::process::id());
        assert!(s.iter().any(|x| x.description.starts_with("ponechány:")));
    }

    // Plán řadí potomky před cíl a žádný krok ukončení není vratný.
    #[test]
    fn plan_marks_steps_irreversible() {
        let a = Action::KillProc {
            pid: std::process::id(),
            create_time: vlastni_cas_vzniku(),
            tree: true,
        };
        let (t, steps) = plan(&a);
        assert!(!steps.is_empty());
        assert!(
            steps
                .iter()
                .filter(|s| s.description.starts_with("ukončit"))
                .all(|s| !s.reversible),
            "kill není vratný"
        );
        // Cíl je v zmrazeném stromu první, v krocích poslední.
        assert_eq!(t.victims.first().map(|v| v.depth), Some(0));
        let posledni_ukonceni = steps
            .iter()
            .rfind(|s| s.description.starts_with("ukončit"))
            .map(|s| s.description.clone())
            .unwrap_or_default();
        assert!(
            posledni_ukonceni.starts_with("ukončit ") && !posledni_ukonceni.contains("potomka")
        );
    }

    // Zmrazený plán s neexistující instancí nic neukončí a nehlásí úspěch.
    #[test]
    fn zmrazena_obet_ktera_neexistuje() {
        let out = execute_frozen(&[Victim {
            pid: 4_000_000_001,
            create_time: 1,
            name: "nic.exe".into(),
            depth: 0,
        }]);
        assert!(!out.ok);
        assert!(out.killed.is_empty());
        assert!(!execute_frozen(&[]).ok);
    }

    // Neexistující proces: ověření hlásí „už neběží".
    #[test]
    fn nonexistent_pid_verifies_as_gone() {
        let a = Action::KillProc {
            pid: 4_000_000_001,
            create_time: 0,
            tree: false,
        };
        assert!(verify(&a));
    }
}
