//! Spuštění odinstalátoru v relaci přihlášeného uživatele (v8).
//!
//! Proč právě tady: služba běží jako SYSTEM v session 0 — izolované
//! neviditelné ploše. Odinstalátor spuštěný odtud by neměl kam vykreslit
//! dialogy, jako SYSTEM by viděl cizí `HKEY_CURRENT_USER` a dostal by
//! práva, se kterými nepočítá. UI proces naopak běží pod uživatelem
//! a v jeho relaci — což je přesně to prostředí, ve kterém odinstalátor
//! očekává, že poběží (jako by ho uživatel spustil z Ovládacích panelů).
//!
//! Rozhodnutí *zda* se smí odinstalovat tady nepadá: příkaz vydává až
//! služba po validaci (`AuthorizeUninstall`), tenhle modul zná jen *jak*.

use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Jak dlouho čekáme na doběhnutí odinstalátoru. Uživatel v něm klikáním
/// prochází dialogy — proto velkoryse, ale ne donekonečna.
const TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Chyby spouštěče.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("odinstalační příkaz nejde přečíst")]
    BadCommand,
    #[error("odinstalátor se nepodařilo spustit: {0}")]
    Spawn(String),
}

/// Rozdělí příkaz z registru na program a argumenty. Uninstall stringy
/// mají dvě podoby: `"C:\...\unins000.exe" /SILENT` (cesta v uvozovkách)
/// nebo `MsiExec.exe /X{GUID}` (bez uvozovek, končí na .exe).
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

/// Spustí odinstalátor a počká, až doběhne. Vrací popis konce pro audit.
///
/// Vědomě přes `ShellExecuteExW`, ne `CreateProcess`: respektuje manifest
/// programu, takže když odinstalátor potřebuje práva správce, Windows
/// samy zobrazí výzvu UAC — uživatel ji vidí a potvrdí. Okno je normální
/// a viditelné; dialogy odklikává uživatel, ne my.
pub fn run_and_wait(command: &str) -> Result<String, Error> {
    let (exe, args) = split_command(command).ok_or(Error::BadCommand)?;
    let wexe = HSTRING::from(exe.as_str());
    let wargs = HSTRING::from(args.as_str());
    // Pracovní adresář = složka odinstalátoru; některé očekávají, že
    // vedle sebe najdou svá data.
    let dir = std::path::Path::new(&exe)
        .parent()
        .map(|p| HSTRING::from(p.to_string_lossy().as_ref()));

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // NOCLOSEPROCESS → dostaneme handle a můžeme počkat na konec.
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

    // SAFETY: struktura je vyplněná dle kontraktu API a přežije volání;
    // řetězce drží HSTRING po celou dobu volání.
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| Error::Spawn(format!("{e}")))?;
    }
    let proc: HANDLE = info.hProcess;
    if proc.is_invalid() {
        // Bez handle nevíme, kdy skončil — nelžeme, že jsme čekali.
        return Ok("odinstalátor spuštěn (konec se nepodařilo sledovat)".into());
    }

    let t0 = Instant::now();
    // SAFETY: handle je platný až do CloseHandle níže.
    let detail = unsafe {
        let mut code = 0u32;
        loop {
            // Po sekundách, ať jde běh případně sledovat/ukončit.
            let w = WaitForSingleObject(proc, 1000);
            if w == WAIT_OBJECT_0 {
                let _ = GetExitCodeProcess(proc, &mut code);
                break format!("odinstalátor skončil s kódem {code}");
            }
            if t0.elapsed() > TIMEOUT {
                break "odinstalátor běží déle než 15 min — nečekáme dál".to_string();
            }
        }
    };
    // SAFETY: handle se zavírá právě jednou.
    unsafe {
        let _ = CloseHandle(proc);
    }
    Ok(detail)
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
}
