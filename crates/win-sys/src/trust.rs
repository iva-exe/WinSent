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

/// Výsledek zjištění podpisu pro identitu (SPEC kap. 4.1 krok 4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignerInfo {
    /// Subject CN z podpisového certifikátu (např. „Google LLC“).
    /// None = bez embedded podpisu (typicky katalogově podepsané
    /// systémové soubory — ty identita pozná podle cesty %SystemRoot%).
    pub subject: Option<String>,
    /// Podpis existuje a řetěz důvěry je platný.
    pub valid: bool,
}

/// Zjistí podepisujícího binárky: subject CN embedded podpisu
/// (CryptQueryObject) + zda je řetěz platný (WinVerifyTrust). Katalogově
/// podepsané systémové soubory nemají embedded podpis a vrací
/// `subject: None` — identita je zařadí větví „os:windows“ podle cesty.
/// Blokující (jednotky až desítky ms) — jen z background vlákna
/// identity, NIKDY v samplovacím cyklu (SPEC kap. 4.2).
pub fn signer_subject(path: &Path) -> SignerInfo {
    let valid = matches!(verify_authenticode(path), Ok(SignatureStatus::Valid));
    SignerInfo {
        subject: signer::embedded_subject(path),
        valid,
    }
}

/// Extrakce subjektu embedded Authenticode podpisu přes CryptQueryObject.
mod signer {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::Win32::Security::Cryptography::{
        CertCloseStore, CertFindCertificateInStore, CertFreeCertificateContext, CertGetNameStringW,
        CryptMsgClose, CryptMsgGetParam, CryptQueryObject, CERT_FIND_SUBJECT_CERT,
        CERT_NAME_SIMPLE_DISPLAY_TYPE, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
        CERT_QUERY_FORMAT_FLAG_BINARY, CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_CERT_INFO_PARAM,
        HCERTSTORE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };

    /// Subject CN prvního signera embedded podpisu; None když soubor
    /// nemá embedded podpis (nebo se nedá přečíst).
    pub fn embedded_subject(path: &Path) -> Option<String> {
        let w: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: store i msg handle uvolňujeme; buffer signer info žije
        // po dobu, kdy z něj čteme certifikát.
        unsafe {
            let mut store = HCERTSTORE::default();
            let mut msg: *mut core::ffi::c_void = std::ptr::null_mut();
            CryptQueryObject(
                CERT_QUERY_OBJECT_FILE,
                w.as_ptr() as *const core::ffi::c_void,
                CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
                CERT_QUERY_FORMAT_FLAG_BINARY,
                0,
                None,
                None,
                None,
                Some(&mut store),
                Some(&mut msg),
                None,
            )
            .ok()?;

            let result = subject_from_msg(msg, store);

            if !msg.is_null() {
                let _ = CryptMsgClose(Some(msg));
            }
            if !store.is_invalid() {
                let _ = CertCloseStore(Some(store), 0);
            }
            result
        }
    }

    /// SAFETY: msg a store jsou platné handly z CryptQueryObject.
    unsafe fn subject_from_msg(msg: *mut core::ffi::c_void, store: HCERTSTORE) -> Option<String> {
        // Velikost CERT_INFO signera.
        let mut size = 0u32;
        CryptMsgGetParam(msg, CMSG_SIGNER_CERT_INFO_PARAM, 0, None, &mut size).ok()?;
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        CryptMsgGetParam(
            msg,
            CMSG_SIGNER_CERT_INFO_PARAM,
            0,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            &mut size,
        )
        .ok()?;

        // Najdi certifikát signera ve store dle vráceného CERT_INFO.
        let cert = CertFindCertificateInStore(
            store,
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            0,
            CERT_FIND_SUBJECT_CERT,
            Some(buf.as_ptr() as *const core::ffi::c_void),
            None,
        );
        if cert.is_null() {
            return None;
        }
        // Subject jako čitelný display name.
        let len = CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, None);
        let subject = if len > 1 {
            let mut name = vec![0u16; len as usize];
            CertGetNameStringW(
                cert,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                None,
                Some(&mut name),
            );
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            let s = String::from_utf16_lossy(&name[..end]).trim().to_string();
            (!s.is_empty()).then_some(s)
        } else {
            None
        };
        let _ = CertFreeCertificateContext(Some(cert));
        subject
    }
}
