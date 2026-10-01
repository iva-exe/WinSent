//! Spuštění odinstalátoru v relaci přihlášeného uživatele (v8).
//!
//! Proč právě tady: služba běží jako SYSTEM v session 0 — izolované
//! neviditelné ploše. Odinstalátor spuštěný odtud by neměl kam vykreslit
//! dialogy, jako SYSTEM by viděl cizí `HKEY_CURRENT_USER` a dostal by
//! práva, se kterými nepočítá. UI proces naopak běží pod uživatelem
//! a v jeho relaci — přesně tam, kde odinstalátor očekává, že poběží
//! (jako by ho uživatel spustil z Ovládacích panelů).
//!
//! Tok je záměrně jednoduchý: pustit → počkat, až doběhne → projít
//! cesty aplikace a ukázat, co zbylo. Rozhodnutí *zda* se smí
//! odinstalovat padá ve validační vrstvě, tenhle modul zná jen *jak*.

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard};

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    GetProcessId, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Spuštěný odinstalátor a to, co z něj vzešlo.
///
/// Odinstalace běží vždy nejvýš jedna — proto stačí jedno místo.
struct Hlidani {
    /// Handle spuštěného procesu. Jako `isize`, protože `HANDLE` drží
    /// syrový ukazatel a nejde poslat mezi vlákny.
    handle: isize,
    /// PID a čas vzniku spuštěného procesu. Čas brání tomu, aby se za
    /// potomka považoval proces, který dostal recyklované PID.
    root: (u32, i64),
    /// Složka odinstalátoru (malými písmeny) — potomci odtud se sledují.
    slozka: String,
    /// Potomci, na které se čeká (PID + čas vzniku). Drží se napříč
    /// dotazy: originál bývá mrtvý dřív, než se UI poprvé zeptá, a jeho
    /// kopii by pak už nebylo podle čeho poznat.
    sledovani: HashSet<(u32, i64)>,
    /// Potomci, kteří k odinstalaci nepatří (prohlížeč s dotazníkem
    /// „proč odcházíte"…). Pamatují se, ať se jejich cesta nečte znovu.
    cizi: HashSet<(u32, i64)>,
}

static RUNNING: Mutex<Option<Hlidani>> = Mutex::new(None);

fn running() -> MutexGuard<'static, Option<Hlidani>> {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Chyby spouštěče.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("odinstalační příkaz nejde přečíst: {0}")]
    BadCommand(String),
    #[error("odinstalátor se nepodařilo spustit: {0}")]
    Spawn(String),
}

/// Rozdělí příkaz z registru na program a argumenty. Uninstall stringy
/// mají dvě podoby: `"C:\...\unins000.exe" /SILENT` (cesta v uvozovkách)
/// nebo `C:\Program Files\App\uninst.exe /X` (bez uvozovek — a klidně
/// s mezerami v cestě, takže dělit podle mezery nejde; hledá se `.exe`).
pub fn split_command(cmd: &str) -> Option<(String, String)> {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        let end = rest.find('"')?;
        let exe = rest[..end].trim().to_string();
        let args = rest[end + 1..].trim().to_string();
        (!exe.is_empty()).then_some((exe, args))
    } else {
        let lc = cmd.to_ascii_lowercase();
        let end = lc.find(".exe")? + 4;
        let exe = cmd[..end].trim().to_string();
        let args = cmd[end..].trim().to_string();
        (!exe.is_empty()).then_some((exe, args))
    }
}

