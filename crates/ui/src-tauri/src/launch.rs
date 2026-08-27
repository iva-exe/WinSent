//! Spuštění nainstalované aplikace z vyhledávání.
//!
//! Spouští UI proces, ne služba: běží v relaci uživatele, takže program
//! naskočí na jeho ploše a pod jeho účtem. Služba je v session 0, kde
//! by okno nemělo kam vykreslit (stejný důvod jako u odinstalátorů).
//!
//! Kde se hledá, co spustit — v tomhle pořadí:
//!   1. Složka „Aplikace" (`shell:AppsFolder`). Je to tentýž seznam,
//!      ze kterého bere nabídka Start a hledání ve Windows: jsou v něm
//!      klasické programy i aplikace ze Storu. Ty druhé žádné .exe ani
//!      zástupce nemají — spouští se přes svoje AUMID, takže bez tohohle
//!      kroku by kliknutí na Kalkulačku nebo Spotify neudělalo nic.
//!   2. Zástupce v nabídce Start — pro případ, že shell položku
//!      nenabídne (skrytá, nezaregistrovaná).
//!   3. Spustitelný soubor z mapy souborů aplikace (od služby).
//! Když neuspěje ani jedno, řekne se to — nic se nedomýšlí a nic
//! „podobného" se nespouští.

use std::path::{Path, PathBuf};

/// Najde a spustí aplikaci. Vrací, co se spustilo (pro hlášku v UI).
pub fn launch(identity_key: &str, display_name: &str) -> Result<String, String> {
    if let Some(aumid) = najdi_v_appsfolder(display_name) {
        spust(&format!("shell:AppsFolder\\{aumid}"), None)?;
        return Ok(display_name.to_string());
    }
    if let Some(lnk) = najdi_zastupce(display_name) {
        spust(&lnk.to_string_lossy(), lnk.parent())?;
        return Ok(lnk.display().to_string());
    }
    if let Some(exe) = najdi_exe(identity_key, display_name) {
        spust(&exe.to_string_lossy(), exe.parent())?;
        return Ok(exe.display().to_string());
    }
    Err(format!(
        "{display_name} neumím spustit — nenašel jsem zástupce ani spustitelný soubor"
    ))
}

/// Otevře cíl přes shell. Zvládne cestu i adresu typu `shell:AppsFolder\…`,
/// takže se aplikace ze Storu spouští stejnou cestou jako obyčejné .exe.
fn spust(cil: &str, dir: Option<&Path>) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let open = HSTRING::from("open");
    let file = HSTRING::from(cil);
    // Pracovní adresář = složka programu. Bez toho si část aplikací
    // nenajde vlastní soubory a spadne hned po startu. Prázdná cesta
    // je null ukazatel = „pracovní adresář neřeš".
    let d = dir.map(|p| HSTRING::from(p.as_os_str()));
    let dirp = d
        .as_ref()
        .map(|x| PCWSTR(x.as_ptr()))
        .unwrap_or_else(PCWSTR::null);
    // SAFETY: všechny řetězce žijí přes celé volání.
    let rc = unsafe { ShellExecuteW(None, &open, &file, None, dirp, SW_SHOWNORMAL) };
    if rc.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("spuštění selhalo (kód {})", rc.0 as usize))
    }
}

/// AUMID položky ze složky „Aplikace", jejíž jméno odpovídá aplikaci.
///
/// Enumerace stojí pár set milisekund, proto se dělá až při kliknutí,
/// ne dopředu při psaní: uživatel spustí jednu aplikaci, ne padesát.
fn najdi_v_appsfolder(display_name: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        SHCreateItemFromParsingName, BHID_EnumItems, IEnumShellItems, IShellItem,
        SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
    };

    let hledane = normalizuj(display_name);
    if hledane.is_empty() {
        return None;
    }

    // SAFETY: COM se inicializuje pro tohle vlákno; všechna rozhraní
    // drží Rust a uvolní je Drop. Řetězce z shellu se uvolňují ručně —
    // patří alokátoru COM, ne Rustu.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let cesta = HSTRING::from("shell:AppsFolder");
        let folder: IShellItem = SHCreateItemFromParsingName(&cesta, None).ok()?;
        let en: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems).ok()?;

        let mut nejlepsi: Option<(usize, String)> = None;
        loop {
            let mut davka: [Option<IShellItem>; 1] = [None];
            let mut nacteno = 0u32;
            if en.Next(&mut davka, Some(&mut nacteno)).is_err() || nacteno == 0 {
                break;
            }
            let Some(polozka) = davka[0].take() else { break };
            let Ok(jmeno_w) = polozka.GetDisplayName(SIGDN_NORMALDISPLAY) else {
                continue;
            };
            let jmeno = jmeno_w.to_string().unwrap_or_default();
            CoTaskMemFree(Some(jmeno_w.0 as *const _));

            let Some(skore) = skore_jmena(&normalizuj(&jmeno), &hledane) else {
                continue;
            };
            if nejlepsi.as_ref().is_some_and(|(s, _)| *s <= skore) {
                continue;
            }
            let Ok(id_w) = polozka.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING) else {
                continue;
            };
            let id = id_w.to_string().unwrap_or_default();
            CoTaskMemFree(Some(id_w.0 as *const _));
            if !id.is_empty() {
                nejlepsi = Some((skore, id));
            }
            // Přesnou shodu už nic nepřebije — dál se hledat nemusí.
            if skore == 0 {
                break;
            }
        }
        nejlepsi.map(|(_, id)| id)
    }
}

