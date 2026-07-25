//! actor-toggle — první exekutor mutující cesty (v5, SPEC kap. 17).
//!
//! v5: testovací akce, na kterých se PROVĚŘUJE vrstva — in-memory
//! přepínač (T0) a testovací operace s vynutitelným selháním (T1,
//! prověření rollbacku). Reálné přepínače (startup, soukromí) přijdou
//! v6 toutéž cestou.
//!
//! Exekutor se NIKDY nespouští bez `Verdict::Allow` z validate/ —
//! to vynucuje orchestrátor v svc; tady je jen *jak*, ne *zda*.

use std::collections::HashMap;
use std::sync::Mutex;

use core_types::action::{Action, PlanStep};

/// Chyby exekutoru.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("krok {step} selhal: {reason}")]
    StepFailed { step: u32, reason: String },
}

/// In-memory úložiště testovacích přepínačů. Reálný stav v OS nemá —
/// v5 prověřuje vrstvu, ne Windows.
static TOGGLES: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);

/// Přečte stav testovacího přepínače (pro fázi ověření).
pub fn toggle_state(key: &str) -> Option<bool> {
    TOGGLES
        .lock()
        .expect("toggle lock")
        .as_ref()
        .and_then(|m| m.get(key).copied())
}

/// Fáze 1 — PLÁN: seznam kroků, nic se nemění (SPEC 17.4).
pub fn plan(action: &Action) -> Vec<PlanStep> {
    match action {
        Action::TestToggle { key, on } => vec![PlanStep {
            description: format!("přepnout {key} na {on} (vratné přepnutím zpět)"),
            reversible: true,
        }],
        Action::TestOp { target, .. } => vec![
            PlanStep {
                description: format!("krok 1: připravit {target}"),
                reversible: true,
            },
            PlanStep {
                description: format!("krok 2: změnit {target}"),
                reversible: true,
            },
            PlanStep {
                description: format!("krok 3: dokončit {target}"),
                reversible: true,
            },
        ],
        Action::CheckProc { pid, .. } => vec![PlanStep {
            description: format!("ověřit proces {pid} (bez mutace)"),
            reversible: true,
        }],
    }
}

/// Výsledek fáze 3 — PROVEDENÍ.
#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub ok: bool,
    pub rolled_back: bool,
    pub detail: String,
}

/// Fáze 3 — PROVEDENÍ: kroky v pořadí, transakčně. Selhání uprostřed
/// → STOP + rollback už provedených kroků (undo stack, LIFO).
pub fn execute(action: &Action) -> ExecOutcome {
    match action {
        Action::TestToggle { key, on } => {
            let mut lock = TOGGLES.lock().expect("toggle lock");
            let map = lock.get_or_insert_with(HashMap::new);
            let prev = map.insert(key.clone(), *on);
            ExecOutcome {
                ok: true,
                rolled_back: false,
                detail: format!("{key}: {prev:?} → {on}"),
            }
        }
        Action::TestOp { target, fail_at } => {
            // Undo stack: každý provedený krok umí couvnout.
            let mut undo: Vec<String> = Vec::new();
            for step in 1..=3u32 {
                if Some(step) == *fail_at {
                    // Umělé selhání (brána v5) → rollback v opačném pořadí.
                    for u in undo.iter().rev() {
                        tracing::info!(undo = %u, "rollback kroku");
                    }
                    return ExecOutcome {
                        ok: false,
                        rolled_back: !undo.is_empty(),
                        detail: format!(
                            "krok {step} u {target} selhal (vynucené); {} kroků vráceno",
                            undo.len()
                        ),
                    };
                }
                undo.push(format!("undo kroku {step} u {target}"));
            }
            ExecOutcome {
                ok: true,
                rolled_back: false,
                detail: format!("3 kroky u {target} provedeny"),
            }
        }
        // CheckProc nic nemění — „provedení“ je no-op.
        Action::CheckProc { pid, .. } => ExecOutcome {
            ok: true,
            rolled_back: false,
            detail: format!("proces {pid} ověřen, žádná mutace"),
        },
    }
}

/// Fáze 4 — OVĚŘENÍ: výsledek proti živému stavu. NIKDY se mlčky
/// netvářit, že akce proběhla (SPEC 17.4).
pub fn verify(action: &Action) -> bool {
    match action {
        Action::TestToggle { key, on } => toggle_state(key) == Some(*on),
        // TestOp nemá trvalý stav — úspěch hlásí execute, orchestrátor
        // ho nepřepisuje; CheckProc nic neměnil.
        Action::TestOp { .. } => true,
        Action::CheckProc { .. } => true,
    }
}

/// Popis vratnosti pro audit (sloupec `reversible`, SPEC 17.6).
pub fn reversible_hint(action: &Action) -> Option<String> {
    match action {
        Action::TestToggle { key, on } => Some(format!("přepnout {key} zpět na {}", !on)),
        Action::TestOp { .. } => Some("kroky mají undo (testovací)".into()),
        Action::CheckProc { .. } => None, // nic se nemění
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_roundtrip_and_verify() {
        let a = Action::TestToggle {
            key: "test:t1".into(),
            on: true,
        };
        assert!(execute(&a).ok);
        assert!(verify(&a));
        let back = Action::TestToggle {
            key: "test:t1".into(),
            on: false,
        };
        assert!(execute(&back).ok);
        assert!(verify(&back));
    }

    #[test]
    fn failed_step_rolls_back() {
        let a = Action::TestOp {
            target: "demo".into(),
            fail_at: Some(2),
        };
        let out = execute(&a);
        assert!(!out.ok);
        assert!(out.rolled_back, "krok 1 proběhl → musí se vrátit");
    }

    #[test]
    fn fail_at_first_step_has_nothing_to_roll_back() {
        let a = Action::TestOp {
            target: "demo".into(),
            fail_at: Some(1),
        };
        let out = execute(&a);
        assert!(!out.ok);
        assert!(!out.rolled_back);
    }
}