/// Spustí odinstalátor a HNED se vrátí — čekání řeší `still_running()`,
/// ať UI mezitím může ukázat, co se děje. Vrací jméno spuštěné binárky
/// (podle něj se pozná, že odinstalátor pořád běží).
///
/// Vědomě přes `ShellExecuteExW`, ne `CreateProcess`: respektuje manifest
/// programu, takže když odinstalátor potřebuje práva správce, Windows
/// samy zobrazí výzvu UAC. Okno je normální a viditelné; dialogy
/// odklikává uživatel, ne my.
pub fn launch(command: &str) -> Result<String, Error> {
    let (exe, args) = split_command(command).ok_or_else(|| Error::BadCommand(command.into()))?;
    let wexe = HSTRING::from(exe.as_str());
    let wargs = HSTRING::from(args.as_str());
    // Pracovní adresář = složka odinstalátoru; některé očekávají, že
    // vedle sebe najdou svá data.
    let dir = std::path::Path::new(&exe)
        .parent()
        .map(|p| HSTRING::from(p.to_string_lossy().as_ref()));

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // NOCLOSEPROCESS → dostaneme handle a poznáme konec.
        // NOASYNC → volání dokončí práci dřív, než se vrátí.
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpFile: windows::core::PCWSTR(wexe.as_ptr()),
        lpParameters: if args.is_empty() {
            windows::core::PCWSTR::null()
        } else {
            windows::core::PCWSTR(wargs.as_ptr())
        },
        lpDirectory: dir
            .as_ref()
            .map(|d| windows::core::PCWSTR(d.as_ptr()))
            .unwrap_or(windows::core::PCWSTR::null()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    // SAFETY: struktura je vyplněná dle kontraktu API a řetězce žijí
    // po celou dobu volání.
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| Error::Spawn(format!("{e}")))?;
    }
    close_running();
    if !info.hProcess.is_invalid() {
        let h = info.hProcess;
        // SAFETY: platný handle z ShellExecuteExW, vlastníme ho.
        let pid = unsafe { GetProcessId(h) };
        let slozka = std::path::Path::new(&exe)
            .parent()
            .map(dlouha_mala)
            .unwrap_or_default();
        *running() = Some(Hlidani {
            handle: h.0 as isize,
            root: (pid, cas_vzniku(h).unwrap_or(0)),
            slozka,
            sledovani: HashSet::new(),
            cizi: HashSet::new(),
        });
    }
    Ok(std::path::Path::new(&exe)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or(exe))
}

/// Běží odinstalátor ještě?
///
/// Tři zdroje, protože ani jeden sám nestačí:
/// 1. handle spuštěného procesu — jistota, dokud ten proces žije,
/// 2. jeho potomci — NSIS se zkopíruje do `%TEMP%` jako `Un_A.exe`
///    (Inno jako `_iu*.tmp`), kopii spustí a originál hned skončí.
///    Kopie má JINÉ jméno, takže podle jména ji nenajdeme; dřív se
///    proto ohlásilo „hotovo“, zatímco dialog odinstalátoru ještě
///    svítil, a audit si natrvalo zapsal neúspěch,
/// 3. jméno binárky mezi běžícími procesy — odinstalátor se může
///    znovu spustit přes jiného rodiče (kvůli právům správce).
///
/// Seznam procesů dodává služba — jako SYSTEM vidí i procesy spuštěné
/// se zvýšenými právy, na které by UI samo nedosáhlo.
pub fn still_running(exe_name: &str) -> bool {
    if let Some(w) = running().as_ref() {
        let h = HANDLE(w.handle as *mut _);
        // SAFETY: handle vlastníme od ShellExecuteExW až po close_running().
        // Timeout 0 = jen se zeptej, nečekej.
        if unsafe { WaitForSingleObject(h, 0) } != WAIT_OBJECT_0 {
            return true;
        }
    }
    // msiexec.exe je systémová služba, která běží skoro pořád — podle
    // jména ani potomků se u ní čekat nedá, tam rozhoduje jen handle.
    if exe_name.eq_ignore_ascii_case("msiexec.exe") {
        return false;
    }
    let rows = match crate::roura::volej(|| ipc::client::query_procs()) {
        Ok(rows) => rows,
        // Bez seznamu procesů radši netvrdíme, že skončil. Pojistkou
        // proti věčnému čekání je ruční „Hotovo“ v UI.
        Err(_) => return true,
    };
    // Zámek až po dotazu na službu: pipe nemá timeout a `close_running`
    // z dokončení odinstalace by jinak čekal na její odpověď.
    if let Some(w) = running().as_mut() {
        let trojice: Vec<(u32, u32, i64)> = rows
            .iter()
            .map(|p| (p.pid, p.parent_pid, p.create_time))
            .collect();
        let temp = dlouha_mala(&std::env::temp_dir());
        let slozka = w.slozka.clone();
        rozsir_potomky(w.root, &trojice, &mut w.sledovani, &mut w.cizi, |pid| {
            patri_k_odinstalaci(pid, &temp, &slozka)
        });
        if trojice
            .iter()
            .any(|&(pid, _, ct)| w.sledovani.contains(&(pid, ct)))
        {
            return true;
        }
    }
    rows.iter().any(|p| p.name.eq_ignore_ascii_case(exe_name))
}

