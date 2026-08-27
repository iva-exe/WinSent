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
//! Když jeden krok najde kandidáta, ale spuštění selže, pokračuje se
//! dalším: zastaralé AUMID po odinstalované aplikaci ze Storu není
//! důvod to vzdát, když zástupce v nabídce Start funguje.
//!
//! Jména se porovnávají PO SLOVECH. Skládat písmena natěsno je krátká
//! cesta k nesmyslům: „git" pak sedí do „diGITální certifikát" i do
//! „loGITech G HUB". Naměřeno na živém seznamu — bez pravidla o slovech
//! se klik na „Git" trefil do „Git FAQs" (a otevřel webovou stránku),
//! „R.E.P.O." do „Report a Problem with Unity" a „SteamVR" do „Steam".

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Najde a spustí aplikaci. Vrací, co se spustilo (pro hlášku v UI).
pub fn launch(identity_key: &str, display_name: &str) -> Result<String, String> {
    let mut chyba: Option<String> = None;
    if let Some(aumid) = najdi_v_appsfolder(display_name) {
        match spust(&format!("shell:AppsFolder\\{aumid}"), None) {
            Ok(()) => return Ok(display_name.to_string()),
            Err(e) => chyba = Some(e),
        }
    }
    if let Some(lnk) = najdi_zastupce(display_name) {
        match spust(&lnk.to_string_lossy(), lnk.parent()) {
            Ok(()) => return Ok(lnk.display().to_string()),
            Err(e) => chyba = Some(e),
        }
    }
    if let Some(exe) = najdi_exe(identity_key, display_name) {
        match spust(&exe.to_string_lossy(), exe.parent()) {
            Ok(()) => return Ok(exe.display().to_string()),
            Err(e) => chyba = Some(e),
        }
    }
    Err(chyba.unwrap_or_else(|| {
        format!("{display_name} neumím spustit — nenašel jsem zástupce ani spustitelný soubor")
    }))
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

// ── Složka „Aplikace" ──────────────────────────────────────────────

type Polozky = Vec<(String, String)>;

/// Naposledy načtená složka „Aplikace" a kdy.
///
/// Enumerace stojí přes půl sekundy (naměřeno: 260 položek ≈ 530 ms)
/// a bez cache by se dělala při každém kliknutí. Platnost je krátká,
/// aby se čerstvě nainstalovaná aplikace neztratila na dlouho.
static APPSFOLDER: std::sync::OnceLock<std::sync::Mutex<Option<(Instant, Polozky)>>> =
    std::sync::OnceLock::new();
const PLATNOST_CACHE: Duration = Duration::from_secs(60);

/// AUMID položky ze složky „Aplikace", jejíž jméno odpovídá aplikaci.
fn najdi_v_appsfolder(display_name: &str) -> Option<String> {
    let hledane = slova(display_name);
    if hledane.is_empty() {
        return None;
    }
    let polozky = appsfolder();
    let mut nejlepsi: Option<(usize, &str, &str)> = None;
    for (jmeno, aumid) in &polozky {
        // Odkaz na webovou stránku není program. Instalátory jich do
        // nabídky Start sypou spousty („Git FAQs", „Nápověda k…")
        // a spustit místo aplikace prohlížeč je horší než nespustit nic.
        if aumid.contains("://") {
            continue;
        }
        let Some(skore) = skore_jmena(jmeno, &hledane) else {
            continue;
        };
        // Kratší jméno vyhrává — „Git CMD" před „Git FAQs (Frequently
        // Asked Questions)". Při shodné délce rozhoduje abeceda, aby
        // výsledek nezávisel na pořadí enumerace: to se mezi spuštěními
        // liší a stejný klik by pak spouštěl pokaždé něco jiného.
        let lepsi = match nejlepsi {
            None => true,
            Some((s, j, _)) => (skore, jmeno.len(), jmeno.as_str()) < (s, j.len(), j),
        };
        if lepsi {
            nejlepsi = Some((skore, jmeno, aumid));
        }
    }
    nejlepsi.map(|(_, _, aumid)| aumid.to_string())
}

/// Seznam (jméno po slovech, AUMID) z cache, nebo čerstvě načtený.
fn appsfolder() -> Polozky {
    let cell = APPSFOLDER.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(c) = cell.lock() {
        if let Some((kdy, seznam)) = c.as_ref() {
            if kdy.elapsed() < PLATNOST_CACHE {
                return seznam.clone();
            }
        }
    }
    let seznam = enumeruj_appsfolder();
    if let Ok(mut c) = cell.lock() {
        *c = Some((Instant::now(), seznam.clone()));
    }
    seznam
}

/// Projde `shell:AppsFolder` a vrátí (jméno po slovech, AUMID).
fn enumeruj_appsfolder() -> Polozky {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        SHCreateItemFromParsingName, BHID_EnumItems, IEnumShellItems, IShellItem,
        SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
    };

    let mut out: Polozky = Vec::new();
    // SAFETY: COM se inicializuje pro tohle vlákno a na konci zase
    // uvolní; všechna rozhraní drží Rust a pouští je Drop na konci
    // vnitřního bloku, tedy PŘED `CoUninitialize`. Řetězce od shellu
    // patří alokátoru COM a uvolňují se ručně.
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        {
            let cesta = HSTRING::from("shell:AppsFolder");
            if let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&cesta, None) {
                if let Ok(en) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) {
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
                        let jmeno = slova(&jmeno_w.to_string().unwrap_or_default());
                        CoTaskMemFree(Some(jmeno_w.0 as *const _));
                        if jmeno.is_empty() {
                            continue;
                        }
                        let Ok(id_w) = polozka.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING) else {
                            continue;
                        };
                        let id = id_w.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(id_w.0 as *const _));
                        if !id.is_empty() {
                            out.push((jmeno, id));
                        }
                    }
                }
            }
        }
        // RPC_E_CHANGED_MODE je chyba — vlákno už COM mělo v jiném
        // režimu a uvolňovat ho po sobě nesmíme.
        if hr.is_ok() {
            CoUninitialize();
        }
    }
    out
}

