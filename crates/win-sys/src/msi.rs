//! MSI inventář (SPEC kap. 5.1, 5.2): nainstalované produkty přes
//! `MsiEnumProductsW` + `MsiGetProductInfoW` a mapa souborů přes
//! komponenty — `MsiEnumComponentsW` → `MsiEnumClientsW` (vlastnící
//! produkt) → `MsiGetComponentPathW` (key path). Confidence `Exact`,
//! přesnější zdroj instalovaných cest neexistuje.
//!
//! POMALÉ (tisíce registry dotazů uvnitř MSI) — volat výhradně
//! z background skenu, nikdy v samplovacím cyklu.

use std::collections::HashMap;

use windows::core::PCWSTR;
use windows::Win32::System::ApplicationInstallationAndServicing::{
    MsiEnumClientsW, MsiEnumComponentsW, MsiEnumProductsW, MsiGetComponentPathW,
    MsiGetProductInfoW, INSTALLSTATE_LOCAL,
};

/// Jeden nainstalovaný MSI produkt.
#[derive(Debug, Clone, Default)]
pub struct MsiProduct {
    /// ProductCode `{GUID}` — párování na Uninstall klíče.
    pub code: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub install_date: Option<String>,
    pub install_location: Option<String>,
}

/// Vyjmenuje nainstalované MSI produkty s metadaty.
pub fn products() -> Vec<MsiProduct> {
    let mut out = Vec::new();
    let mut buf = [0u16; 39]; // GUID má 38 znaků + nul
    let mut index = 0u32;
    // SAFETY: buffer má pevnou velikost dle kontraktu API (39 znaků).
    unsafe {
        while MsiEnumProductsW(index, windows::core::PWSTR(buf.as_mut_ptr())) == 0 {
            index += 1;
            let code = String::from_utf16_lossy(&buf[..38]);
            out.push(MsiProduct {
                name: product_info(&code, "ProductName"),
                version: product_info(&code, "VersionString"),
                publisher: product_info(&code, "Publisher"),
                install_date: product_info(&code, "InstallDate"),
                install_location: product_info(&code, "InstallLocation"),
                code,
            });
        }
    }
    out
}

/// Jedna property produktu (dvoufázově kvůli délce).
fn product_info(code: &str, prop: &str) -> Option<String> {
    let wcode: Vec<u16> = code.encode_utf16().chain(std::iter::once(0)).collect();
    let wprop: Vec<u16> = prop.encode_utf16().chain(std::iter::once(0)).collect();
    let mut len = 0u32;
    // SAFETY: první volání zjistí délku, druhé čte do bufferu len+1.
    unsafe {
        let rc = MsiGetProductInfoW(
            PCWSTR(wcode.as_ptr()),
            PCWSTR(wprop.as_ptr()),
            Some(windows::core::PWSTR::null()),
            Some(&mut len),
        );
        // ERROR_MORE_DATA (234) = potřebujeme buffer; 0 s len==0 = prázdné.
        if rc != 234 && rc != 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let mut cap = buf.len() as u32;
        if MsiGetProductInfoW(
            PCWSTR(wcode.as_ptr()),
            PCWSTR(wprop.as_ptr()),
            Some(windows::core::PWSTR(buf.as_mut_ptr())),
            Some(&mut cap),
        ) != 0
        {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..cap as usize])
            .trim()
            .to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// Mapa souborů: ProductCode → key paths jeho komponent (jen soubory,
/// které na disku existují jako INSTALLSTATE_LOCAL).
pub fn component_paths() -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut comp = [0u16; 39];
    let mut ci = 0u32;
    // SAFETY: pevné GUID buffery; path buffer se čte dvoufázově.
    unsafe {
        while MsiEnumComponentsW(ci, windows::core::PWSTR(comp.as_mut_ptr())) == 0 {
            ci += 1;
            // Vlastníci komponenty (téměř vždy 1 produkt).
            let mut prod = [0u16; 39];
            let mut pi = 0u32;
            while MsiEnumClientsW(
                PCWSTR(comp.as_ptr()),
                pi,
                windows::core::PWSTR(prod.as_mut_ptr()),
            ) == 0
            {
                pi += 1;
                let mut buf = vec![0u16; 512];
                let mut len = buf.len() as u32;
                let state = MsiGetComponentPathW(
                    PCWSTR(prod.as_ptr()),
                    PCWSTR(comp.as_ptr()),
                    Some(windows::core::PWSTR(buf.as_mut_ptr())),
                    Some(&mut len),
                );
                if state == INSTALLSTATE_LOCAL && len > 0 {
                    let path = String::from_utf16_lossy(&buf[..len as usize]);
                    // Registrové key paths (začínají číslem: „02:\…“) přeskočit.
                    if path.len() > 2
                        && path.as_bytes()[1] == b':'
                        && path.as_bytes()[0].is_ascii_alphabetic()
                    {
                        out.entry(String::from_utf16_lossy(&prod[..38]))
                            .or_default()
                            .push(path);
                    }
                }
            }
        }
    }
    out
}
