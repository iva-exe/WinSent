//! Rozhodovací kaskáda identity (SPEC kap. 4.1). Běží na background
//! vlákně — smí volat drahá API (podpis, VERSIONINFO). První shoda
//! vyhrává; krok 0 (uživatelský override) přijde s persistencí později.

use core_types::proc::Confidence;

use crate::{parent_dir, under_dir, under_system_root, Identity, Tables};

/// Identita vlastních procesů. Poznává se podle jména binárky nebo
/// instalačního adresáře služby — WebView2 renderery se poznají podle
/// toho, že leží v našem instalačním stromu.
fn own_identity(path: &str, image_name: &str) -> Option<Identity> {
    const OWN_EXES: &[&str] = &["syswatch.exe", "winsent.exe", "ui.exe"];
    let lc = path.to_ascii_lowercase();
    let name_lc = image_name.to_ascii_lowercase();
    let own_dir = lc.contains("\\programdata\\syswatch\\")
        || lc.contains("\\winsent\\target\\debug\\")
        || lc.contains("\\winsent\\target\\release\\");
    if !(OWN_EXES.contains(&name_lc.as_str()) || own_dir) {
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

    // 1b. Vlastní procesy — služba (syswatch.exe), UI (winsent.exe)
    //     i WebView2 potomci UI patří pod JEDNU aplikaci „Winsent".
    //     Jinak by se ve vývoji rozpadly na tři různé řádky podle cest.
    if let Some(id) = own_identity(path, image_name) {
        return id;
    }

    // Podpis (potřebný pro krok 2 i 4) — zjistíme jednou.
    let signer = win_sys::trust::signer_subject(std::path::Path::new(path));

    // 2. Windows OS — cesta pod %SystemRoot% a Microsoft podpis.
    //    Edge/Office jsou v Program Files (mimo SystemRoot) → sem nespadnou.
    if under_system_root(path) {
        let is_ms = signer
            .subject
            .as_deref()
            .map(|s| s.contains("Microsoft"))
            .unwrap_or(signer.valid || signer.subject.is_none());
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
