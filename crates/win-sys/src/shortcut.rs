//! Rozbalení zástupce (.lnk) na cílovou binárku přes IShellLink.
//!
//! Zásadní pro ikony: `.lnk` NENÍ PE soubor, takže z něj ikonu nejde
//! přečíst z resource — a shell v session 0 (služba) vrací generickou.
//! Proto se zástupce rozbalí na cíl a ikona se čte z cílového .exe.
//! Vyžaduje COM inicializované vlákno (viz `wic::init_com_for_thread`).

use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// Cesta, na kterou zástupce míří. None = nejde přečíst.
pub fn resolve_lnk(lnk_path: &str) -> Option<String> {
    let wide: Vec<u16> = lnk_path.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: COM objekty se uvolní přes Drop; buffer má pevnou velikost.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let path = String::from_utf16_lossy(&buf[..end]);
        (!path.is_empty()).then_some(path)
    }
}

/// Ikona, kterou zástupce deklaruje (`IconLocation`) — některé
/// zástupce míří na .ico nebo na jinou binárku než na cíl.
pub fn lnk_icon_location(lnk_path: &str) -> Option<(String, i32)> {
    let wide: Vec<u16> = lnk_path.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: viz výše.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        let mut index = 0i32;
        link.GetIconLocation(&mut buf, &mut index).ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let path = String::from_utf16_lossy(&buf[..end]);
        (!path.is_empty()).then_some((path, index))
    }
}

/// Cesta k .exe z registru `App Paths` (aplikace ji tam registrují,
/// aby šly spustit jménem — spolehlivý zdroj i pro neběžící appky).
pub fn app_path(exe_name: &str) -> Option<String> {
    use crate::registry::{read_string, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let sub = format!(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{}",
        exe_name
    );
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        if let Some(p) = read_string(root, &sub, "") {
            let p = p.trim().trim_matches('"').to_string();
            if std::path::Path::new(&p).is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Vyjmenuje registrované App Paths (jméno exe → plná cesta).
pub fn all_app_paths() -> Vec<(String, String)> {
    use crate::registry::{enum_subkeys, read_string, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    const BASE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
    let mut out = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for sub in enum_subkeys(root, BASE) {
            let key = format!("{BASE}\\{sub}");
            if let Some(p) = read_string(root, &key, "") {
                let p = p.trim().trim_matches('"').to_string();
                if std::path::Path::new(&p).is_file() {
                    out.push((sub.to_lowercase(), p));
                }
            }
        }
    }
    out
}
