//! actor-proc — ukončování procesů (v7, SPEC kap. 17, 18.4).
//!
//! Exekutor: JEN *jak*, nikdy *zda* — spouští se výhradně po
//! `Verdict::Allow` z validate/. Plán ukazuje, kdo všechno padne
//! (strom potomků z čerstvého snapshotu), provedení jde odspodu
//! nahoru (potomci první, ať nezůstanou sirotci).
//!
//! Kill je NEVRATNÝ — proto T1 s potvrzením. Žádný rollback
//! neexistuje, o to důkladnější je fáze ověření.

use core_types::action::{Action, PlanStep};

/// Chyby exekutoru.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
}

/// Jeden proces v plánu ukončení.
#[derive(Debug, Clone)]
pub struct Victim {
    pub pid: u32,
    pub name: String,
    /// Hloubka pod cílem (0 = samotný cíl).
    pub depth: u32,
}

/// Strom potomků cíle z čerstvého snapshotu (SPEC 17.3 — živý stav).
/// Parent PID z aktuálního snapshotu; osiřelé procesy (rodič už
/// neexistuje) se do stromu nepočítají.
pub fn victims(pid: u32, tree: bool) -> Vec<Victim> {
    let mut buf = Vec::new();
    let Ok(procs) = win_sys::proc::snapshot_processes(&mut buf) else {
        return Vec::new();
    };
    let Some(target) = procs.iter().find(|x| x.pid == pid) else {
        return Vec::new();
    };
    let mut out = vec![Victim {
        pid,
        name: target.name.clone(),
        depth: 0,
    }];
    if !tree {
        return out;
    }
    // Po vrstvách, pojistka proti cyklům v parent vazbách.
    let mut frontier = vec![pid];
    let mut depth = 1;
    while !frontier.is_empty() && depth <= 8 {
        let mut next = Vec::new();
        for p in &procs {
            if frontier.contains(&p.parent_pid)
                && p.pid != p.parent_pid
                && !out.iter().any(|v| v.pid == p.pid)
            {
                out.push(Victim {
                    pid: p.pid,
                    name: p.name.clone(),
                    depth,
                });
                next.push(p.pid);
            }
        }
        frontier = next;
        depth += 1;
    }
    out
}

/// Fáze 1 — PLÁN: co se ukončí a co to znamená. Nic nemění.
pub fn plan(action: &Action) -> Vec<PlanStep> {
    match action {
        Action::KillProc { pid, tree, .. } => {
            let vs = victims(*pid, *tree);
            let mut steps: Vec<PlanStep> = vs
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
            steps
        }
        _ => Vec::new(),
    }
}

/// Výsledek provedení.
#[derive(Debug, Clone)]
pub struct KillOutcome {
    pub ok: bool,
    pub killed: Vec<u32>,
    pub detail: String,
}

/// Fáze 3 — PROVEDENÍ: potomci první, pak cíl. Selhání jednoho kroku
/// akci nezastaví (potomek mohl mezitím sám skončit), ale hlásí se.
pub fn execute(action: &Action) -> KillOutcome {
    let Action::KillProc { pid, tree, .. } = action else {
        return KillOutcome {
            ok: false,
            killed: Vec::new(),
            detail: "actor-proc neumí tuto akci".into(),
        };
    };
    let vs = victims(*pid, *tree);
    let mut killed = Vec::new();
    let mut failed = Vec::new();
    for v in vs.iter().rev() {
        match win_sys::procinfo::terminate(v.pid) {
            Ok(()) => killed.push(v.pid),
            Err(e) => failed.push(format!("{} ({}): {e}", v.name, v.pid)),
        }
    }
    // Cíl MUSÍ skončit, jinak akce neuspěla.
    let ok = killed.contains(pid);
    KillOutcome {
        ok,
        detail: if failed.is_empty() {
            format!("ukončeno {} procesů", killed.len())
        } else {
            format!(
                "ukončeno {}, neúspěšné: {}",
                killed.len(),
                failed.join("; ")
            )
        },
        killed,
    }
}

/// Fáze 4 — OVĚŘENÍ: cíl už mezi procesy není. Čte se ZNOVU z OS;
/// nikdy se netvářit, že akce prošla.
///
/// TerminateProcess je asynchronní — objekt procesu chvíli přežívá,
/// než jádro dokončí úklid. Proto se ověřuje opakovaně do 3 s; teprve
/// pak je neúspěch skutečný neúspěch.
pub fn verify(action: &Action) -> bool {
    let Action::KillProc { pid, .. } = action else {
        return false;
    };
    let mut buf = Vec::new();
    for attempt in 0..15 {
        match win_sys::proc::snapshot_processes(&mut buf) {
            Ok(procs) => {
                if !procs.iter().any(|p| p.pid == *pid) {
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

    // Vlastní proces je vždy v seznamu obětí (bez stromu právě jeden).
    #[test]
    fn victims_include_target() {
        let vs = victims(std::process::id(), false);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].pid, std::process::id());
        assert_eq!(vs[0].depth, 0);
    }

    // Plán řadí potomky před cíl a žádný krok není vratný.
    #[test]
    fn plan_marks_steps_irreversible() {
        let a = Action::KillProc {
            pid: std::process::id(),
            create_time: 0,
            tree: true,
        };
        let steps = plan(&a);
        assert!(!steps.is_empty());
        assert!(steps.iter().all(|s| !s.reversible), "kill není vratný");
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
