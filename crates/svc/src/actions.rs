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
        // Fáze 3 + 4: ověření proti stavu — nikdy mlčky (SPEC 17.4).
        let (verified, rolled_back, detail) = execute_for(&action);
        let outcome = if verified {
            "ok"
        } else if rolled_back {
            "rolled_back"
        } else {
            "failed"
        };
        let id = self.audit(&action, "allow", None, Some(outcome), Some(&detail));
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
        let steps = plan_for(&action);
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
        let (verified, rolled_back, detail) = execute_for(&action);
        let outcome = if verified {
            "ok"
        } else if rolled_back {
            "rolled_back"
        } else {
            "failed"
        };
        let id = self.audit(&action, "allow", None, Some(outcome), Some(&detail));
        ActionResult {
            verdict: "allow".into(),
            deny_reason: None,
            outcome: Some(outcome.into()),
            duration_ms: t0.elapsed().as_millis() as u64,
            audit_id: id,
        }
    }

    /// Odinstalace, fáze 2 — SCHVÁLENÍ místo provedení.
    ///
    /// Služba běží jako SYSTEM v session 0. Odinstalátor spuštěný odtud
    /// nemá viditelnou plochu (uživatel neodklikne dialogy), vidí HKCU
    /// SYSTEMu místo uživatelova a dostane práva, se kterými nepočítá —
    /// pozorovaný následek byl zásek systému a rozbité audio. Proto se
    /// tady jen znovu validuje živý stav, přečte se ČERSTVÝ příkaz
    /// z registru a zapíše audit; spuštění dělá UI ve své relaci.
    pub fn authorize_uninstall(&self, plan_id: u64) -> Result<(String, i64), ActionResult> {
        let t0 = Instant::now();
        let plan = self.plans.lock().expect("plans lock").remove(&plan_id);
        let Some(plan) = plan else {
            return Err(deny_result(
                "plán neexistuje (už použit, nebo nikdy nevznikl)",
                t0,
                -1,
            ));
        };
        let action = plan.action;
        let Action::UninstallApp { identity_key } = &action else {
            return Err(deny_result("plán není odinstalace", t0, -1));
        };
        if unix_now() > plan.expires_ts {
            let reason = "plán vypršel — sestav ho znovu (stav systému se mohl změnit)";
            let id = self.audit(&action, "deny", Some(reason), None, None);
            return Err(deny_result(reason, t0, id));
        }
        // Validace proti ŽIVÉMU stavu teď, ne při plánu (SPEC 17.3).
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return Err(deny_result(reason, t0, id));
        }
        // Příkaz čte vrstva sama z registru — UI ho nemůže podvrhnout.
        let name = identity_key.strip_prefix("app:").unwrap_or(identity_key);
        let Some(command) = validate::uninstall_command(name) else {
            let reason = "odinstalační příkaz se v registru nenašel";
            let id = self.audit(&action, "deny", Some(reason), None, None);
            return Err(deny_result(reason, t0, id));
        };
        // Odinstalace je nevratná → bod obnovení PŘED spuštěním.
        if self.strict {
            if let Err(e) = win_sys::restore::create_restore_point("Winsent: před odinstalací") {
                let reason = format!("bod obnovení se nepodařil ({e}) — akce zastavena");
                let id = self.audit(&action, "deny", Some(&reason), None, None);
                return Err(deny_result(reason, t0, id));
            }
        }
        // Výsledek zatím neznáme — doplní ho ReportUninstall (fáze 4).
        let id = self.audit(
            &action,
            "allow",
            None,
            Some("running"),
            Some(&format!("spouští UI v relaci uživatele: {command}")),
        );
        Ok((command, id))
    }

    /// Odinstalace, fáze 4 — OVĚŘENÍ po doběhnutí odinstalátoru.
    /// Rozhoduje registr, ne návratový kód odinstalátoru ani UI.
    pub fn report_uninstall(
        &self,
        audit_id: i64,
        identity_key: &str,
        detail: &str,
    ) -> ActionResult {
        let t0 = Instant::now();
        let action = Action::UninstallApp {
            identity_key: identity_key.to_string(),
        };
        let verified = actor_app::verify(&action);
        let outcome = if verified { "ok" } else { "failed" };
        if audit_id > 0 {
            let conn = self.audit_conn.lock().expect("audit conn lock");
            if let Err(e) = store::audit::set_outcome(&conn, audit_id, outcome, detail) {
                tracing::error!(error = %e, "doplnění výsledku do auditu selhalo");
            }
        }
        ActionResult {
            verdict: "allow".into(),
            deny_reason: None,
            outcome: Some(outcome.into()),
            duration_ms: t0.elapsed().as_millis() as u64,
            audit_id,
        }
    }
}

/// Plán podle typu akce — exekutor si vybírá orchestrátor, ne UI.
fn plan_for(action: &Action) -> Vec<core_types::action::PlanStep> {
    match action {
        Action::KillProc { .. } => actor_proc::plan(action),
        Action::DeleteFiles { .. } => actor_file::plan(action),

        Action::UninstallApp { .. } => actor_app::plan(action),
        _ => actor_toggle::plan(action),
    }
}

/// Provedení + ověření podle typu akce (fáze 3 a 4).
/// Vrací (ok, rolled_back, detail).
fn execute_for(action: &Action) -> (bool, bool, String) {
    match action {
        Action::KillProc { .. } => {
            let out = actor_proc::execute(action);
            let verified = out.ok && actor_proc::verify(action);
            (verified, false, out.detail)
        }
        Action::DeleteFiles { .. } => {
            let out = actor_file::execute(action);
            let verified = out.ok && actor_file::verify(action);
            (verified, false, out.detail)
        }

        // Pojistka: odinstalátor se ze služby NIKDY nespouští. Kdyby
        // sem akce přesto dorazila (nová cesta v kódu), skončí tady —
        // ne na neviditelné ploše session 0 pod účtem SYSTEM.
        Action::UninstallApp { .. } => (
            false,
            false,
            "odinstalátor spouští UI v relaci uživatele — služba ho nespouští".into(),
        ),
        _ => {
            let out = actor_toggle::execute(action);
            let verified = out.ok && actor_toggle::verify(action);
            (verified, out.rolled_back, out.detail)
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
