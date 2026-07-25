//! Orchestrátor mutujících akcí (v5, SPEC 17.4): plán → validace →
//! provedení → ověření. Jediné místo, které smí spustit exekutor —
//! a NIKDY bez `Verdict::Allow` z validate/.
//!
//! Audit se zapisuje synchronně vlastním spojením (mutace jsou vzácné,
//! id záznamu se vrací hned v odpovědi). Plány T1 mají expiraci —
//! zastaralý plán je při Execute zamítnut.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use core_types::action::{Action, ActionClass, ActionPlan, ActionResult};

/// Životnost T1 plánu (fáze 1 → potvrzení uživatelem).
const PLAN_TTL_S: i64 = 60;

pub struct Orchestrator {
    /// Vlastní zapisovací spojení pro audit (busy_timeout — writer
    /// vzorků má přednost, mutace jsou vzácné).
    audit_conn: Mutex<store::Connection>,
    plans: Mutex<HashMap<u64, ActionPlan>>,
    next_id: AtomicU64,
    /// Striktní režim (SPEC 17.5): nevratná T1 → bod obnovení.
    strict: bool,
}

impl Orchestrator {
    pub fn new(audit_conn: store::Connection) -> Orchestrator {
        Orchestrator {
            audit_conn: Mutex::new(audit_conn),
            plans: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            strict: true,
        }
    }

    /// Audit zápis (allow i deny — každá akce nechává stopu).
    fn audit(
        &self,
        action: &Action,
        verdict: &str,
        deny_reason: Option<&str>,
        outcome: Option<&str>,
        detail: Option<&str>,
    ) -> i64 {
        let conn = self.audit_conn.lock().expect("audit conn lock");
        let reversible = actor_toggle::reversible_hint(action);
        store::audit::insert(
            &conn,
            unix_now(),
            action.name(),
            &action.target(),
            action.class().as_str(),
            verdict,
            deny_reason,
            outcome,
            reversible.as_deref(),
            detail,
        )
        .unwrap_or_else(|e| {
            // Audit selhal → akce se stejně hlásí, ale hlasitě do logu.
            tracing::error!(error = %e, "zápis auditu selhal");
            -1
        })
    }

