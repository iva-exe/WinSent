//! Gate: identita procesů se nesmí opírat o instalační adresář, který
//! ukazuje na kořen disku nebo na sdílený systémový adresář.
//!
//! Blender 2.93 má v registru `InstallLocation = D:\`, stuby vestavěných
//! aplikací `C:\WINDOWS\System32\`. Dokud se braly tak, jak jsou, hlásil
//! se `D:\steam\steam.exe` jako „blender" a `NVDisplay.Container.exe`
//! jako „mspaint-b330ad9e-…". Kontroluje se přímo kaskáda, ne služba —
//! chyba je v tabulkách, ne v přenosu.

use std::collections::HashMap;

fn main() {
    let tables = identity::load_tables();
    let mut buf = Vec::new();
    let procs = match win_sys::proc::snapshot_processes(&mut buf) {
        Ok(p) => p,
        Err(e) => {
            println!("FAIL: snímek procesů selhal: {e}");
            std::process::exit(1);
        }
    };

    let mut bad = 0usize;
    let mut by_app: HashMap<String, Vec<String>> = HashMap::new();
    for p in &procs {
        if p.pid == 0 {
            continue;
        }
        let path = win_sys::procinfo::image_path(p.pid);
        let id = identity::cascade::resolve(p.pid, &p.name, path.as_deref(), &tables);
        by_app
            .entry(id.app_name.clone())
            .or_default()
            .push(p.name.clone());

        // Jméno aplikace tvaru „mspaint-b330ad9e-f80b-…" je jméno
        // podklíče uninstallu, ne aplikace. Nesmí vzniknout.
        let looks_like_guid = id.app_name.matches('-').count() >= 4
            && id.app_name.chars().filter(|c| c.is_ascii_hexdigit()).count() > 20;
        if looks_like_guid {
            println!("FAIL: {} ({}) → app_name={}", p.name, p.pid, id.app_name);
            bad += 1;
        }
    }

    // Aplikace, pod kterou spadly procesy z nesouvisejících binárek,
    // je skoro jistě falešná shoda přes kořen disku.
    for (app, names) in &by_app {
        let mut uniq: Vec<&String> = names.iter().collect();
        uniq.sort();
        uniq.dedup();
        if uniq.len() >= 4 && app != "Windows" && app != "Winsent" {
            println!("POZOR: „{app}“ sdruží {} různých binárek: {:?}", uniq.len(), uniq);
        }
    }

    for probe in ["steam.exe", "NVDisplay.Container.exe", "explorer.exe"] {
        if let Some(p) = procs.iter().find(|p| p.name.eq_ignore_ascii_case(probe)) {
            let path = win_sys::procinfo::image_path(p.pid);
            let id = identity::cascade::resolve(p.pid, &p.name, path.as_deref(), &tables);
            println!("  {probe:<28} app={} key={}", id.app_name, id.identity_key);
        }
    }

    // Žádný instalační adresář nesmí být nadřazený jinému.
    //
    // Sběrné adresáře jsou legitimní záznamy, které seznam pevných jmen
    // nezachytí: Minecraft Launcher má `InstallLocation = D:\hry\` a
    // jeho binárka tam opravdu leží — jenže vedle ní i Genshin Impact
    // a Star Rail. Prefixová shoda pak celý adresář ohlásila jako
    // „Minecraft Launcher" s confidence Exact.
    for (loc, jmeno) in &tables.uninstall {
        // Na hranici komponenty, ne po znacích: „…\hollow knight" a
        // „…\hollow knight silksong" jsou sousedi, ne vnoření.
        if let Some((pod, kdo)) = tables.uninstall.iter().find(|(jiny, _)| {
            jiny.len() > loc.len()
                && jiny.starts_with(loc.as_str())
                && jiny.as_bytes()[loc.len()] == b'\\'
        }) {
            println!("FAIL: „{jmeno}\" ({loc}) je nadřazený „{kdo}\" ({pod})");
            bad += 1;
        }
    }
    println!("  instalačních adresářů v tabulce: {}", tables.uninstall.len());

    if bad > 0 {
        println!("FAIL: {bad} procesů s vymyšlenou identitou");
        std::process::exit(1);
    }
    println!("OK: identita bez falešných shod ({} procesů)", procs.len());
}
