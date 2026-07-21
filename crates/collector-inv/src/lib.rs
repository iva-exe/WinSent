//! collector-inv — inventář aplikací + mapa souborů (SPEC kap. 5).
//!
//! „Co mám nainstalované a kde všude to má soubory." Zdroje seznamu:
//! registry Uninstall (3 kořeny), MSI, MSIX. Mapa souborů se skládá
//! sestupně dle spolehlivosti a KAŽDÁ cesta nese zdroj + confidence:
//! MSI komponenty `Exact`, MSIX lokace `Exact`, registry `High`,
//! heuristika `Guess` — nikdy netvrdit, že víme, když jen hádáme.
//!
//! POMALÉ — sken běží výhradně na pozadí (BELOW_NORMAL vlákno svc),
//! on-demand nebo řídce, nikdy v samplovacím cyklu. Jen čte.

use std::collections::HashMap;

use core_types::config::Config;

/// Chyby této crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {}

/// Stav kolektoru (sken je bezstavový).
pub struct State;

/// Inicializace kolektoru při startu služby.
pub fn init(_cfg: &Config) -> Result<State, Error> {
    Ok(State)
}

/// Jeden krok sběru (nic průběžného — sken je on-demand).
pub fn tick(_state: &mut State) -> Result<(), Error> {
    Ok(())
}

/// Korektní ukončení kolektoru.
pub fn shutdown(_state: State) {}

/// Jedna cesta v mapě souborů aplikace.
#[derive(Debug, Clone)]
pub struct PathEntry {
    /// Souborová cesta, nebo registry větev (role == "registry").
    pub path: String,
    /// install | config | data | cache | logs | registry
    pub role: &'static str,
    /// msi | msix | registry | heuristic
    pub source: &'static str,
    /// exact | high | guess
    pub confidence: &'static str,
}

/// Jedna aplikace z inventáře.
#[derive(Debug, Clone)]
pub struct AppEntry {
    /// Klíč shodný s identity kaskádou (`app:{name_lc}` / `msix:{family}`)
    /// — spojuje inventář s běžícími procesy i ikonami.
    pub identity_key: String,
    /// os | desktop | msix
    pub kind: &'static str,
    pub display_name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    /// InstallDate „YYYYMMDD“ → unix (půlnoc), když jde naparsovat.
    pub install_ts: Option<i64>,
    pub paths: Vec<PathEntry>,
}