/// Jak dobře jméno odpovídá hledanému: 0 = přesně, 1 = obsahuje ho,
/// 2 = hledané obsahuje jeho (zkrácený název). `None` = neodpovídá.
///
/// Kratší než čtyři znaky se do třetí kategorie nepouští: „app" nebo
/// „go" uvnitř názvu je náhoda, ne shoda.
fn skore_jmena(jmeno: &str, hledane: &str) -> Option<usize> {
    if jmeno.is_empty() {
        return None;
    }
    if jmeno == hledane {
        Some(0)
    } else if jmeno.contains(hledane) {
        Some(1)
    } else if hledane.contains(jmeno) && jmeno.len() >= 4 {
        Some(2)
    } else {
        None
    }
}

/// Složky nabídky Start — uživatelská i společná.
fn start_menu_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(a) = std::env::var("APPDATA") {
        out.push(PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(p) = std::env::var("ProgramData") {
        out.push(PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    out
}

/// Zástupce, jehož jméno odpovídá aplikaci.
///
/// Přesná shoda vyhrává; jinak se bere ten, který jméno obsahuje —
/// instalátory k němu často přilepí verzi („GIMP 2.10"). Prohledává se
/// do omezené hloubky, ať se z toho nestane skenování disku.
fn najdi_zastupce(display_name: &str) -> Option<PathBuf> {
    let hledane = normalizuj(display_name);
    if hledane.is_empty() {
        return None;
    }
    let mut nejlepsi: Option<(usize, PathBuf)> = None;
    for dir in start_menu_dirs() {
        projdi(&dir, 0, &mut |p| {
            if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("lnk")) {
                return;
            }
            let jmeno = normalizuj(&p.file_stem().unwrap_or_default().to_string_lossy());
            let Some(skore) = skore_jmena(&jmeno, &hledane) else {
                return;
            };
            if nejlepsi.as_ref().is_none_or(|(s, _)| skore < *s) {
                nejlepsi = Some((skore, p.to_path_buf()));
            }
        });
    }
    nejlepsi.map(|(_, p)| p)
}

/// Spustitelný soubor z mapy souborů aplikace.
fn najdi_exe(identity_key: &str, display_name: &str) -> Option<PathBuf> {
    let mapa = ipc::client::query_app_map(identity_key.to_string()).ok()?;
    let hledane = normalizuj(display_name);
    let mut nejlepsi: Option<(usize, PathBuf)> = None;
    for radek in mapa.iter().filter(|r| r.role == "install") {
        let dir = PathBuf::from(&radek.path);
        if !dir.is_dir() {
            continue;
        }
        projdi(&dir, 0, &mut |p| {
            if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("exe")) {
                return;
            }
            let stem = normalizuj(&p.file_stem().unwrap_or_default().to_string_lossy());
            // Odinstalátory a pomocné nástroje spouštět NECHCEME —
            // kliknutí na aplikaci nesmí náhodou spustit odinstalaci.
            if ["uninstall", "uninst", "unins000", "setup", "crashreport", "update"]
                .iter()
                .any(|z| stem.contains(z))
            {
                return;
            }
            let skore = if stem == hledane {
                0
            } else if hledane.contains(&stem) || stem.contains(&hledane) {
                1
            } else {
                2
            };
            if nejlepsi.as_ref().is_none_or(|(s, _)| skore < *s) {
                nejlepsi = Some((skore, p.to_path_buf()));
            }
        });
    }
    // Skóre 2 znamená „nějaké .exe v té složce" — to je hádání.
    nejlepsi.filter(|(s, _)| *s < 2).map(|(_, p)| p)
}

/// Projde adresář do hloubky 2. Hlouběji už to není hledání zástupce,
/// ale skenování disku.
fn projdi(dir: &Path, hloubka: u8, f: &mut impl FnMut(&Path)) {
    if hloubka > 2 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            projdi(&p, hloubka + 1, f);
        } else {
            f(&p);
        }
    }
}

/// Jméno pro porovnání: malá písmena, jen písmena a číslice.
fn normalizuj(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skore_radi_od_presne_shody() {
        assert_eq!(skore_jmena("discord", "discord"), Some(0));
        // Instalátory přilepují verzi — pořád je to ta aplikace.
        assert_eq!(skore_jmena("gimp210", "gimp"), Some(1));
        // Zkrácený název zástupce („Zen" pro „Zen Browser").
        assert_eq!(skore_jmena("zenbrowser", "zenbrowserprohlizec"), Some(2));
        assert_eq!(skore_jmena("cosijineho", "discord"), None);
    }

    #[test]
    fn kratke_jmeno_neprojde_jako_zkracene() {
        // „osu" uvnitř jiného názvu je náhoda, ne shoda — jinak by
        // kliknutí spustilo úplně jinou aplikaci.
        assert_eq!(skore_jmena("osu", "osusomethingelse"), None);
    }

    #[test]
    fn normalizace_zahodi_vypln() {
        assert_eq!(normalizuj("Zen Browser (x64)"), "zenbrowserx64");
        assert_eq!(normalizuj("osu!"), "osu");
    }
}