    /// T0: validace + provedení + ověření v jednom volání (< 50 ms).
    /// Bez potvrzení — akce je vratná z definice.
    pub fn toggle(&self, action: Action) -> ActionResult {
        let t0 = Instant::now();
        if action.class() != ActionClass::T0 {
            let reason = "akce není třídy T0 — vyžaduje plán a potvrzení".to_string();
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return deny_result(reason, t0, id);
        }
        // Startup položky sahají na Task Scheduler přes COM — obslužné
        // vlákno pipe ho potřebuje inicializované (idempotentní).
        win_sys::wic::init_com_for_thread();
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return deny_result(reason, t0, id);
        }
        let out = actor_toggle::execute(&action);
        // Fáze 4: ověření proti stavu — nikdy mlčky (SPEC 17.4).
        let verified = out.ok && actor_toggle::verify(&action);
        let outcome = if verified {
            "ok"
        } else if out.rolled_back {
            "rolled_back"
        } else {
            "failed"
        };
        let id = self.audit(&action, "allow", None, Some(outcome), Some(&out.detail));
        ActionResult {
            verdict: "allow".into(),
            deny_reason: None,
            outcome: Some(outcome.into()),
            duration_ms: t0.elapsed().as_millis() as u64,
            audit_id: id,
        }
    }

    /// T1 fáze 1 — PLÁN (+ časná validace pro UX; rozhodná validace
    /// běží znovu ČERSTVĚ při Execute).
    pub fn plan(&self, action: Action) -> Result<ActionPlan, ActionResult> {
        let t0 = Instant::now();
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return Err(deny_result(reason, t0, id));
        }
        let steps = actor_toggle::plan(&action);
        let plan = ActionPlan {
            plan_id: self.next_id.fetch_add(1, Ordering::SeqCst),
            class: action.class(),
            action,
            steps,
            expires_ts: unix_now() + PLAN_TTL_S,
        };
        self.plans
            .lock()
            .expect("plans lock")
            .insert(plan.plan_id, plan.clone());
        Ok(plan)
    }

    /// T1 fáze 2–4 — po potvrzení uživatelem. Plán je jednorázový.
    pub fn execute(&self, plan_id: u64) -> ActionResult {
        let t0 = Instant::now();
        let plan = self.plans.lock().expect("plans lock").remove(&plan_id);
        let Some(plan) = plan else {
            return deny_result("plán neexistuje (už proveden, nebo nikdy nevznikl)", t0, -1);
        };
        let action = plan.action;
        if unix_now() > plan.expires_ts {
            let reason = "plán vypršel — sestav ho znovu (stav systému se mohl změnit)";
            let id = self.audit(&action, "deny", Some(reason), None, None);
            return deny_result(reason, t0, id);
        }
        // Fáze 2: validace proti ŽIVÉMU stavu teď, ne při plánu (17.3).
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return deny_result(reason, t0, id);
        }
        // Striktní režim: nevratný krok → bod obnovení PŘED provedením.
        if self.strict && plan.steps.iter().any(|s| !s.reversible) {
            if let Err(e) = win_sys::restore::create_restore_point("Winsent: před akcí") {
                let reason = format!("bod obnovení se nepodařil ({e}) — akce zastavena");
                let id = self.audit(&action, "deny", Some(&reason), None, None);
                return deny_result(reason, t0, id);
            }
        }
        // Fáze 3 + 4.
        let out = actor_toggle::execute(&action);
        let verified = out.ok && actor_toggle::verify(&action);
        let outcome = if verified {
            "ok"
        } else if out.rolled_back {
            "rolled_back"
        } else {
            "failed"
        };
        let id = self.audit(&action, "allow", None, Some(outcome), Some(&out.detail));
        ActionResult {
            verdict: "allow".into(),
            deny_reason: None,
            outcome: Some(outcome.into()),
            duration_ms: t0.elapsed().as_millis() as u64,
            audit_id: id,
        }
    }
}

fn deny_result(reason: impl Into<String>, t0: Instant, audit_id: i64) -> ActionResult {
    ActionResult {
        verdict: "deny".into(),
        deny_reason: Some(reason.into()),
        outcome: None,
        duration_ms: t0.elapsed().as_millis() as u64,
        audit_id,
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orch() -> Orchestrator {
        let conn = store::Connection::open_in_memory().expect("in-memory DB");
        store::migrations::run(&conn).expect("migrace");
        Orchestrator::new(conn)
    }

    // Brána v5: expirovaný plán je při Execute odmítnut.
    #[test]
    fn expired_plan_rejected() {
        let o = orch();
        let plan = o
            .plan(Action::TestOp {
                target: "demo".into(),
                fail_at: None,
            })
            .expect("plán projde");
        o.plans
            .lock()
            .unwrap()
            .get_mut(&plan.plan_id)
            .unwrap()
            .expires_ts = 1;
        let r = o.execute(plan.plan_id);
        assert_eq!(r.verdict, "deny");
        assert!(r.deny_reason.unwrap().contains("vypršel"));
    }

    // T1 akce nesmí projít T0 (toggle) cestou — vyžaduje plán+potvrzení.
    #[test]
    fn t1_via_toggle_denied() {
        let o = orch();
        let r = o.toggle(Action::TestOp {
            target: "demo".into(),
            fail_at: None,
        });
        assert_eq!(r.verdict, "deny");
    }

    // Deny při toggle nechává auditní stopu.
    #[test]
    fn deny_leaves_audit_trail() {
        let o = orch();
        let r = o.toggle(Action::TestToggle {
            key: "spatny:klic".into(),
            on: true,
        });
        assert_eq!(r.verdict, "deny");
        assert!(r.audit_id > 0, "deny musí mít auditní záznam");
        let conn = o.audit_conn.lock().unwrap();
        let rows = store::audit::recent(&conn, 5).unwrap();
        assert_eq!(rows[0].verdict, "deny");
    }
}