/// Kompletní sken inventáře. Trvá sekundy — volat z pozadí.
pub fn scan() -> Vec<AppEntry> {
    let mut apps: HashMap<String, AppEntry> = HashMap::new();

    // ── MSI produkty + mapa komponent (Exact) ──
    let msi_products = win_sys::msi::products();
    let mut msi_paths = win_sys::msi::component_paths();
    let mut msi_by_code: HashMap<String, &win_sys::msi::MsiProduct> = HashMap::new();
    for p in &msi_products {
        msi_by_code.insert(p.code.clone(), p);
    }

    // ── Uninstall registry (základ seznamu, High) ──
    for u in uninstall_entries() {
        let name = u.display_name.clone();
        let key = format!("app:{}", name.to_ascii_lowercase());
        let entry = apps.entry(key.clone()).or_insert_with(|| AppEntry {
            identity_key: key,
            kind: "desktop",
            display_name: name,
            publisher: None,
            version: None,
            install_ts: None,
            paths: Vec::new(),
        });
        entry.publisher = entry.publisher.take().or(u.publisher);
        entry.version = entry.version.take().or(u.version);
        entry.install_ts = entry.install_ts.take().or(u.install_ts);

        if let Some(loc) = u
            .install_location
            .as_deref()
            .filter(|l| !l.trim().is_empty())
        {
            push_unique(
                &mut entry.paths,
                PathEntry {
                    path: loc.trim_end_matches('\\').to_string(),
                    role: "install",
                    source: "registry",
                    confidence: "high",
                },
            );
        }
        // GUID klíč = MSI produkt → mapa komponent je Exact.
        if let Some(paths) = msi_paths.remove(&u.key_name) {
            for dir in collapse_dirs(paths, 12) {
                push_unique(
                    &mut entry.paths,
                    PathEntry {
                        path: dir,
                        role: "install",
                        source: "msi",
                        confidence: "exact",
                    },
                );
            }
        }
        if let Some(msi) = msi_by_code.get(&u.key_name) {
            entry.publisher = entry.publisher.take().or_else(|| msi.publisher.clone());
            entry.version = entry.version.take().or_else(|| msi.version.clone());
        }
    }

    // ── MSI produkty bez Uninstall záznamu (vzácné) ──
    for p in &msi_products {
        let Some(name) = p.name.clone().filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        let key = format!("app:{}", name.to_ascii_lowercase());
        if apps.contains_key(&key) {
            continue;
        }
        let mut paths = Vec::new();
        if let Some(loc) = p
            .install_location
            .as_deref()
            .filter(|l| !l.trim().is_empty())
        {
            paths.push(PathEntry {
                path: loc.trim_end_matches('\\').to_string(),
                role: "install",
                source: "msi",
                confidence: "exact",
            });
        }
        if let Some(comp) = msi_paths.remove(&p.code) {
            for dir in collapse_dirs(comp, 12) {
                push_unique(
                    &mut paths,
                    PathEntry {
                        path: dir,
                        role: "install",
                        source: "msi",
                        confidence: "exact",
                    },
                );
            }
        }
        apps.insert(
            key.clone(),
            AppEntry {
                identity_key: key,
                kind: "desktop",
                display_name: name,
                publisher: p.publisher.clone(),
                version: p.version.clone(),
                install_ts: parse_install_date(p.install_date.as_deref()),
                paths,
            },
        );
    }

    // ── MSIX balíčky (Exact lokace + kontejner) ──
    for pkg in win_sys::msix::packages() {
        let key = format!("msix:{}", pkg.family);
        let mut paths = Vec::new();
        if let Some(p) = pkg.install_path.clone() {
            paths.push(PathEntry {
                path: p,
                role: "install",
                source: "msix",
                confidence: "exact",
            });
        }
        // Kontejner s daty balíčku v každém uživatelském profilu.
        for profile in user_profiles() {
            let c = format!("{profile}\\AppData\\Local\\Packages\\{}", pkg.family);
            if std::path::Path::new(&c).is_dir() {
                paths.push(PathEntry {
                    path: c,
                    role: "data",
                    source: "msix",
                    confidence: "exact",
                });
            }
        }
        apps.insert(
            key.clone(),
            AppEntry {
                identity_key: key,
                kind: "msix",
                display_name: pkg.display_name,
                publisher: pkg.publisher,
                version: pkg.version,
                install_ts: None,
                paths,
            },
        );
    }

    // ── Heuristika (Guess) + registry větve — pro desktop aplikace ──
    let profiles = user_profiles();
    let programdata =
        std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    for app in apps.values_mut() {
        if app.kind != "desktop" {
            continue;
        }
        heuristic_paths(app, &profiles, &programdata);
        registry_branches(app);
    }

    let mut list: Vec<AppEntry> = apps.into_values().collect();
    list.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    list
}

/// Záznam z Uninstall registru.
struct UninstallEntry {
    /// Název podklíče (u MSI je to ProductCode `{GUID}`).
    key_name: String,
    display_name: String,
    publisher: Option<String>,
    version: Option<String>,
    install_location: Option<String>,
    install_ts: Option<i64>,
}

