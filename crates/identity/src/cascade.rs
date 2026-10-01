//! Rozhodovací kaskáda identity (SPEC kap. 4.1). Běží na background
//! vlákně — smí volat drahá API (podpis, VERSIONINFO). První shoda
//! vyhrává; krok 0 (uživatelský override) přijde s persistencí později.

use core_types::proc::Confidence;

use crate::{parent_dir, under_dir, under_system_root, Identity, Tables};

/// Adresář, ze kterého běží služba (malými písmeny) — v instalaci
/// `%ProgramFiles%\Winsent`, ve vývoji `<workspace>\target\debug`.
/// UI se staví i instaluje do téhož adresáře.
fn own_dir() -> Option<&'static str> {
    static DIR: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        Some(exe.parent()?.to_str()?.to_ascii_lowercase())
    })
    .as_deref()
}

/// Identita vlastních procesů: služba a UI ležící přímo v adresáři,
/// ze kterého běží služba.
///
/// Dřív stačilo jméno (syswatch.exe, winsent.exe, ui.exe kdekoli na
/// disku) nebo cesta obsahující `\programdata\syswatch\` — tam ale smí
/// zakládat soubory každý uživatel. Libovolná binárka se tak v Procesech
/// schovala pod řádek monitoru s vydavatelem Winsent a přesnou
/// identitou. A nainstalované UI (syswatch-ui.exe) pravidlo naopak
/// míjelo. Adresář služby je v instalaci chráněný (Program Files).
fn own_identity(path: &str) -> Option<Identity> {
    own_identity_in(path, own_dir()?)
}

fn own_identity_in(path: &str, own_dir: &str) -> Option<Identity> {
    const OWN_EXES: &[&str] = &["syswatch.exe", "syswatch-ui.exe"];
    let lc = path.to_ascii_lowercase();
    let name_lc = lc.rsplit('\\').next().unwrap_or_default();
    if parent_dir(&lc) != own_dir.trim_end_matches('\\') || !OWN_EXES.contains(&name_lc) {
        return None;
    }
    Some(Identity {
        identity_key: "app:winsent".into(),
        app_name: "Winsent".into(),
        publisher: Some("Winsent".into()),
        confidence: Confidence::Exact,
    })
}

/// Vyhodnotí kaskádu pro jeden proces (běží na background vlákně).
pub fn resolve(pid: u32, image_name: &str, path: Option<&str>, tables: &Tables) -> Identity {
    // 1. MSIX/AppX — PackageFamilyName.
    if let Some(family) = win_sys::procinfo::package_family(pid) {
        return Identity {
            app_name: msix_display(&family),
            identity_key: format!("msix:{family}"),
            publisher: None,
            confidence: Confidence::Exact,
        };
    }

    let Some(path) = path else {
        // Bez cesty (chráněný proces) — jen provisional dle jména.
        return Identity {
            identity_key: format!("name:{}", image_name.to_ascii_lowercase()),
            app_name: image_name.trim_end_matches(".exe").to_string(),
            publisher: None,
            confidence: Confidence::Guess,
        };
    };

    // 1b. Vlastní procesy — služba (syswatch.exe) a UI (syswatch-ui.exe)
    //     patří pod JEDNU aplikaci „Winsent". WebView2 renderery UI sem
    //     přidá až reparent_hosts v collector-proc podle rodiče.
    if let Some(id) = own_identity(path) {
        return id;
    }

    // Podpis (potřebný pro krok 2 i 4) — zjistíme jednou.
    let signer = win_sys::trust::signer_subject(std::path::Path::new(path));

    // 2. Windows OS — cesta pod %SystemRoot% a PLATNÝ podpis Microsoftu,
    //    embedded nebo katalogový. Edge/Office jsou v Program Files
    //    (mimo SystemRoot) → sem nespadnou.
    //
    //    Dřív stačilo „bez embedded podpisu", protože systémové soubory
    //    jsou podepsané jen katalogem. Jenže to splnila i libovolná
    //    nepodepsaná binárka v C:\Windows\Temp nebo C:\Windows\Tasks
    //    (zapisovatelné pro běžného uživatele) a u embedded podpisu se
    //    platnost nekontrolovala vůbec — stačil vlastní certifikát
    //    s CN „Microsoft Corporation". Katalog se ověřuje jen u souborů
    //    bez embedded podpisu, ať se neplatí dvakrát.
    if under_system_root(path) {
        let is_ms = match signer.subject.as_deref() {
            Some(s) => signer.valid && is_microsoft_signer(s),
            None => win_sys::trust::catalog_signer(std::path::Path::new(path))
                .is_some_and(|s| is_microsoft_signer(&s)),
        };
        if is_ms {
            return Identity {
                identity_key: "os:windows".into(),
                app_name: "Windows".into(),
                publisher: Some("Microsoft Windows".into()),
                confidence: Confidence::Exact,
            };
        }
    }

    // 3. Uninstall — nejdelší InstallLocation, který je prefixem cesty.
    // Shoda musí padnout na hranici komponenty cesty, ne po znacích:
    // „…\zen browser" by jinak sedlo i na „…\zen browser nightly\zen.exe".
    //
    // Sběrný adresář (pod kterým leží instalace jiné aplikace) platí jen
    // pro binárky PŘÍMO v něm. `D:\hry` je bydliště Minecraft Launcheru
    // a zároveň místo, kam si uživatel dává hry: `MinecraftLauncher.exe`
    // se tam pozná, ale `D:\hry\Star Rail Games\StarRail.exe` už ne —
    // ten se dořeší podpisem, což je pravdivější než cizí jméno.
    let path_lc = path.to_ascii_lowercase();
    let dir_lc = parent_dir(&path_lc);
    if let Some(e) = tables.uninstall.iter().find(|e| {
        under_dir(&path_lc, &e.loc) && (!e.collection || dir_lc == e.loc)
    }) {
        let name = &e.name;
        return Identity {
            identity_key: format!("app:{}", name.to_ascii_lowercase()),
            app_name: name.clone(),
            publisher: signer.subject.clone(),
            confidence: Confidence::Exact,
        };
    }

    // 4. Podpis — subject CN + ProductName z VERSIONINFO.
    if let Some(subject) = signer.subject.clone() {
        let ver = win_sys::verinfo::version_strings(path);
        let app_name = ver
            .product_name
            .clone()
            .unwrap_or_else(|| clean_subject(&subject));
        return Identity {
            identity_key: format!("sig:{}", subject.to_ascii_lowercase()),
            app_name,
            publisher: Some(subject),
            confidence: Confidence::Exact,
        };
    }

    // 5. Fallback — adresář binárky (nespolehlivé, confidence guess).
    let dir = parent_dir(path);
    let app_name = std::path::Path::new(&dir)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(image_name)
        .to_string();
    Identity {
        identity_key: format!("path:{}", dir.to_ascii_lowercase()),
        app_name,
        publisher: None,
        confidence: Confidence::Guess,
    }
}

