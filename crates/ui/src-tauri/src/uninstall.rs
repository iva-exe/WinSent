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

use std::sync::Mutex;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Handle běžícího odinstalátoru. Ukládá se jako `isize`, protože
/// `HANDLE` drží syrový ukazatel a nejde poslat mezi vlákny.
/// Odinstalace běží vždy nejvýš jedna — proto stačí jedno místo.
static RUNNING: Mutex<Option<isize>> = Mutex::new(None);

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
        *RUNNING.lock().expect("running lock") = Some(info.hProcess.0 as isize);
    }
    Ok(std::path::Path::new(&exe)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or(exe))
}

/// Běží odinstalátor ještě?
///
/// Dva zdroje, protože ani jeden sám nestačí:
/// 1. handle spuštěného procesu — jistota, dokud ten proces žije,
/// 2. jméno binárky mezi běžícími procesy — mnoho odinstalátorů se
///    po startu znovu spustí (kvůli právům správce nebo z kopie v temp)
///    a ten původní hned skončí; bez tohohle kroku bychom ohlásili
///    „hotovo“ dřív, než se odinstalátor vůbec stihl načíst.
///
/// Seznam procesů dodává služba — jako SYSTEM vidí i procesy spuštěné
/// se zvýšenými právy, na které by UI samo nedosáhlo.
pub fn still_running(exe_name: &str) -> bool {
    if handle_alive() {
        return true;
    }
    // msiexec.exe je systémová služba, která běží skoro pořád — podle
    // jména se u ní čekat nedá, tam rozhoduje jen handle výše.
    if exe_name.eq_ignore_ascii_case("msiexec.exe") {
        return false;
    }
    match ipc::client::query_procs() {
        Ok(rows) => rows.iter().any(|p| p.name.eq_ignore_ascii_case(exe_name)),
        // Bez seznamu procesů radši netvrdíme, že skončil.
        Err(_) => false,
    }
}

/// Žije ještě proces, který jsme spustili?
fn handle_alive() -> bool {
    let guard = RUNNING.lock().expect("running lock");
    let Some(raw) = *guard else {
        return false;
    };
    let h = HANDLE(raw as *mut _);
    // SAFETY: handle vlastníme od ShellExecuteExW až po close_running().
    // Timeout 0 = jen se zeptej, nečekej.
    unsafe { WaitForSingleObject(h, 0) != WAIT_OBJECT_0 }
}

/// Zavře uložený handle (konec odinstalace, nebo start další).
pub fn close_running() {
    let mut guard = RUNNING.lock().expect("running lock");
    if let Some(raw) = guard.take() {
        // SAFETY: handle se zavírá právě jednou — `take()` ho vyjme.
        unsafe {
            let _ = CloseHandle(HANDLE(raw as *mut _));
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
