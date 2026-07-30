//! Brána v8D — odinstalace se spouští v relaci uživatele, ne ve službě.
//! `cargo run -p ipc --example v8dcheck`
//!
//! Nic neodinstalovává. Ověřuje, že služba odinstalátor NESPOUŠTÍ a že
//! příkaz najde i u aplikací nainstalovaných „jen pro mě" (HKU hive).

use core_types::action::Action;

fn main() {
    let mut fail = 0;

    // 1) Verze protokolu — služba musí běžet na nové binárce.
    match ipc::client::ping() {
        Ok(p) if p.protocol_version == core_types::ipc::PROTOCOL_VERSION => {
            println!("OK  protokol v{}", p.protocol_version)
        }
        Ok(p) => {
            fail += 1;
            println!(
                "!!  služba běží na v{}, čekáme v{}",
                p.protocol_version,
                core_types::ipc::PROTOCOL_VERSION
            );
        }
        Err(e) => {
            println!("!!  služba neodpovídá: {e}");
            std::process::exit(1);
        }
    }

    // 2) Per-user instalace (HKU\<SID>) — služba jako SYSTEM je vidí.
    let apps = ipc::client::query_apps().unwrap_or_default();
    let mut with_cmd = 0;
    let mut sample = Vec::new();
    for a in apps.iter().take(400) {
        let key = a
            .identity_key
            .strip_prefix("app:")
            .unwrap_or(&a.identity_key);
        if validate::uninstall_command(key).is_some() {
            with_cmd += 1;
            if sample.len() < 3 {
                sample.push(a.display_name.clone());
            }
        }
    }
    if with_cmd > 0 {
        println!("OK  odinstalační příkaz nalezen u {with_cmd} aplikací (např. {sample:?})");
    } else {
        fail += 1;
        println!("!!  odinstalační příkaz se nenašel u žádné aplikace");
    }

    // 3) Neexistující plán se neschválí.
    match ipc::client::authorize_uninstall(999_999) {
        Ok(Err(d)) if d.verdict == "deny" => {
            println!(
                "OK  neznámý plán zamítnut: {}",
                d.deny_reason.unwrap_or_default()
            )
        }
        Ok(Ok((cmd, _))) => {
            fail += 1;
            println!("!!  neznámý plán vydal příkaz: {cmd}");
        }
        other => {
            fail += 1;
            println!("!!  nečekaná odpověď: {other:?}");
        }
    }

    // 4) TVRDÁ POJISTKA: server-side Execute odinstalace nic nespustí.
    let target = apps
        .iter()
        .find(|a| {
            let k = a
                .identity_key
                .strip_prefix("app:")
                .unwrap_or(&a.identity_key);
            validate::uninstall_command(k).is_some()
        })
        .map(|a| a.identity_key.clone());
    match target {
        Some(identity_key) => {
            let name = identity_key.clone();
            match ipc::client::plan_action(Action::UninstallApp { identity_key }) {
                Ok(Ok(plan)) => match ipc::client::execute_action(plan.plan_id) {
                    Ok(r) if r.outcome.as_deref() != Some("ok") => {
                        println!(
                            "OK  služba odinstalátor nespustila ({} / {:?}) — {name}",
                            r.verdict,
                            r.outcome.or(r.deny_reason)
                        );
                    }
                    Ok(r) => {
                        fail += 1;
                        println!("!!  služba odinstalaci provedla sama: {r:?}");
                    }
                    Err(e) => {
                        fail += 1;
                        println!("!!  execute selhal: {e}");
                    }
                },
                Ok(Err(d)) => println!("OK  plán zamítnut: {:?}", d.deny_reason),
                Err(e) => {
                    fail += 1;
                    println!("!!  plán selhal: {e}");
                }
            }
        }
        None => println!("--  žádná aplikace s odinstalátorem — krok 4 přeskočen"),
    }

    println!("\n{}", if fail == 0 { "v8D: PASS" } else { "v8D: FAIL" });
}
