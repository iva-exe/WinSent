//! Hlášení o pádech, která Windows už mají — a jejich překlad do řeči,
//! které rozumí člověk (v3+, SPEC kap. 16).
//!
//! Windows si o každém pádu vedou záznam a vědí u něj jednu věc, kterou
//! odjinud nezjistíme: **ve kterém modulu to spadlo**. To bývá jiná
//! knihovna než aplikace sama — a právě to je odpověď na otázku „kdo za
//! to může". „Photoshop spadl" je k ničemu; „Photoshop spadl v ovladači
//! grafiky" je něco, s čím se dá pracovat.
//!
//! Pravidlo, které tenhle modul drží: **nikdy netvrdit příčinu, kterou
//! neznáme.** Windows hlásí, kde to spadlo, ne proč. Když modul patří
//! aplikaci samotné, řekne se to; když je to systémová knihovna, řekne
//! se výslovně, že to obvykle neznamená vinu Windows. Kde jistota není,
//! je to v textu poznat.

/// Pád aplikace, jak ho zaznamenaly Windows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppCrash {
    pub ts: i64,
    /// Jméno spadlé aplikace (`EADesktop.exe`).
    pub app: String,
    pub app_version: String,
    /// Modul, ve kterém to spadlo — viník. Může být táž aplikace.
    pub module: String,
    pub module_version: String,
    /// Cesta k modulu; podle ní se pozná systémová knihovna i ovladač.
    pub module_path: String,
    /// Kód výjimky (`c0000005`).
    pub code: String,
    /// Kde v modulu to spadlo — do detailu, laikovi neříká nic.
    pub offset: String,
}

/// Přečte pády aplikací z protokolu Windows.
///
/// Filtruje se i podle poskytovatele, ne jen podle ID: pod ID 1000
/// v kanálu Application hlásí i WMI a samotné číslo by natahalo úplně
/// jiné události.
pub fn app_crashes(limit: usize) -> Vec<AppCrash> {
    let q = "*[System[Provider[@Name='Application Error'] and (EventID=1000)]]";
    let evs = match win_sys::evtlog::query("Application", q, limit) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "protokol pádů aplikací nelze číst");
            return Vec::new();
        }
    };
    evs.into_iter()
        .filter_map(|e| {
            // Pořadí polí je dané poskytovatelem „Application Error"
            // a je stabilní: aplikace, verze, razítko, modul, verze,
            // razítko, kód výjimky, offset, PID, čas startu, cesta
            // k aplikaci, cesta k modulu, ID hlášení.
            let g = |i: usize| e.data.get(i).cloned().unwrap_or_default();
            let app = g(0);
            if app.is_empty() {
                return None;
            }
            Some(AppCrash {
                ts: e.ts,
                app,
                app_version: g(1),
                module: g(3),
                module_version: g(4),
                code: g(6),
                offset: g(7),
                module_path: g(11),
            })
        })
        .collect()
}

/// Co znamená kód výjimky. Vrací `None`, když ho neznáme — vymyslet si
/// vysvětlení by bylo horší než přiznat, že ho nemáme.
pub fn exception_human(code: &str) -> Option<&'static str> {
    let c = code.trim_start_matches("0x").to_ascii_lowercase();
    Some(match c.as_str() {
        "c0000005" => "program sáhl do paměti, kam nesměl — nejčastější druh pádu",
        "c0000006" => "program nemohl načíst část sebe sama z disku",
        "c000001d" => "program chtěl vykonat instrukci, které procesor nerozumí",
        "c0000025" => "program se dostal do stavu, ze kterého se nešlo zotavit",
        "c0000094" => "dělení nulou",
        "c00000fd" => "programu došel prostor pro volání funkcí (zacyklil se)",
        "c0000374" => "program si rozbil vlastní správu paměti",
        "c0000409" => "program si přepsal paměť a Windows ho radši ukončily",
        "c000041d" => "chyba nastala uvnitř obsluhy jiné chyby",
        "c0000602" => "program sám ohlásil, že nemůže pokračovat",
        "80000003" => "program narazil na ladicí zarážku (bývá u nedodělaných verzí)",
        "e0434352" => "chyba v programu psaném pro .NET, kterou nikdo neošetřil",
        "e06d7363" => "chyba v programu psaném v C++, kterou nikdo neošetřil",
        _ => return None,
    })
}

