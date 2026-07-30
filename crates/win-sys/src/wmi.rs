//! Minimální WMI klient (IWbemLocator → IWbemServices → ExecQuery).
//!
//! WMI je pomalé a umí se zaseknout, proto platí dvě pravidla ze SPEC
//! kap. 15.1: **jednorázově, nikdy v sekundovém cyklu**, a volající si
//! výsledek cachuje. Používá se jen tam, kde jiná cesta není — teploty
//! z ACPI a z LibreHardwareMonitoru, které Win32 API nenabízí.
//!
//! Vrací prosté řetězce a čísla; složitější typy tenhle modul neumí
//! a ani nemá — kdo potřebuje víc, ať si napíše vlastní volání.

use std::collections::HashMap;

use windows::core::{BSTR, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeSecurity, CLSCTX_INPROC_SERVER, EOAC_NONE,
    RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{VariantChangeType, VARIANT, VT_BSTR, VT_R8};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

/// Jeden řádek výsledku: název vlastnosti → hodnota jako text.
/// Čísla se drží textem záměrně — volající ví, co čeká, a parsuje si
/// to sám; univerzální typ pro VARIANT by tady byl jen na obtíž.
pub type Row = HashMap<String, String>;

/// Spustí WQL dotaz v daném jmenném prostoru (např. `root\WMI`).
///
/// Vrací prázdný seznam, když jmenný prostor neexistuje — to je běžný,
/// očekávaný stav (LibreHardwareMonitor většina lidí nemá), ne chyba.
/// COM musí být na vlákně inicializované (`wic::init_com_for_thread`).
pub fn query(namespace: &str, wql: &str, props: &[&str]) -> Vec<Row> {
    // SAFETY: COM objekty se drží jen po dobu volání; řetězce jako BSTR
    // žijí déle než volání, které je používá.
    unsafe {
        let Ok(locator) =
            CoCreateInstance::<_, IWbemLocator>(&WbemLocator, None, CLSCTX_INPROC_SERVER)
        else {
            return Vec::new();
        };
        // Bez nastavené bezpečnosti WMI odmítne proxy; když ji nastavil
        // někdo jiný dřív (RPC_E_TOO_LATE), je to v pořádku.
        let _ = CoInitializeSecurity(
            None,
            -1,
            None,
            None,
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
            None,
        );
        let Ok(svc) = locator.ConnectServer(
            &BSTR::from(namespace),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            0,
            &BSTR::new(),
            None,
        ) else {
            return Vec::new();
        };
        let Ok(en) = svc.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from(wql),
            // FORWARD_ONLY + RETURN_IMMEDIATELY = semi-synchronní režim,
            // kterým se WMI dotazy dělají, aby nedržely paměť.
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        ) else {
            return Vec::new();
        };

        let mut rows = Vec::new();
        loop {
            let mut obj: [Option<IWbemClassObject>; 1] = [None];
            let mut got = 0u32;
            if en.Next(WBEM_INFINITE, &mut obj, &mut got).is_err() || got == 0 {
                break;
            }
            let Some(obj) = obj[0].take() else { break };
            let mut row = Row::new();
            for p in props {
                if let Some(v) = get_prop(&obj, p) {
                    row.insert((*p).to_string(), v);
                }
            }
            if !row.is_empty() {
                rows.push(row);
            }
        }
        rows
    }
}

/// Vlastnost jako text. Čísla projdou přes VT_R8, ať se nemusí řešit
/// každý celočíselný podtyp zvlášť.
/// # Safety
/// `obj` musí být platný WMI objekt z právě probíhající enumerace.
unsafe fn get_prop(obj: &IWbemClassObject, name: &str) -> Option<String> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut val = VARIANT::default();
    obj.Get(PCWSTR(wide.as_ptr()), 0, &mut val, None, None)
        .ok()?;
    // Nejdřív text (názvy, verze), pak číslo (teploty, kapacity).
    let mut as_str = VARIANT::default();
    if VariantChangeType(&mut as_str, &val, Default::default(), VT_BSTR).is_ok() {
        // BSTR ve VARIANTu vlastní VARIANT — jen ho přečteme, uvolní
        // ho Drop VARIANTu, ne my.
        let s = as_str.Anonymous.Anonymous.Anonymous.bstrVal.to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    let mut as_num = VARIANT::default();
    if VariantChangeType(&mut as_num, &val, Default::default(), VT_R8).is_ok() {
        return Some(as_num.Anonymous.Anonymous.Anonymous.dblVal.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Neexistující jmenný prostor je normální stav, ne pád.
    #[test]
    fn missing_namespace_is_empty_not_panic() {
        crate::wic::init_com_for_thread();
        let rows = query(
            r"root\RozhodneNeexistuje",
            "SELECT * FROM Cokoliv",
            &["Name"],
        );
        assert!(rows.is_empty());
    }

    // root\CIMV2 má každý Windows — dotaz musí něco vrátit.
    #[test]
    fn cimv2_os_query_returns_caption() {
        crate::wic::init_com_for_thread();
        let rows = query(
            r"root\CIMV2",
            "SELECT Caption FROM Win32_OperatingSystem",
            &["Caption"],
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["Caption"].to_lowercase().contains("windows"));
    }
}
