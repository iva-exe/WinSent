//! Brána v9D — Security. `cargo run -p ipc --example v9seccheck`
//!
//! Nejdůležitější kontrola celé fáze (ROADMAP v9): NIKDY nepředstírat
//! tvrdý zámek u Win32 aplikací — `enforced` smí nést jen balené.
//! Falešný pocit ochrany je horší než žádný.

use std::time::Instant;

fn main() {
    let mut fail = 0;

    match ipc::client::ping() {
        Ok(p) if p.protocol_version == core_types::ipc::PROTOCOL_VERSION => {
            println!("OK  protokol v{}", p.protocol_version)
        }
        other => {
            println!("!!  služba: {other:?}");
            std::process::exit(1);
        }
    }

    let t0 = Instant::now();
    let r = match ipc::client::query_security() {
        Ok(r) => r,
        Err(e) => {
            println!("!!  dotaz selhal: {e}");
            std::process::exit(1);
        }
    };
    let first_ms = t0.elapsed().as_millis();
    let p = &r.protection;

    // 1) Stav ochrany: na desktopu je vždy aspoň jeden AV (Defender)
    // a firewall profily jdou přečíst.
    if p.av.is_empty() {
        println!("--  Security Center nehlásí žádný AV (server edice?)");
    } else {
        for (name, enabled, fresh) in &p.av {
            println!("OK  AV: {name} — běží: {enabled}, aktuální: {fresh}");
        }
    }
    if p.fw_private.is_none() && p.fw_public.is_none() {
        fail += 1;
        println!("!!  firewall profily nejdou přečíst");
    } else {
        println!(
            "OK  firewall — doména: {:?}, privátní: {:?}, veřejná: {:?}",
            p.fw_domain, p.fw_private, p.fw_public
        );
    }
    println!(
        "OK  SecureBoot: {:?} · TPM: {:?} · UAC: {} (prompt {:?}) · šifrování: {:?}",
        p.secure_boot, p.tpm, p.uac_enabled, p.uac_admin_prompt, p.encryption
    );

    // 2) Oprávnění: živý stroj má záznamy a kamera/mikrofon nechybí.
    if r.permissions.is_empty() {
        fail += 1;
        println!("!!  ConsentStore prázdný — čtení HKU selhalo");
    } else {
        let caps: std::collections::BTreeSet<_> = r
            .permissions
            .iter()
            .map(|p| p.capability.as_str())
            .collect();
        println!(
            "OK  {} oprávnění ve {} kategoriích: {:?}",
            r.permissions.len(),
            caps.len(),
            caps
        );
    }

    // 3) NEJDŮLEŽITĚJŠÍ: enforced jen u balených aplikací. Cesta
    // k .exe s enforced=true by v UI ukázala zelený zámek, který
    // neexistuje.
    let lying: Vec<_> = r
        .permissions
        .iter()
        .filter(|p| p.enforced && p.app.contains('\\'))
        .collect();
    if lying.is_empty() {
        let (pack, win32): (Vec<_>, Vec<_>) = r.permissions.iter().partition(|p| p.enforced);
        println!(
            "OK  vynucení poctivé: {} balených (vynucené), {} Win32 (poradní)",
            pack.len(),
            win32.len()
        );
        // Desktop s prohlížečem má NonPackaged záznamy vždy (Chrome,
        // Edge…). Nula znamená, že se per-exe klíče zase zahazují.
        if win32.is_empty() {
            fail += 1;
            println!("!!  žádný Win32 záznam — NonPackaged čtení nefunguje");
        }
    } else {
        fail += 1;
        println!(
            "!!  {} Win32 záznamů označených jako vynucené:",
            lying.len()
        );
        for l in lying.iter().take(3) {
            println!("    {:?}", l.app);
        }
    }

    // 4) Konzistence živé tečky: in_use vyžaduje čas použití.
    for perm in &r.permissions {
        if perm.in_use && perm.last_used.is_none() {
            fail += 1;
            println!(
                "!!  in_use bez času: {} / {}",
                perm.capability, perm.app_name
            );
        }
    }
    let live: Vec<_> = r
        .permissions
        .iter()
        .filter(|p| p.in_use && p.allow)
        .collect();
    println!(
        "OK  živě používá: {:?}",
        live.iter()
            .map(|l| format!("{} → {}", l.app_name, l.capability))
            .collect::<Vec<_>>()
    );

    // 5) Rozpočet: druhé volání jde z cache ochrany + registru.
    let t1 = Instant::now();
    let _ = ipc::client::query_security();
    let again_ms = t1.elapsed().as_millis();
    if again_ms <= 300 {
        println!("OK  dotaz: první {first_ms} ms, další {again_ms} ms");
    } else {
        fail += 1;
        println!("!!  dotaz je drahý i z cache: {again_ms} ms");
    }

    println!("\n{}", if fail == 0 { "v9D: PASS" } else { "v9D: FAIL" });
}
