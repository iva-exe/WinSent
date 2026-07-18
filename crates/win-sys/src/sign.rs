//! Identita podepisujícího (SPEC kap. 4.1–4.2): subjekt CN z embedded
//! Authenticode podpisu + detekce katalogového podpisu (systémové
//! soubory Windows embedded podpis nemají — jsou v katalozích).
//!
//! POMALÉ (jednotky–desítky ms) — volá se výhradně z BELOW_NORMAL
//! sig-worker vlákna, nikdy ze samplovacího cyklu. Výsledky cachuje
//! identity engine + persistentní sig_cache.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::{w, GUID, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, TRUST_E_NOSIGNATURE};
use windows::Win32::Security::Cryptography::Catalog::{
    CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2,
    CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
    CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext, CATALOG_INFO,
};
use windows::Win32::Security::Cryptography::{CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE};
use windows::Win32::Security::WinTrust::{
    WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, GENERIC_READ, OPEN_EXISTING,
};

/// Výsledek ověření podpisu souboru.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerInfo {
    pub status: SigStatus,
    /// Subjekt (CN) podepisujícího certifikátu, když je znám.
    pub subject: Option<String>,
    /// Podpis je katalogový (typicky systémové soubory Windows).
    pub catalog: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigStatus {
    Valid,
    Unsigned,
    Invalid,
}

/// Ověří podpis a vytáhne subjekt. Embedded podpis má přednost;
/// při TRUST_E_NOSIGNATURE se zkusí katalog.
pub fn signer(path: &Path) -> SignerInfo {
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

    // SAFETY: standardní VERIFY→(čtení stavu)→CLOSE sekvence; stavová
    // data WinVerifyTrust žijí mezi VERIFY a CLOSE.
    unsafe {
        let mut data = WINTRUST_DATA {
            cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
            Anonymous: WINTRUST_DATA_0 {
                pFile: &file_info as *const _ as *mut _,
            },
            dwStateAction: WTD_STATEACTION_VERIFY,
            ..Default::default()
        };
        let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let status = WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut _ as *mut _);

        let mut out = if status == 0 {
            SignerInfo {
                status: SigStatus::Valid,
                subject: subject_from_state(data.hWVTStateData),
                catalog: false,
            }
        } else if status == TRUST_E_NOSIGNATURE.0 {
            SignerInfo {
                status: SigStatus::Unsigned,
                subject: None,
                catalog: false,
            }
        } else {
            SignerInfo {
                status: SigStatus::Invalid,
                subject: subject_from_state(data.hWVTStateData),
                catalog: false,
            }
        };

        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut _ as *mut _);

        // Bez embedded podpisu: katalogy (WTD_CHOICE_CATALOG cesta,
        // SPEC 4.2 — jinak systémové soubory vyjdou jako nepodepsané).
        if out.status == SigStatus::Unsigned {
            if let Some(catalog_path) = find_catalog(&wide) {
                out.status = SigStatus::Valid;
                out.catalog = true;
                // Katalogy v %SystemRoot%\System32\catroot podepisuje
                // Microsoft pro komponenty Windows — poctivá zkratka
                // místo drahého ověřování podpisu katalogu per soubor.
                if catalog_path.to_lowercase().contains("\\catroot\\") {
                    out.subject = Some("Microsoft Windows".to_string());
                }
            }
        }
        out
    }
}

/// Subjekt (simple display name) leaf certifikátu z WinVerifyTrust stavu.
unsafe fn subject_from_state(state: HANDLE) -> Option<String> {
    if state.is_invalid() {
        return None;
    }
    let prov = WTHelperProvDataFromStateData(state);
    if prov.is_null() {
        return None;
    }
    let sgnr = WTHelperGetProvSignerFromChain(prov, 0, false, 0);
    if sgnr.is_null() || (*sgnr).csCertChain == 0 || (*sgnr).pasCertChain.is_null() {
        return None;
    }
    let cert = (*(*sgnr).pasCertChain).pCert;
    if cert.is_null() {
        return None;
    }
    let mut buf = [0u16; 256];
    let len = CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut buf));
    if len <= 1 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize - 1]))
}

/// Najde katalog obsahující SHA-256 hash souboru; vrací cestu katalogu.
unsafe fn find_catalog(wide_path: &[u16]) -> Option<String> {
    let file = CreateFileW(
        PCWSTR(wide_path.as_ptr()),
        GENERIC_READ.0,
        FILE_SHARE_READ,
        None,
        OPEN_EXISTING,
        FILE_FLAGS_AND_ATTRIBUTES(0),
        None,
    )
    .ok()?;

    let mut admin: isize = 0;
    let result = (|| -> Option<String> {
        CryptCATAdminAcquireContext2(&mut admin, None, w!("SHA256"), None, 0).ok()?;

        let mut hash_len = 0u32;
        let _ = CryptCATAdminCalcHashFromFileHandle2(admin, file, &mut hash_len, None, 0);
        if hash_len == 0 {
            return None;
        }
        let mut hash = vec![0u8; hash_len as usize];
        CryptCATAdminCalcHashFromFileHandle2(
            admin,
            file,
            &mut hash_len,
            Some(hash.as_mut_ptr()),
            0,
        )
        .ok()?;

        let cat = CryptCATAdminEnumCatalogFromHash(admin, &hash, 0, None);
        if cat == 0 {
            return None;
        }
        let mut info = CATALOG_INFO {
            cbStruct: std::mem::size_of::<CATALOG_INFO>() as u32,
            ..Default::default()
        };
        let path = if CryptCATCatalogInfoFromContext(cat, &mut info, 0).is_ok() {
            let end = info
                .wszCatalogFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info.wszCatalogFile.len());
            Some(String::from_utf16_lossy(&info.wszCatalogFile[..end]))
        } else {
            // Katalog existuje (soubor je katalogově podepsaný),