/// Doplní do `sledovani` potomky spuštěného procesu (i potomky potomků).
///
/// `rows` jsou trojice (PID, PID rodiče, čas vzniku). Potomek se uzná,
/// jen když vznikl až po rodiči — PID se recyklují a proces se stejným
/// PID, jaké měl dávno mrtvý rodič, by jinak prošel jako jeho dítě.
/// Ze stejného důvodu se neuzná dítě, jehož rodičovské PID mezitím
/// dostal jiný, novější proces.
///
/// `patri` rozhoduje, jestli potomek k odinstalaci patří; kdo nepatří,
/// skončí v `cizi` a jeho potomci se nesledují.
fn rozsir_potomky(
    root: (u32, i64),
    rows: &[(u32, u32, i64)],
    sledovani: &mut HashSet<(u32, i64)>,
    cizi: &mut HashSet<(u32, i64)>,
    patri: impl Fn(u32) -> bool,
) {
    // Kopie může spustit další kopii, proto do ustálení.
    loop {
        let mut zmena = false;
        for &(pid, ppid, ct) in rows {
            let klic = (pid, ct);
            if klic == root || sledovani.contains(&klic) || cizi.contains(&klic) {
                continue;
            }
            let rodic = std::iter::once(&root)
                .chain(sledovani.iter())
                .find(|&&(rp, rct)| rp == ppid && ct >= rct)
                .copied();
            let Some((_, rct)) = rodic else {
                continue;
            };
            // Rodičovské PID už patří jinému procesu, který vznikl po
            // našem rodiči a před tímhle dítětem — dítě je jeho.
            let podvrh = rows
                .iter()
                .any(|&(p, _, c)| p == ppid && c > rct && c <= ct);
            if podvrh {
                continue;
            }
            if patri(pid) {
                sledovani.insert(klic);
                zmena = true;
            } else {
                cizi.insert(klic);
            }
        }
        if !zmena {
            break;
        }
    }
}

/// Patří potomek k odinstalaci?
///
/// Odinstalátory se kopírují do `%TEMP%` nebo pouštějí pomocníky ze své
/// složky. Proces odjinud — typicky prohlížeč s dotazníkem „proč
/// odcházíte“ — k ní nepatří a čekat na jeho zavření by znamenalo
/// nikdy nedokončit. Když cestu přečíst nejde (zvýšená práva), raději
/// se čeká: ruční „Hotovo“ v UI je pojistka.
fn patri_k_odinstalaci(pid: u32, temp: &str, slozka: &str) -> bool {
    let Some(cesta) = cesta_procesu(pid) else {
        return true;
    };
    pod_slozkou(std::path::Path::new(&cesta), temp, slozka)
}

/// Leží binárka pod `temp` nebo pod `slozka` (obě z [`dlouha_mala`])?
///
/// Cesta procesu se převádí stejně jako `TEMP`. NSIS i Inno skládají
/// cestu kopie z `GetTempPath`, tedy z `TEMP` tak, jak je zapsaný —
/// často krátce (`C:\Users\NOVAKJ~1\…`) — a QueryFullProcessImageNameW
/// pak vrací tentýž krátký tvar (ověřeno: proces spuštěný přes 8.3
/// cestu ji tak i hlásí). Proti dlouhému `TEMP` by se kopie nikdy
/// neshodla, skončila by mezi cizími a konec odinstalace by se ohlásil
/// předčasně. Binárka běží, takže převod na dlouhý tvar projde.
fn pod_slozkou(cesta: &std::path::Path, temp: &str, slozka: &str) -> bool {
    let cesta = dlouha_mala(cesta);
    (!temp.is_empty() && cesta.starts_with(temp))
        || (!slozka.is_empty() && cesta.starts_with(slozka))
}

