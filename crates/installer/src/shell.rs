//! Zápisy, které z instalace dělají „normální aplikaci": zástupce
//! v nabídce Start a záznam v Programech a funkcích.
//!
//! Bez záznamu v Uninstall klíči by šlo Winsent odebrat jen ručně —
//! a nástroj, který sám ukazuje zbytky po nedoinstalovaných
//! programech, si tohle dovolit nemůže.

use std::path::Path;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE,
    KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
};

const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Winsent";

/// Zapíše záznam do Programů a funkcí, aby šlo Winsent odinstalovat
/// běžnou cestou přes Nastavení Windows.
pub fn register_uninstall(
    setup_exe: &Path,
    install_dir: &Path,
    version: &str,
    size_kb: u32,
) -> Result<(), String> {
    let ui = install_dir.join("syswatch-ui.exe");
    // SAFETY: klíč se vždy zavírá; hodnoty jsou platné UTF-16 buffery.
    unsafe {
        let mut key = HKEY::default();
        let rc = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(UNINSTALL_KEY),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if rc.is_err() {
            return Err(format!("nelze zapsat do registru: {rc:?}"));
        }

        let sz = |name: &str, value: &str| {
            let w: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2);
            let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(bytes));
        };
        let dw = |name: &str, value: u32| {
            let _ = RegSetValueExW(
                key,
                &HSTRING::from(name),
                None,
                REG_DWORD,
                Some(&value.to_le_bytes()),
            );
        };

        sz("DisplayName", "Winsent");
        sz("DisplayVersion", version);
        sz("Publisher", "Winsent");
        sz("InstallLocation", &install_dir.to_string_lossy());
        sz("DisplayIcon", &ui.to_string_lossy());
        sz(
            "UninstallString",
            &format!("\"{}\" /uninstall", setup_exe.to_string_lossy()),
        );
        // Odinstalace i „změna" vedou na tentýž instalátor — ten se
        // podle parametru rozhodne, co udělat.
        sz(
            "ModifyPath",
            &format!("\"{}\"", setup_exe.to_string_lossy()),
        );
        dw("NoModify", 0);
        dw("NoRepair", 1);
        dw("EstimatedSize", size_kb);

        let _ = RegCloseKey(key);
    }
    Ok(())
}

/// Odstraní záznam z Programů a funkcí.
pub fn unregister_uninstall() {
    // SAFETY: mazání existujícího podstromu; neexistence není chyba.
    unsafe {
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, &HSTRING::from(UNINSTALL_KEY));
    }
}

/// Vytvoří zástupce v nabídce Start (pro všechny uživatele).
///
/// Zkratka se skládá přes IShellLink — stejné API, jaké používá
/// Průzkumník; žádné generování .lnk bajtů ručně.
pub fn create_shortcut(target: &Path, lnk: &Path) -> Result<(), String> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    // SAFETY: COM se inicializuje pro tohle vlákno; rozhraní se uvolní
    // s koncem platnosti proměnných.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("nelze vytvořit zástupce: {e}"))?;
        link.SetPath(&HSTRING::from(target.as_os_str()))
            .map_err(|e| format!("zástupce bez cíle: {e}"))?;
        if let Some(dir) = target.parent() {
            let _ = link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()));
        }
        let _ = link.SetDescription(&HSTRING::from("Winsent — správa a monitoring Windows"));

        let persist: IPersistFile = link
            .cast()
            .map_err(|e| format!("zástupce nelze uložit: {e}"))?;
        persist
            .Save(PCWSTR(HSTRING::from(lnk.as_os_str()).as_ptr()), true)
            .map_err(|e| format!("zástupce nelze zapsat: {e}"))?;
    }
    Ok(())
}