/// Načte Uninstall záznamy ze tří kořenů. Systémové komponenty
/// (SystemComponent=1) a KB aktualizace se přeskakují.
fn uninstall_entries() -> Vec<UninstallEntry> {
    use win_sys::registry::{
        enum_subkeys, read_string, read_u64, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    };
    let roots = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ];
    let mut out = Vec::new();
    for (root, base) in roots {
        for sub in enum_subkeys(root, base) {
            let key = format!("{base}\\{sub}");
            let Some(name) = read_string(root, &key, "DisplayName") else {
                continue; // bez jména to není aplikace pro uživatele
            };
            if read_u64(root, &key, "SystemComponent") == Some(1) {
                continue;
            }
            // KB aktualizace a hotfixy nepatří do seznamu aplikací.
            if name.starts_with("KB") && name[2..].chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let install_ts = parse_install_date(read_string(root, &key, "InstallDate").as_deref());
            out.push(UninstallEntry {
                key_name: sub,
                display_name: name,
                publisher: read_string(root, &key, "Publisher"),
                version: read_string(root, &key, "DisplayVersion"),
                install_location: read_string(root, &key, "InstallLocation"),
                install_ts,
            });
        }
    }
    out
}

/// „YYYYMMDD“ → unix ts (UTC půlnoc). Hrubé, ale na řazení stačí.
fn parse_install_date(s: Option<&str>) -> Option<i64> {
    let s = s?.trim();
    if s.len() != 8 || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: i64 = s[4..6].parse().ok()?;
    let d: i64 = s[6..8].parse().ok()?;
    if !(1970..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Dny od epochy (civil → days, Howard Hinnant algoritmus, zkrácený).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400)
}

/// Uživatelské profily s AppData (SYSTEM nemá smysluplné %LOCALAPPDATA%,
/// heuristika proto prochází reálné profily v C:\Users).
fn user_profiles() -> Vec<String> {
    let mut out = Vec::new();
    let users = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + r"\Users";
    let Ok(rd) = std::fs::read_dir(&users) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if matches!(
            name.as_str(),
            "Default" | "Default User" | "Public" | "All Users"
        ) {
            continue;
        }
        if p.join("AppData").is_dir() {
            out.push(p.to_string_lossy().into_owned());
        }
    }
    out
}

/// Varianty jména vydavatele: plné + bez právních přípon + první slovo
/// („Google LLC" → i „Google" — reálné adresáře používají krátký tvar).
fn publisher_variants(publisher: &str) -> Vec<String> {
    let mut v = Vec::new();
    let full = publisher.trim();
    if full.is_empty() {
        return v;
    }
    v.push(full.to_string());
    const SUFFIXES: &[&str] = &[
        "inc.",
        "inc",
        "llc",
        "corporation",
        "corp.",
        "corp",
        "ltd.",
        "ltd",
        "gmbh",
        "s.r.o.",
        "a.s.",
        "co.",
        "company",
        "software",
        "technologies",
    ];
    let mut words: Vec<&str> = full.split_whitespace().collect();
    while let Some(last) = words.last() {
        let l = last.to_lowercase();
        if SUFFIXES.contains(&l.trim_end_matches(',')) {
            words.pop();
        } else {
            break;
        }
    }
    let stripped = words.join(" ");
    if !stripped.is_empty() && stripped != full {
        v.push(stripped.clone());
    }
    if let Some(first) = words.first() {
        if *first != full && !v.iter().any(|x| x == first) {
            v.push(first.to_string());
        }
    }
    v
}

/// Varianty jména produktu: plné + bez slova vydavatele na začátku
/// („Google Chrome" → i „Chrome").
fn product_variants(product: &str, publisher_first: Option<&str>) -> Vec<String> {
    let mut v = vec![product.to_string()];
    if let Some(pf) = publisher_first {
        let prefix = format!("{pf} ");
        if let Some(rest) = product.strip_prefix(&prefix) {
            if !rest.is_empty() {
                v.push(rest.to_string());
            }
        }
    }
    v
}

