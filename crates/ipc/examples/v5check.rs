//! Prověření brány v5: `cargo run -p ipc --example v5check`.
//! Testuje cesty selhání víc než cesty úspěchu (ROADMAP v5).

use core_types::action::Action;

fn main() {
    let mut fails = 0u32;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "OK " } else { "FAIL" });
        if !ok {
            fails += 1;
        }
    };

    // 1. T0 přepínač: allow, ok, pod 50 ms.
    match ipc::client::toggle_action(Action::TestToggle {
        key: "test:v5".into(),
        on: true,
    }) {
        Ok(r) => check(
            "T0 toggle",
            r.verdict == "allow" && r.outcome.as_deref() == Some("ok") && r.duration_ms < 50,
            format!(
                "{} / {:?} / {} ms / audit {}",
                r.verdict, r.outcome, r.duration_ms, r.audit_id
            ),
        ),
        Err(e) => check("T0 toggle", false, e.to_string()),
    }

    // 2. T0 s neznámým klíčem: DENY.
    match ipc::client::toggle_action(Action::TestToggle {
        key: "startup:zle".into(),
        on: true,
    }) {
        Ok(r) => check(
            "T0 deny neznámý klíč",
            r.verdict == "deny",
            format!("{} / {:?}", r.verdict, r.deny_reason),
        ),
        Err(e) => check("T0 deny neznámý klíč", false, e.to_string()),
    }

    // 3. T1 na fake cíli: DENY už při plánu.
    match ipc::client::plan_action(Action::TestOp {
        target: "fake:cil".into(),
        fail_at: None,
    }) {
        Ok(Err(r)) => check(
            "T1 deny fake cíl",
            r.verdict == "deny",
            format!("{:?}", r.deny_reason),
        ),
        Ok(Ok(_)) => check("T1 deny fake cíl", false, "plán prošel!".into()),
        Err(e) => check("T1 deny fake cíl", false, e.to_string()),
    }

    // 4. T1 plná kaskáda: plán → execute → ok.
    match ipc::client::plan_action(Action::TestOp {
        target: "demo".into(),
        fail_at: None,
    }) {
        Ok(Ok(plan)) => {
            let steps = plan.steps.len();
            match ipc::client::execute_action(plan.plan_id) {
                Ok(r) => check(
                    "T1 plán+execute",
                    r.verdict == "allow" && r.outcome.as_deref() == Some("ok"),
                    format!("{steps} kroků / {:?}", r.outcome),
                ),
                Err(e) => check("T1 plán+execute", false, e.to_string()),
            }
        }
        other => check("T1 plán+execute", false, format!("{other:?}")),
    }

    // 5. Selhání ve fázi 3 → FAILED + rollback (ne mlčky).
    match ipc::client::plan_action(Action::TestOp {
        target: "demo".into(),
        fail_at: Some(2),
    }) {
        Ok(Ok(plan)) => match ipc::client::execute_action(plan.plan_id) {
            Ok(r) => check(
                "T1 rollback při selhání",
                r.outcome.as_deref() == Some("rolled_back"),
                format!("{:?}", r.outcome),
            ),
            Err(e) => check("T1 rollback při selhání", false, e.to_string()),
        },
        other => check("T1 rollback při selhání", false, format!("{other:?}")),
    }

    // 6. Dvojí execute téhož plánu: druhý pokus zamítnut (jednorázový).
    match ipc::client::plan_action(Action::TestOp {
        target: "demo2".into(),
        fail_at: None,
    }) {
        Ok(Ok(plan)) => {
            let _ = ipc::client::execute_action(plan.plan_id);
            match ipc::client::execute_action(plan.plan_id) {
                Ok(r) => check(
                    "T1 plán je jednorázový",
                    r.verdict == "deny",
                    format!("{:?}", r.deny_reason),
                ),
                Err(e) => check("T1 plán je jednorázový", false, e.to_string()),
            }
        }
        other => check("T1 plán je jednorázový", false, format!("{other:?}")),
    }

    // 7. CheckProc na neexistujícím PID: DENY (živá data).
    match ipc::client::plan_action(Action::CheckProc {
        pid: 4_000_000_001,
        create_time: 1,
    }) {
        Ok(Err(r)) => check(
            "živý cíl neexistuje → deny",
            r.verdict == "deny",
            format!("{:?}", r.deny_reason),
        ),
        other => check("živý cíl neexistuje → deny", false, format!("{other:?}")),
    }

    // 8. Audit: záznamy allow i deny existují.
    match ipc::client::query_audit(20) {
        Ok(rows) => {
            let allow = rows.iter().filter(|r| r.verdict == "allow").count();
            let deny = rows.iter().filter(|r| r.verdict == "deny").count();
            check(
                "audit stopy",
                allow >= 2 && deny >= 2,
                format!("{} záznamů ({allow} allow, {deny} deny)", rows.len()),
            );
        }
        Err(e) => check("audit stopy", false, e.to_string()),
    }

    println!("\ncelkem selhání: {fails}");
    std::process::exit(if fails == 0 { 0 } else { 1 });
}
