//! Typy mutujících akcí (v5, SPEC kap. 17). Sdílené mezi validate,
//! exekutory, svc a UI — samotná rozhodovací logika žije VÝHRADNĚ
//! v crate `validate`.

use serde::{Deserialize, Serialize};

/// Mutující akce. v5 obsahuje testovací akce na prověření vrstvy
/// (brána v5); reálné akce (startup toggle, kill…) přibudou v6/v7
/// a VŽDY projdou toutéž kaskádou.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// T0: testovací přepínač (in-memory, vratný, bez potvrzení).
    /// Klíč musí mít prefix `test:` — cokoliv jiného validátor zamítne.
    TestToggle { key: String, on: bool },
    /// T1: testovací operace. `fail_at` = uměle selže v kroku N
    /// (prověření rollbacku); cíl `fake:*` neexistuje (test zamítnutí).
    TestOp {
        target: String,
        fail_at: Option<u32>,
    },
    /// T1: validace živého procesu (bez mutace) — prověřuje čtení
    /// čerstvého stavu OS: existence (pid + create_time) a třída
    /// ochrany. Základ pro kill ve v7.
    CheckProc { pid: u32, create_time: i64 },
}

impl Action {
    /// Třída akce (SPEC 17.2): T0 rychlá a vratná, T1 těžká.
    pub fn class(&self) -> ActionClass {
        match self {
            Action::TestToggle { .. } => ActionClass::T0,
            Action::TestOp { .. } | Action::CheckProc { .. } => ActionClass::T1,
        }
    }

    /// Lidský popis cíle pro audit.
    pub fn target(&self) -> String {
        match self {
            Action::TestToggle { key, on } => format!("{key}={on}"),
            Action::TestOp { target, .. } => target.clone(),
            Action::CheckProc { pid, create_time } => format!("pid {pid} @{create_time}"),
        }
    }

    /// Název akce pro audit.
    pub fn name(&self) -> &'static str {
        match self {
            Action::TestToggle { .. } => "test_toggle",
            Action::TestOp { .. } => "test_op",
            Action::CheckProc { .. } => "check_proc",
        }
    }
}

/// Třída akce (SPEC 17.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionClass {
    T0,
    T1,
}

impl ActionClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionClass::T0 => "T0",
            ActionClass::T1 => "T1",
        }
    }
}

/// Jeden krok plánu (fáze 1). Exekutor nic nemění, jen popisuje.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanStep {
    pub description: String,
    pub reversible: bool,
}

/// Plán T1 akce vrácený do UI (fáze 1 → potvrzení uživatelem).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionPlan {
    pub plan_id: u64,
    pub action: Action,
    pub class: ActionClass,
    pub steps: Vec<PlanStep>,
    /// Po tomto čase je Execute zamítnut (plán zastaral).
    pub expires_ts: i64,
}

/// Výsledek akce (T0 přímo, T1 po Execute) — vždy s auditní stopou.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    /// allow | deny
    pub verdict: String,
    pub deny_reason: Option<String>,
    /// ok | failed | rolled_back (jen u allow)
    pub outcome: Option<String>,
    pub duration_ms: u64,
    pub audit_id: i64,
}

/// Řádek auditu pro UI (SPEC 17.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRow {
    pub id: i64,
    pub ts: i64,
    pub action: String,
    pub target: String,
    pub class: String,
    pub verdict: String,
    pub deny_reason: Option<String>,
    pub outcome: Option<String>,
    pub reversible: Option<String>,
}