/// Plná cesta k binárce procesu, nebo `None`, když ji přečíst nejde.
fn cesta_procesu(pid: u32) -> Option<String> {
    // SAFETY: handle z OpenProcess se vždy zavře; buffer má hlášenou délku.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Cesta v dlouhém tvaru a malými písmeny, pro porovnání předpon.
///
/// `TEMP` bývá zapsaný krátkými jmény (`C:\Users\NOVAKJ~1\…`), kdežto
/// cesta procesu přijde dlouhá — bez převodu by se nikdy neshodly.
fn dlouha_mala(p: &std::path::Path) -> String {
    // Holé jméno bez složky (`MsiExec.exe`) nemá s čím se porovnávat.
    if p.as_os_str().is_empty() {
        return String::new();
    }
    let s = std::fs::canonicalize(p)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string_lossy().into_owned());
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.trim_end_matches(char::from(92u8)).to_lowercase() + "\\"
}

/// Čas vzniku procesu (FILETIME jako i64, stejně jako v `ProcRow`).
fn cas_vzniku(h: HANDLE) -> Option<i64> {
    let (mut c, mut e, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: platný handle, výstupy jsou lokální struktury.
    unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).ok()? };
    Some(((c.dwHighDateTime as i64) << 32) | c.dwLowDateTime as i64)
}

/// Zavře uložený handle (konec odinstalace, nebo start další).
pub fn close_running() {
    if let Some(w) = running().take() {
        // SAFETY: handle se zavírá právě jednou — `take()` ho vyjme.
        unsafe {
            let _ = CloseHandle(HANDLE(w.handle as *mut _));
        }
    }
}

