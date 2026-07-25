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

        // ── T1: kontrola živého procesu — ČERSTVÉ čtení OS, žádná
        // cache. Vzor pro kill ve v7.
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Cesty selhání se testují víc než cesty úspěchu (brána v5) ──

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