/// Heuristické cesty dle SPEC 5.2 — jen ty, co na disku existují.
/// Kombinuje varianty vydavatele × produktu (adresáře bývají krátké
/// tvary: `Google\Chrome`, ne `Google LLC\Google Chrome`).
fn heuristic_paths(app: &mut AppEntry, profiles: &[String], programdata: &str) {
    let publisher = app.publisher.clone().unwrap_or_default();
    let pubs = publisher_variants(&publisher);
    let prods = product_variants(&app.display_name, pubs.last().map(|s| s.as_str()));

    let candidates_in = |base: &str| -> Vec<String> {
        let mut v = Vec::new();
        for p in &pubs {
            for pr in &prods {
                v.push(format!("{base}\\{p}\\{pr}"));
            }
        }
        for pr in &prods {
            v.push(format!("{base}\\{pr}"));
        }
        v
    };
    let mut found_dirs: Vec<String> = Vec::new();
    let mut try_push = |path: String, found: &mut Vec<String>| {
        if std::path::Path::new(&path).is_dir() {
            let role = role_of(&path);
            found.push(path.clone());
            push_unique(
                &mut app.paths,
                PathEntry {
                    path,
                    role,
                    source: "heuristic",
                    confidence: "guess",
                },
            );
        }
    };
    for profile in profiles {
        for base in [
            format!("{profile}\\AppData\\Local"),
            format!("{profile}\\AppData\\Roaming"),
        ] {
            for c in candidates_in(&base) {
                try_push(c, &mut found_dirs);
            }
        }
        for pr in &prods {
            try_push(
                format!("{profile}\\.{}", pr.to_lowercase().replace(' ', "")),
                &mut found_dirs,
            );
            try_push(format!("{profile}\\Documents\\{pr}"), &mut found_dirs);
        }
    }
    for c in candidates_in(programdata) {
        try_push(c, &mut found_dirs);
    }

    // Úroveň hlouběji: cache/logs/User Data podadresáře nalezených cest
    // (SPEC příklad Chrome: User Data i Cache jako samostatné řádky).
    for dir in found_dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let nlc = name.to_lowercase();
            let role = if nlc.contains("cache") {
                "cache"
            } else if nlc == "logs" || nlc == "log" {
                "logs"
            } else if nlc == "user data" {
                "data"
            } else {
                continue;
            };
            push_unique(
                &mut app.paths,
                PathEntry {
                    path: e.path().to_string_lossy().into_owned(),
                    role,
                    source: "heuristic",
                    confidence: "guess",
                },
            );
        }
    }
}

/// Registry větve HKLM/HKU\…\Software\{Publisher}\{Product} (High).
/// Stejné varianty jmen jako heuristika souborů.
fn registry_branches(app: &mut AppEntry) {
    use win_sys::registry::{enum_subkeys, HKEY_LOCAL_MACHINE, HKEY_USERS};
    let Some(publisher) = app.publisher.clone().filter(|p| !p.is_empty()) else {
        return;
    };
    let pubs = publisher_variants(&publisher);
    let prods = product_variants(&app.display_name, pubs.last().map(|s| s.as_str()));
    let combos: Vec<(String, String)> = pubs
        .iter()
        .flat_map(|p| prods.iter().map(move |pr| (p.clone(), pr.clone())))
        .collect();

    for (p, pr) in &combos {
        let sub = format!(r"SOFTWARE\{p}\{pr}");
        if !enum_subkeys(HKEY_LOCAL_MACHINE, &sub).is_empty() {
            push_unique(
                &mut app.paths,
                PathEntry {
                    path: format!(r"HKLM\{sub}"),
                    role: "registry",
                    source: "registry",
                    confidence: "high",
                },
            );
            break; // stačí nejlepší shoda
        }
    }
    // HKU: reálné uživatelské SIDy (S-1-5-21…), bez _Classes.
    for sid in enum_subkeys(HKEY_USERS, "") {
        if !sid.starts_with("S-1-5-21") || sid.ends_with("_Classes") {
            continue;
        }
        for (p, pr) in &combos {
            let sub = format!(r"{sid}\Software\{p}\{pr}");
            if !enum_subkeys(HKEY_USERS, &sub).is_empty() {
                push_unique(
                    &mut app.paths,
                    PathEntry {
                        path: format!(r"HKU\{sub}"),
                        role: "registry",
                        source: "registry",
                        confidence: "high",
                    },
                );
                break;
            }
        }
    }
}

