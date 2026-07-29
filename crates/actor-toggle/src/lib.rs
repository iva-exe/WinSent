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
        Action::StartupToggle { id, on } => vec![PlanStep {
            description: format!(
                "{} položku {id} (vratné opačným přepnutím)",
                if *on { "povolit" } else { "zakázat" }
            ),
            reversible: true,
        }],
        // Ukončování procesů má vlastní exekutor (actor-proc).
        Action::KillProc { .. } => Vec::new(),
    }
}

/// Zápis startup položky (v6, SPEC kap. 7) — NIKDY mazání hodnoty:
/// Run/složky přes StartupApproved, úlohy přes Enabled, služby přes
/// start typ. Volá se jen po `Verdict::Allow`.
fn apply_startup(id: &str, on: bool) -> Result<String, String> {
    let Some((source, name)) = id.split_once('|') else {
        return Err("neplatný identifikátor".into());
    };
    match source {
        "run_user" | "run_machine" | "folder_user" | "folder_common" => {
            let machine = source == "run_machine";
            let sub = if source.starts_with("run") {
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
            } else {
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder"
            };
            let root = if machine {
                win_sys::registry::HKEY_LOCAL_MACHINE
            } else {
                win_sys::registry::HKEY_CURRENT_USER
            };
            // 12 bajtů: [0] = 0x02 povoleno / 0x03 zakázáno,
            // [4..12] = FILETIME okamžiku zákazu (0 u povolení).
            let mut data = [0u8; 12];
            data[0] = if on { 0x02 } else { 0x03 };
            if !on {
                let ft = filetime_now();
                data[4..12].copy_from_slice(&ft.to_le_bytes());
            }
            win_sys::registry::write_binary(root, sub, name, &data).map_err(|e| e.to_string())?;
            Ok(format!(
                "StartupApproved {name} = {}",
                if on { "on" } else { "off" }
            ))
        }
        "task" => {
            win_sys::tasksched::set_task_enabled(name, on).map_err(|e| e.to_string())?;
            Ok(format!("úloha {name} enabled={on}"))
        }
        "service" => {
            win_sys::services::set_service_auto_start(name, on).map_err(|e| e.to_string())?;
            Ok(format!(
                "služba {name} start={}",
                if on { "auto" } else { "ruční" }
            ))
        }
        other => Err(format!("zdroj {other} nelze přepínat")),
    }
}

/// Aktuální stav startup položky (fáze ověření — čte se ZNOVU z OS).
fn read_startup_state(id: &str) -> Option<bool> {
    let (source, name) = id.split_once('|')?;
    match source {
        "run_user" | "run_machine" | "folder_user" | "folder_common" => {
            let machine = source == "run_machine";
            let sub = if source.starts_with("run") {
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
            } else {
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder"
            };
            let root = if machine {
                win_sys::registry::HKEY_LOCAL_MACHINE
            } else {
                win_sys::registry::HKEY_CURRENT_USER
            };
            // Chybí-li hodnota, položka je povolená.
            Some(
                win_sys::registry::read_binary(root, sub, name)
                    .and_then(|d| d.first().map(|b| b & 0x01 == 0))
                    .unwrap_or(true),
            )
        }
        "task" => win_sys::tasksched::task_enabled(name),
        "service" => win_sys::registry::read_u64(
            win_sys::registry::HKEY_LOCAL_MACHINE,
            &format!(r"SYSTEM\CurrentControlSet\Services\{name}"),
            "Start",
        )
        .map(|t| t == 2),
        _ => None,
    }
}

/// Windows FILETIME (100ns od 1601) pro razítko zákazu.
fn filetime_now() -> u64 {
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    EPOCH_DIFF_100NS + secs * 10_000_000
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
        Action::StartupToggle { id, on } => match apply_startup(id, *on) {
            Ok(detail) => ExecOutcome {
                ok: true,
                rolled_back: false,
                detail,
            },
            // Jediný krok — není co vracet, stav zůstal původní.
            Err(reason) => ExecOutcome {
                ok: false,
                rolled_back: false,
                detail: format!("zápis selhal: {reason}"),
            },
        },
        // Kill patří actor-proc — sem se nikdy nedostane (orchestrátor
        // vybírá exekutor podle typu akce).
        Action::KillProc { .. } => ExecOutcome {
            ok: false,
            rolled_back: false,
            detail: "špatný exekutor pro ukončení procesu".into(),
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
        // Přečíst ZNOVU z OS — nikdy se netvářit, že zápis prošel.
        Action::StartupToggle { id, on } => read_startup_state(id) == Some(*on),
        Action::KillProc { .. } => false,
    }
}

/// Popis vratnosti pro audit (sloupec `reversible`, SPEC 17.6).
pub fn reversible_hint(action: &Action) -> Option<String> {
    match action {
        Action::TestToggle { key, on } => Some(format!("přepnout {key} zpět na {}", !on)),
        Action::TestOp { .. } => Some("kroky mají undo (testovací)".into()),
        Action::CheckProc { .. } => None, // nic se nemění
        Action::StartupToggle { id, on } => Some(format!(
            "přepnout {id} zpět na {}",
            if *on { "vypnuto" } else { "zapnuto" }
        )),
        // Ukončený proces se nevrátí — audit to musí říct rovnou.
        Action::KillProc { .. } => None,
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
