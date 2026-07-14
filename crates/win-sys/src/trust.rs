//! Ověření Authenticode podpisu souboru přes WinVerifyTrust.
//!
//! Používá služba při startu ke kontrole integrity vlastních binárek
//! (SPEC kap. 2.3). Ověření katalogovým podpisem (WTD_CHOICE_CATALOG,
//! kap. 4.2) přijde s cache podpisů ve v2 — pro vlastní binárky stačí
//! embedded podpis souboru.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::{
    HWND, TRUST_E_NOSIGNATURE, TRUST_E_PROVIDER_UNKNOWN, TRUST_E_SUBJECT_FORM_UNKNOWN,
};
use windows::Win32::Security::WinTrust::{
    WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
    WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};

/// Výsledek ověření podpisu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Podpis existuje a řetěz důvěry je platný.
    Valid,
    /// Soubor nemá žádný podpis (během vývoje očekávaný stav).
    Unsigned,
    /// Podpis existuje, ale neověřil se — soubor mohl být podvržen.
    Invalid { code: i32 },
}

/// Ověří Authenticode podpis souboru. Blokující volání (jednotky až
/// desítky ms) — nikdy nevolat v horké cestě, jen při startu služby.
pub fn verify_authenticode(path: &Path) -> Result<SignatureStatus, crate::Error> {
    // Cesta jako NUL-ukončený UTF-16 řetězec pro Win32.
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };

    // SAFETY: struktura se předává WinVerifyTrust dle dokumentace —
    // VERIFY naplní stav, párové CLOSE ho vždy uvolní.
    let status = unsafe {
        let mut data = WINTRUST_DATA {
            cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 {
                pFile: &file_info as *const _ as *mut _,
            },
            dwStateAction: WTD_STATEACTION_VERIFY,
            ..Default::default()
        };
        let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let status = WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut c_void,
        );

        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut c_void,
        );
        status
    };

    // Chybové kódy „není podepsané“ vs. „podpis nesedí“ rozlišujeme,
    // protože během vývoje je Unsigned jen varování, Invalid je problém.
    Ok(match status {
        0 => SignatureStatus::Valid,
        s if s == TRUST_E_NOSIGNATURE.0
            || s == TRUST_E_SUBJECT_FORM_UNKNOWN.0
            || s == TRUST_E_PROVIDER_UNKNOWN.0 =>
        {
            SignatureStatus::Unsigned
        }
        s => SignatureStatus::Invalid { code: s },
    })
}