/// Role cesty podle názvů segmentů (jednoduchá heuristika dle SPEC 5.2).
fn role_of(path: &str) -> &'static str {
    let p = path.to_ascii_lowercase();
    if p.contains("\\cache") || p.ends_with("cache") {
        "cache"
    } else if p.contains("\\log") {
        "logs"
    } else if p.contains("\\config") || p.contains("\\settings") {
        "config"
    } else if p.contains("\\program files") {
        "install"
    } else {
        "data"
    }
}

/// Zredukuje seznam souborů na nejkratší pokrývající adresáře (max n).
/// Tisíce MSI komponent → pár kořenů, se kterými se dá v UI pracovat.
fn collapse_dirs(paths: Vec<String>, max: usize) -> Vec<String> {
    let mut dirs: Vec<String> = paths
        .iter()
        .filter_map(|p| {
            std::path::Path::new(p)
                .parent()
                .map(|d| d.to_string_lossy().to_lowercase())
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    // Nejkratší prefixy vyhrávají; delší cesty pod nimi se zahodí.
    dirs.sort_by_key(|d| d.len());
    let mut kept: Vec<String> = Vec::new();
    for d in dirs {
        if kept.iter().any(|k| {
            d.starts_with(k.as_str()) && (d.len() == k.len() || d.as_bytes()[k.len()] == b'\\')
        }) {
            continue;
        }
        kept.push(d);
        if kept.len() >= max {
            break;
        }
    }
    kept
}

/// Přidá cestu, pokud tam (case-insensitive) ještě není.
fn push_unique(paths: &mut Vec<PathEntry>, entry: PathEntry) {
    let lc = entry.path.to_ascii_lowercase();
    if paths.iter().any(|p| p.path.to_ascii_lowercase() == lc) {
        return;
    }
    paths.push(entry);
}

/// Velikost adresáře rekurzivně (on-demand, SPEC 5.2 „lazy“).
/// Symlinky/junctiony se nenásledují (smyčky, dvojí počítání).
pub fn dir_size(path: &str) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![std::path::PathBuf::from(path)];
    let mut visited = 0u32;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > 200_000 {
            break; // pojistka proti patologickým stromům
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_keeps_covering_roots() {
        let paths = vec![
            r"C:\Program Files\App\bin\a.exe".to_string(),
            r"C:\Program Files\App\bin\b.dll".to_string(),
            r"C:\Program Files\App\data\c.dat".to_string(),
            r"C:\Other\x.txt".to_string(),
        ];
        let dirs = collapse_dirs(paths, 12);
        assert!(dirs.contains(&r"c:\other".to_string()));
        assert!(dirs.contains(&r"c:\program files\app\bin".to_string()));
        assert!(dirs.contains(&r"c:\program files\app\data".to_string()));
    }

    #[test]
    fn install_date_parses() {
        let ts = parse_install_date(Some("20240115")).unwrap();
        // 2024-01-15 UTC
        assert_eq!(ts, 1_705_276_800);
        assert!(parse_install_date(Some("garbage")).is_none());
    }

    #[test]
    fn roles_classify() {
        assert_eq!(role_of(r"C:\Users\x\AppData\Local\App\Cache"), "cache");
        assert_eq!(role_of(r"C:\Program Files\App"), "install");
        assert_eq!(role_of(r"C:\Users\x\AppData\Roaming\App"), "data");
    }
}
