//! Brána v7: `cargo run -p ipc --example killcheck` (elevovaně).
//! Ověřuje, že kill kritických procesů je zamítnutý PŘED provedením,
//! a že ukončení vlastního testovacího procesu projde celou kaskádou.

use core_types::action::Action;

fn main() {
    let mut fails = 0;

    // 1) Kritický proces (System, pid 4) — musí být zamítnut.
    let procs = ipc::client::query_procs().expect("query_procs");
    if let Some(sys) = procs.iter().find(|p| p.pid == 4) {
        match ipc::client::plan_action(Action::KillProc {
            pid: sys.pid,
            create_time: sys.create_time,
            tree: false,
        }) {
            Ok(Err(d)) => println!("System (pid 4): deny {:?}", d.deny_reason),
            other => {
                println!("CHYBA: System nebyl zamítnut: {other:?}");
                fails += 1;
            }
        }
    }

    // 2) Chráněné/systémové procesy — namátkou.
    for name in ["csrss.exe", "wininit.exe", "services.exe", "lsass.exe"] {
        let Some(p) = procs.iter().find(|p| p.name.eq_ignore_ascii_case(name)) else {
            continue;
        };
        match ipc::client::plan_action(Action::KillProc {
            pid: p.pid,
            create_time: p.create_time,
            tree: false,
        }) {
            Ok(Err(d)) => println!("{name}: deny {:?}", d.deny_reason),
            other => {
                println!("CHYBA: {name} nebyl zamítnut: {other:?}");
                fails += 1;
            }
        }
    }

    // 3) Recyklovaný PID (špatná instance) — musí být zamítnut.
    if let Some(p) = procs
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case("explorer.exe"))
    {
        match ipc::client::plan_action(Action::KillProc {
            pid: p.pid,
            create_time: p.create_time + 1,
            tree: false,
        }) {
            Ok(Err(d)) => println!("špatná instance: deny {:?}", d.deny_reason),
            other => {
                println!("CHYBA: špatná instance neodmítnuta: {other:?}");
                fails += 1;
            }
        }
    }

    // 4) Vlastní testovací proces — celá kaskáda až k ukončení.
    let child = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", "Start-Sleep 120"])
        .spawn()
        .expect("spuštění testovacího procesu");
    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_secs(3));
    let procs = ipc::client::query_procs().expect("query_procs");
    let Some(target) = procs.iter().find(|p| p.pid == pid) else {
        println!("CHYBA: testovací proces {pid} sampler nevidí");
        std::process::exit(1);
    };
    let action = Action::KillProc {
        pid,
        create_time: target.create_time,
        tree: true,
    };
    match ipc::client::plan_action(action) {
        Ok(Ok(plan)) => {
            println!("plán ({} kroků):", plan.steps.len());
            for s in &plan.steps {
                println!("  - {}", s.description);
            }
            match ipc::client::execute_action(plan.plan_id) {
                Ok(r) => {
                    println!("výsledek: {} / {:?}", r.verdict, r.outcome);
                    if r.outcome.as_deref() != Some("ok") {
                        fails += 1;
                    }
                }
                Err(e) => {
                    println!("CHYBA execute: {e}");
                    fails += 1;
                }
            }
        }
        other => {
            println!("CHYBA: plán testovacího procesu selhal: {other:?}");
            fails += 1;
        }
    }
    // Ověření z druhé strany: proces opravdu zmizel.
    std::thread::sleep(std::time::Duration::from_secs(2));
    let procs = ipc::client::query_procs().expect("query_procs");
    if procs.iter().any(|p| p.pid == pid) {
        println!("CHYBA: proces {pid} pořád běží");
        fails += 1;
    } else {
        println!("testovací proces ukončen ✓");
    }

    println!("\ncelkem selhání: {fails}");
    std::process::exit(if fails == 0 { 0 } else { 1 });
}