// ── Porovnávání jmen ───────────────────────────────────────────────

/// Jméno rozložené na slova: malá písmena, všechno ostatní je mezera.
///
/// Oproti prostému slepení znaků drží hranice slov, na kterých pak
/// stojí celé porovnávání (viz komentář u modulu).
fn slova(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut mezera = true;
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() {
            out.push(c);
            mezera = false;
        } else if !mezera {
            out.push(' ');
            mezera = true;
        }
    }
    let konec = out.trim_end().len();
    out.truncate(konec);
    out
}

/// Jak dobře jméno položky odpovídá hledané aplikaci:
///   0 = přesná shoda,
///   1 = hledané je celý začátek jména („Docker" → „Docker Desktop"),
///   2 = jméno je celý začátek hledaného („Zen Browser" → „Zen Browser
///       (x64)"; instalátory k názvu přilepují verzi a architekturu).
/// `None` = neodpovídá.
///
/// „Celý začátek" znamená po hranici slova, ne po znaku — proto se
/// „steam" netrefí do „steamvr" a „repo" do „report a problem".
fn skore_jmena(jmeno: &str, hledane: &str) -> Option<usize> {
    if jmeno.is_empty() || hledane.is_empty() {
        return None;
    }
    if jmeno == hledane {
        return Some(0);
    }
    // `starts_with` zaručuje, že délka předpony je platná hranice
    // znaku, takže indexace bajtem je v pořádku.
    if jmeno.starts_with(hledane) && jmeno.as_bytes()[hledane.len()] == b' ' {
        return Some(1);
    }
    if hledane.starts_with(jmeno) && hledane.as_bytes()[jmeno.len()] == b' ' {
        return Some(2);
    }
    None
}

