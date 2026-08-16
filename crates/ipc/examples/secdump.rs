//! Výpis oprávnění tak, jak je vidí služba.
//! `cargo run -p ipc --example secdump [schopnost]`
//!
//! Slouží k ladění seskupování: ConsentStore klíčuje záznamy cestou
//! k .exe, takže aplikace, která se instaluje do složky s číslem verze,
//! má v seznamu jeden řádek za každou verzi, kterou kdy měla. Tenhle
//! výpis ukazuje syrové klíče vedle sebe, ať je vidět, co mají společné.

fn main() {
    let want = std::env::args().nth(1);

    let rep = match ipc::client::query_security() {
        Ok(r) => r,
        Err(e) => {
            println!("!!  dotaz na Security selhal: {e}");
            std::process::exit(1);
        }
    };

    // Seskupení podle čitelného jména — právě to uživatel čte a právě
    // tam se duplicity projeví.
    let mut by_name: std::collections::BTreeMap<(&str, &str), Vec<&core_types::proc::PermissionRow>> =
        std::collections::BTreeMap::new();
    for p in &rep.permissions {
        if let Some(w) = &want {
            if !p.capability.eq_ignore_ascii_case(w) {
                continue;
            }
        }
        by_name
            .entry((p.capability.as_str(), p.app_name.as_str()))
            .or_default()
            .push(p);
    }

    let mut dupes = 0;
    for ((cap, name), rows) in &by_name {
        if rows.len() > 1 {
            dupes += 1;
        }
        println!("{cap} · {name}  ({} záznamů)", rows.len());
        for r in rows {
            println!(
                "    allow={} vynuceno={} používá={} naposledy={}  {}",
                r.allow,
                r.enforced,
                r.in_use,
                r.last_used.map(|t| t.to_string()).unwrap_or("—".into()),
                r.app
            );
        }
    }

    println!(
        "\n    celkem {} záznamů, {} skupin, z toho {dupes} s víc než jedním záznamem",
        rep.permissions.len(),
        by_name.len()
    );
}
