//! Diagnostika: které běžící aplikace NEMAJÍ ikonu a proč.
//! `cargo run -p ipc --example iconmiss`

fn main() {
    let procs = match ipc::client::query_procs() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("query_procs selhal: {e}");
            std::process::exit(1);
        }
    };
    // Unikátní aplikace + jeden reprezentativní proces.
    let mut seen: std::collections::BTreeMap<String, (String, u32)> =
        std::collections::BTreeMap::new();
    for p in &procs {
        seen.entry(p.identity_key.clone())
            .or_insert((p.app_name.clone(), p.pid));
    }
    let mut missing = Vec::new();
    let mut have = 0usize;
    for (key, (app, pid)) in &seen {
        match ipc::client::query_icon(key.clone()) {
            Ok(Some(_)) => have += 1,
            _ => missing.push((key.clone(), app.clone(), *pid)),
        }
    }
    println!(
        "aplikaci: {}, s ikonou: {have}, bez: {}",
        seen.len(),
        missing.len()
    );
    for (key, app, pid) in &missing {
        println!("  BEZ IKONY: {app}  [{key}]  pid={pid}");
    }

    // Inventář (nainstalované, i neběžící) — tady se pozná, jestli
    // chybějící ikony jsou runtime balíčky, nebo skutečné aplikace.
    let apps = ipc::client::query_apps().unwrap_or_default();
    let mut miss_apps = Vec::new();
    let mut have_apps = 0usize;
    for a in &apps {
        match ipc::client::query_icon(a.identity_key.clone()) {
            Ok(Some(_)) => have_apps += 1,
            _ => miss_apps.push(a),
        }
    }
    println!(
        "\ninventar: {}, s ikonou: {have_apps}, bez: {}",
        apps.len(),
        miss_apps.len()
    );
    for a in miss_apps.iter().take(25) {
        println!(
            "  BEZ: {}  [{}]  cest={}",
            a.display_name, a.kind, a.path_count
        );
    }
}