/// Podepisuje tímhle jménem Microsoft soubory Windows? Celé jméno, ne
/// podřetězec: „Microsoft Windows Hardware Compatibility Publisher"
/// podepisuje katalogy ovladačů TŘETÍCH stran (WHQL), takže by z nich
/// udělal Windows, a podřetězec by pustil i „Not Microsoft s.r.o.".
fn is_microsoft_signer(subject: &str) -> bool {
    matches!(
        subject.trim(),
        "Microsoft Windows" | "Microsoft Windows Publisher" | "Microsoft Corporation"
    )
}

/// Zpřehlední MSIX PackageFamilyName na čitelné jméno (část před `_`).
fn msix_display(family: &str) -> String {
    family.split('_').next().unwrap_or(family).to_string()
}

/// Odstraní právní přípony ze subject CN pro hezčí app_name.
fn clean_subject(subject: &str) -> String {
    subject
        .trim_end_matches(", Inc.")
        .trim_end_matches(" Inc.")
        .trim_end_matches(", LLC")
        .trim_end_matches(" LLC")
        .trim_end_matches(" Corporation")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vlastní identita jen pro naše binárky přímo v adresáři služby —
    // ne podle jména kdekoli na disku, ne podle ProgramData.
    #[test]
    fn own_identity_needs_our_directory() {
        let dir = r"c:\program files\winsent";
        assert!(own_identity_in(r"C:\Program Files\Winsent\syswatch.exe", dir).is_some());
        assert!(own_identity_in(r"C:\Program Files\Winsent\syswatch-ui.exe", dir).is_some());
        for cizi in [
            r"C:\Users\x\Downloads\syswatch.exe",
            r"C:\Temp\ui.exe",
            r"C:\ProgramData\syswatch\x\evil.exe",
            r"C:\Program Files\Winsent\jiny.exe",
            r"C:\Program Files\Winsent\sub\syswatch.exe",
            r"C:\Program Files\Winsent2\syswatch.exe",
        ] {
            assert!(own_identity_in(cizi, dir).is_none(), "{cizi}");
        }
    }

    #[test]
    fn microsoft_signer_is_exact_name() {
        assert!(is_microsoft_signer("Microsoft Windows"));
        assert!(is_microsoft_signer("Microsoft Corporation"));
        assert!(!is_microsoft_signer(
            "Microsoft Windows Hardware Compatibility Publisher"
        ));
        assert!(!is_microsoft_signer("Not Microsoft Corporation s.r.o."));
    }
}