// ── Zástupci a spustitelné soubory ─────────────────────────────────

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
/// Prohledává se do omezené hloubky, ať se z toho nestane skenování
/// disku. Při shodném skóre vyhrává kratší jméno — ze stejného důvodu
/// jako u složky „Aplikace".
fn najdi_zastupce(display_name: &str) -> Option<PathBuf> {
    let hledane = slova(display_name);
    if hledane.is_empty() {
        return None;
    }
    let mut nejlepsi: Option<(usize, String, PathBuf)> = None;
    for dir in start_menu_dirs() {
        projdi(&dir, 0, &mut |p| {
            if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("lnk")) {
                return;
            }
            let jmeno = slova(&p.file_stem().unwrap_or_default().to_string_lossy());
            let Some(skore) = skore_jmena(&jmeno, &hledane) else {
                return;
            };
            let lepsi = match &nejlepsi {
                None => true,
                Some((s, j, _)) => (skore, jmeno.len(), &jmeno) < (*s, j.len(), j),
            };
            if lepsi {
                nejlepsi = Some((skore, jmeno, p.to_path_buf()));
            }
        });
    }
    nejlepsi.map(|(_, _, p)| p)
}

/// Spustitelný soubor z mapy souborů aplikace.
fn najdi_exe(identity_key: &str, display_name: &str) -> Option<PathBuf> {
    let mapa = ipc::client::query_app_map(identity_key.to_string()).ok()?;
    let hledane = slova(display_name);
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
            let stem = slova(&p.file_stem().unwrap_or_default().to_string_lossy());
            // Odinstalátory a pomocné nástroje spouštět NECHCEME —
            // kliknutí na aplikaci nesmí náhodou spustit odinstalaci.
            if ["uninstall", "uninst", "unins000", "setup", "crashreport", "update"]
                .iter()
                .any(|z| stem.contains(z))
            {
                return;
            }
            // Jméno souboru bývá zkomolené („chrome" pro „Google
            // Chrome"), takže se tu pouští i shoda uvnitř řetězce —
            // ale jen v rámci JEDNÉ instalační složky té aplikace,
            // kde už si na nic jiného sáhnout nemůžeme.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slova_drzi_hranice() {
        assert_eq!(slova("Zen Browser (x64)"), "zen browser x64");
        assert_eq!(slova("osu!"), "osu");
        assert_eq!(slova("R.E.P.O."), "r e p o");
        assert_eq!(slova("  "), "");
    }

    #[test]
    fn skore_radi_od_presne_shody() {
        assert_eq!(skore_jmena("discord", "discord"), Some(0));
        // Instalátory přilepují verzi — pořád je to ta aplikace.
        assert_eq!(skore_jmena("docker desktop", "docker"), Some(1));
        // Zkrácený název zástupce proti názvu z inventáře.
        assert_eq!(skore_jmena("zen browser", "zen browser x64"), Some(2));
        assert_eq!(skore_jmena("cosi jineho", "discord"), None);
    }

    #[test]
    fn shoda_uvnitr_slova_neprojde() {
        // Přesně tyhle případy klikaly na úplně jinou aplikaci:
        // naměřeno na živém seznamu 260 položek složky „Aplikace".
        assert_eq!(skore_jmena("digitalni certifikat pro vba", "git"), None);
        assert_eq!(skore_jmena("logitech g hub", "git"), None);
        assert_eq!(skore_jmena("report a problem with unity", "r e p o"), None);
        assert_eq!(skore_jmena("steam", "steamvr"), None);
        assert_eq!(
            skore_jmena("excel", "security update for microsoft office excel 2007"),
            None
        );
    }

    #[test]
    fn kratsi_jmeno_vyhrava_pri_shodnem_skore() {
        // Bez tohohle pravidla rozhodovalo pořadí enumerace a stejný
        // klik spouštěl pokaždé něco jiného.
        let a = ("git cmd", 1usize);
        let b = ("git faqs frequently asked questions", 1usize);
        assert!((a.1, a.0.len(), a.0) < (b.1, b.0.len(), b.0));
    }
}