/// Kdo je viník, řečeno pro člověka.
///
/// `drivers` je soupis ovladačů (jméno souboru INF nebo modulu → popis),
/// aby šlo říct „ovladač grafiky od NVIDIA" místo `nvwgf2umx.dll`.
pub fn culprit_human(c: &AppCrash, drivers: &[(String, String)]) -> String {
    let m = c.module.to_ascii_lowercase();
    let app = c.app.to_ascii_lowercase();

    if m.is_empty() {
        return "Windows nezaznamenaly, ve které části programu to spadlo.".into();
    }
    // Spadl sám v sobě — nejjednodušší a nejčastější případ.
    if m == app {
        return format!("Spadl přímo {} — chyba je v samotném programu.", c.app);
    }
    // Systémové knihovny. Tady se hodně chybuje: že pád nastal
    // v ntdll.dll neznamená, že za to můžou Windows. Ta knihovna jen
    // provádí, o co ji program požádal.
    const SYSTEM_LIBS: &[&str] = &[
        "ntdll.dll",
        "kernelbase.dll",
        "kernel32.dll",
        "combase.dll",
        "msvcrt.dll",
        "ucrtbase.dll",
        "user32.dll",
    ];
    if SYSTEM_LIBS.contains(&m.as_str()) {
        return format!(
            "Pád nastal v systémové knihovně {}. To ale obvykle neznamená chybu Windows — \
             ta knihovna jen dělá, o co ji program požádal, a špatný pokyn přišel od {}.",
            c.module, c.app
        );
    }
    // Ovladač: když jméno modulu sedí na něco ze soupisu ovladačů,
    // umíme říct od koho je a jak starý.
    let stem = m.trim_end_matches(".dll").trim_end_matches(".sys");
    if let Some((_, popis)) = drivers
        .iter()
        .find(|(name, _)| !stem.is_empty() && name.to_ascii_lowercase().contains(stem))
    {
        return format!(
            "Pád nastal v modulu {}, který patří k tomuhle ovladači: {popis}. \
             Když se to opakuje, stojí za pokus jeho aktualizace.",
            c.module
        );
    }
    format!(
        "Pád nastal v modulu {}, ne přímo v {}. Bývá to doplněk nebo knihovna, \
         kterou program používá.",
        c.module, c.app
    )
}

/// Celý příběh incidentu jednou větou plus podrobnostmi.
///
/// Vrací (shrnutí, podrobnosti). Shrnutí je to, co uvidí každý; zbytek
/// je pro toho, kdo chce vědět víc.
pub fn describe(c: &AppCrash, drivers: &[(String, String)]) -> (String, String) {
    let what = exception_human(&c.code)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Windows to označily kódem {} a víc neřekly", c.code));

    let summary = format!("{} se ukončil: {what}.", c.app);

    let mut detail = String::new();
    detail.push_str(&culprit_human(c, drivers));
    if !c.app_version.is_empty() {
        detail.push_str(&format!("\nVerze programu: {}", c.app_version));
    }
    if !c.module_version.is_empty() && c.module != c.app {
        detail.push_str(&format!("\nVerze modulu: {}", c.module_version));
    }
    if !c.module_path.is_empty() {
        detail.push_str(&format!("\nModul: {}", c.module_path));
    }
    detail.push_str(&format!("\nKód výjimky: 0x{}", c.code.trim_start_matches("0x")));
    (summary, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crash(app: &str, module: &str, code: &str) -> AppCrash {
        AppCrash {
            app: app.into(),
            module: module.into(),
            code: code.into(),
            ..Default::default()
        }
    }

    // Neznámý kód se nesmí vydávat za známý.
    #[test]
    fn unknown_exception_is_admitted() {
        assert!(exception_human("c0000005").is_some());
        assert!(exception_human("deadbeef").is_none());
        let (s, _) = describe(&crash("hra.exe", "hra.exe", "deadbeef"), &[]);
        assert!(s.contains("kódem deadbeef"), "{s}");
    }

    // Pád v sobě samém se pozná a řekne rovnou.
    #[test]
    fn crash_in_itself_names_the_app() {
        let t = culprit_human(&crash("EADesktop.exe", "EADesktop.exe", "c0000005"), &[]);
        assert!(t.contains("přímo EADesktop.exe"), "{t}");
    }

    // Systémová knihovna NENÍ automaticky viník — text to musí říct.
    #[test]
    fn system_library_is_not_blamed() {
        let t = culprit_human(&crash("hra.exe", "ntdll.dll", "c0000005"), &[]);
        assert!(t.contains("neznamená chybu Windows"), "{t}");
        assert!(t.contains("hra.exe"), "{t}");
    }

    // Když modul sedí na ovladač, řekne se od koho je.
    #[test]
    fn driver_module_is_matched_to_its_driver() {
        let drv = vec![(
            "nvwgf2umx".to_string(),
            "grafika NVIDIA, verze 551.23 z 2. 2. 2024".to_string(),
        )];
        let t = culprit_human(&crash("hra.exe", "nvwgf2umx.dll", "c0000005"), &drv);
        assert!(t.contains("NVIDIA"), "{t}");
        assert!(t.contains("551.23"), "{t}");
    }

    // Shrnutí musí být jedna srozumitelná věta.
    #[test]
    fn summary_is_one_plain_sentence() {
        let (s, d) = describe(&crash("Discord.exe", "chrome_elf.dll", "c0000005"), &[]);
        assert!(s.starts_with("Discord.exe se ukončil"), "{s}");
        assert!(s.contains("do paměti, kam nesměl"), "{s}");
        assert!(d.contains("chrome_elf.dll"), "{d}");
    }
}