/// Projde cesty aplikace a vrátí ty, které na disku pořád jsou.
/// Volá se PO odinstalaci, se seznamem zachyceným PŘED ní — inventář
/// mezitím odinstalovanou aplikaci ze své databáze odstraní.
/// Kontroluje se z UI procesu, tedy pod uživatelem: vidíme přesně to,
/// co uvidí uživatel v Průzkumníku.
pub fn remaining(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        // Registry větve se takhle kontrolovat nedají.
        .filter(|p| !p.starts_with("HK") && std::path::Path::new(p).exists())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Obě podoby uninstall stringu se rozdělí správně.
    #[test]
    fn splits_both_command_shapes() {
        let (exe, args) = split_command("\"C:\\App\\unins000.exe\" /SILENT").expect("quoted");
        assert_eq!(exe, "C:\\App\\unins000.exe");
        assert_eq!(args, "/SILENT");

        let (exe, args) = split_command("MsiExec.exe /X{1234-5678}").expect("bare");
        assert_eq!(exe, "MsiExec.exe");
        assert_eq!(args, "/X{1234-5678}");

        // Příkaz bez .exe a bez uvozovek nerozdělujeme — radši nic.
        assert!(split_command("neco divneho").is_none());
    }

    // Skutečný případ z registru: cesta S MEZERAMI, ale BEZ uvozovek.
    // Dělit podle mezery by uřízlo „C:\Program" — proto hledáme .exe.
    #[test]
    fn splits_unquoted_path_with_spaces() {
        let (exe, args) = split_command(
            r"C:\Program Files (x86)\Overwolf\OWUninstaller.exe --uninstall-app=pibhbkkg",
        )
        .expect("bare with spaces");
        assert_eq!(exe, r"C:\Program Files (x86)\Overwolf\OWUninstaller.exe");
        assert_eq!(args, "--uninstall-app=pibhbkkg");
    }

    // Příkaz bez argumentů dá prázdný druhý díl, ne mezeru.
    #[test]
    fn splits_command_without_args() {
        let (exe, args) = split_command("\"C:\\App\\uninstall.exe\"").expect("quoted");
        assert_eq!(exe, "C:\\App\\uninstall.exe");
        assert!(args.is_empty());
    }

    // NSIS: originál (100) spustí kopii v TEMPu (200) a skončí; kopie
    // spustí další pomocníka (300). Na oba se čeká, i když originál
    // ze seznamu už zmizel.
    #[test]
    fn potomci_se_sleduji_i_po_smrti_originalu() {
        let root = (100, 1_000);
        let rows = [(200, 100, 1_010), (300, 200, 1_020), (400, 4, 500)];
        let (mut s, mut c) = (HashSet::new(), HashSet::new());
        rozsir_potomky(root, &rows, &mut s, &mut c, |_| true);
        assert!(s.contains(&(200, 1_010)));
        assert!(s.contains(&(300, 1_020)));
        assert!(!s.contains(&(400, 500)));
    }

    // Recyklované PID: proces s PID rodiče, ale starší než rodič, není
    // potomek. A dítě procesu, který PID rodiče dostal až později, taky ne.
    #[test]
    fn recyklovane_pid_neprojde() {
        let root = (100, 1_000);
        // Starší proces, jehož rodičovské PID je náhodou 100.
        let rows = [(500, 100, 900)];
        let (mut s, mut c) = (HashSet::new(), HashSet::new());
        rozsir_potomky(root, &rows, &mut s, &mut c, |_| true);
        assert!(s.is_empty());

        // Potomek 200 skončil, PID 200 dostal cizí proces (1_050)
        // a ten spustil 600. 600 k odinstalaci nepatří.
        let mut s = HashSet::from([(200, 1_010)]);
        let rows = [(200, 7, 1_050), (600, 200, 1_060)];
        rozsir_potomky(root, &rows, &mut s, &mut c, |_| true);
        assert!(!s.contains(&(600, 1_060)));
    }

    // Prohlížeč s dotazníkem po odinstalaci k ní nepatří — ani on, ani
    // jeho potomci; jinak by se nikdy nedokončila.
    #[test]
    fn cizi_potomek_se_nesleduje() {
        let root = (100, 1_000);
        let rows = [(200, 100, 1_010), (210, 200, 1_011)];
        let (mut s, mut c) = (HashSet::new(), HashSet::new());
        rozsir_potomky(root, &rows, &mut s, &mut c, |pid| pid != 200);
        assert!(s.is_empty());
        assert!(c.contains(&(200, 1_010)));
    }

    // Kopie odinstalátoru spuštěná přes krátkou (8.3) cestu do TEMPu
    // k odinstalaci patří; cizí binárka ne. Dřív se krátká cesta
    // procesu porovnávala s dlouhým TEMPem a kopie vypadla.
    #[test]
    fn kratka_cesta_kopie_patri_do_tempu() {
        use std::os::windows::process::CommandExt;
        let temp = dlouha_mala(&std::env::temp_dir());
        let dir = std::env::temp_dir().join(format!("winsent dlouhe jmeno {}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("složka");
        let exe = dir.join("Un_A dlouhy odinstalator.exe");
        std::fs::write(&exe, b"MZ").expect("zapsat");

        // Krátký tvar si řekne cmd (8.3 jména můžou být na svazku vypnutá,
        // pak vrátí dlouhý a test platí stejně).
        let out = std::process::Command::new("cmd")
            .raw_arg(format!(
                "/c for %I in (\"{}\") do @echo %~sI",
                exe.display()
            ))
            .output()
            .expect("cmd");
        let kratka = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert!(kratka.to_lowercase().ends_with(".exe"), "{kratka}");
        assert!(pod_slozkou(std::path::Path::new(&kratka), &temp, ""));
        // Jiná velikost písmen a „..“ v cestě také.
        let oklika = format!(
            "{}\\..\\{}",
            dir.display(),
            dir.file_name().unwrap().to_string_lossy()
        );
        let oklika =
            std::path::Path::new(&oklika.to_uppercase()).join("Un_A dlouhy odinstalator.exe");
        assert!(pod_slozkou(&oklika, &temp, ""));
        // Mimo TEMP i mimo složku aplikace — prohlížeč s dotazníkem.
        assert!(!pod_slozkou(
            std::path::Path::new(r"C:\Windows\System32\cmd.exe"),
            &temp,
            &dlouha_mala(&dir)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Zbytky: existující cesty ano, registry větve a smazané ne.
    #[test]
    fn remaining_keeps_only_existing_paths() {
        let tmp = std::env::temp_dir().join("winsent-uninstall-test.tmp");
        std::fs::write(&tmp, b"x").expect("zapsat");
        let paths = vec![
            tmp.to_string_lossy().into_owned(),
            r"C:\neexistuje-xyz\a.txt".into(),
            r"HKLM\SOFTWARE\Neco".into(),
        ];
        let left = remaining(&paths);
        assert_eq!(left.len(), 1);
        assert!(left[0].contains("winsent-uninstall-test"));
        let _ = std::fs::remove_file(&tmp);
    }
}
