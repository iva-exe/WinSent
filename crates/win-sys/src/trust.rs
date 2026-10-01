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

/// Podepisující platného katalogu, ve kterém je soubor zapsaný — nebo
/// `None`, když v žádném systémovém katalogu není (nebo se katalog
/// neověří).
///
/// Většina souborů Windows nemá embedded podpis, podepsané jsou jen
/// otiskem v katalogu (`CatRoot`). Identita je dřív brala podle cesty:
/// „bez podpisu pod %SystemRoot%" = Windows. To ale platilo i pro
/// libovolnou nepodepsanou binárku v uživatelsky zapisovatelném
/// `C:\Windows\Temp` nebo `C:\Windows\Tasks` — monitor ji sám schoval
/// pod řádek „Windows" s přesnou identitou. Tohle je poctivý test
/// „soubor je opravdu ze systému": otisk souboru je v katalogu
/// a katalog má platný podpis.
///
/// Blokující (jednotky až desítky ms) — jen z background vlákna
/// identity, a jen u souborů bez embedded podpisu.
pub fn catalog_signer(path: &Path) -> Option<String> {
    use std::os::windows::io::AsRawHandle;
    let file = std::fs::File::open(path).ok()?;
    let h = windows::Win32::Foundation::HANDLE(file.as_raw_handle());
    // Katalogy Windows 10/11 nesou SHA-256; starší balíčky ovladačů
    // jen SHA-1.
    [windows::core::w!("SHA256"), windows::core::w!("SHA1")]
        .into_iter()
        .find_map(|alg| catalog::signer(path, h, alg))
}

/// Ověření souboru proti systémovým katalogům (WTD_CHOICE_CATALOG).
mod catalog {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::{GUID, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::Cryptography::Catalog::{
        CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2,
        CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
        CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext, CATALOG_INFO,
    };
    use windows::Win32::Security::Cryptography::{
        CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    };
    use windows::Win32::Security::WinTrust::{
        WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain,
        WTHelperProvDataFromStateData, WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2,
        WINTRUST_CATALOG_INFO, WINTRUST_DATA, WINTRUST_DATA_0, WTD_CHOICE_CATALOG, WTD_REVOKE_NONE,
        WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };

    /// Jeden pokus s daným hashovacím algoritmem.
    pub(super) fn signer(path: &Path, file: HANDLE, alg: PCWSTR) -> Option<String> {
        let mut admin: isize = 0;
        // SAFETY: kontext se vždy uvolní níž; `admin` žije přes celé
        // použití v `with_admin`.
        unsafe { CryptCATAdminAcquireContext2(&mut admin, None, alg, None, None) }.ok()?;
        let out = with_admin(path, file, admin);
        // SAFETY: `admin` pochází z úspěšného AcquireContext2.
        unsafe {
            let _ = CryptCATAdminReleaseContext(admin, 0);
        }
        out
    }

    fn with_admin(path: &Path, file: HANDLE, admin: isize) -> Option<String> {
        // SAFETY: dvoufázové volání podle kontraktu — nejdřív délka,
        // pak buffer té délky. První volání s prázdným bufferem končí
        // chybou „málo místa", délku ale vyplní.
        let mut hash = unsafe {
            let mut len = 0u32;
            let _ = CryptCATAdminCalcHashFromFileHandle2(admin, file, &mut len, None, None);
            if len == 0 || len > 64 {
                return None;
            }
            let mut hash = vec![0u8; len as usize];
            CryptCATAdminCalcHashFromFileHandle2(
                admin,
                file,
                &mut len,
                Some(hash.as_mut_ptr()),
                None,
            )
            .ok()?;
            hash.truncate(len as usize);
            hash
        };
        // SAFETY: hash je platný buffer; vrácený kontext katalogu se
        // uvolní hned po použití.
        let cat = unsafe { CryptCATAdminEnumCatalogFromHash(admin, &hash, None, None) };
        if cat == 0 {
            return None;
        }
        let mut info = CATALOG_INFO {
            cbStruct: std::mem::size_of::<CATALOG_INFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `cat` je platný kontext z EnumCatalogFromHash.
        let out = if unsafe { CryptCATCatalogInfoFromContext(cat, &mut info, 0) }.is_ok() {
            verify(path, file, admin, &info, &mut hash)
        } else {
            None
        };
        // SAFETY: párové uvolnění kontextu katalogu.
        unsafe {
            let _ = CryptCATAdminReleaseCatalogContext(admin, cat, 0);
        }
        out
    }

    /// WinVerifyTrust v režimu katalogu; při úspěchu jméno toho, kdo
    /// katalog podepsal.
    fn verify(
        path: &Path,
        file: HANDLE,
        admin: isize,
        info: &CATALOG_INFO,
        hash: &mut [u8],
    ) -> Option<String> {
        // Člen katalogu se adresuje otiskem jako hex řetězcem.
        let tag: Vec<u16> = hash
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<String>()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let wpath: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let ci = WINTRUST_CATALOG_INFO {
            cbStruct: std::mem::size_of::<WINTRUST_CATALOG_INFO>() as u32,
            dwCatalogVersion: 0,
            pcwszCatalogFilePath: PCWSTR(info.wszCatalogFile.as_ptr()),
            pcwszMemberTag: PCWSTR(tag.as_ptr()),
            pcwszMemberFilePath: PCWSTR(wpath.as_ptr()),
            hMemberFile: file,
            pbCalculatedFileHash: hash.as_mut_ptr(),
            cbCalculatedFileHash: hash.len() as u32,
            pcCatalogContext: std::ptr::null_mut(),
            hCatAdmin: admin,
        };
        // SAFETY: všechny ukazatele v `ci` míří do bufferů, které žijí
        // do konce funkce; VERIFY naplní stav, párové CLOSE ho uvolní
        // a ze stavu se čte jen mezi nimi.
        unsafe {
            let mut data = WINTRUST_DATA {
                cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
                dwUIChoice: WTD_UI_NONE,
                fdwRevocationChecks: WTD_REVOKE_NONE,
                dwUnionChoice: WTD_CHOICE_CATALOG,
                Anonymous: WINTRUST_DATA_0 {
                    pCatalog: &ci as *const _ as *mut _,
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
            let subject = if status == 0 {
                signer_of_state(data.hWVTStateData)
            } else {
                None
            };
            data.dwStateAction = WTD_STATEACTION_CLOSE;
            WinVerifyTrust(
                HWND::default(),
                &mut action,
                &mut data as *mut _ as *mut c_void,
            );
            subject
        }
    }

    /// SAFETY: `state` je hWVTStateData z úspěšného VERIFY, ještě před CLOSE.
    unsafe fn signer_of_state(state: HANDLE) -> Option<String> {
        let prov = WTHelperProvDataFromStateData(state);
        if prov.is_null() {
            return None;
        }
        let sgnr = WTHelperGetProvSignerFromChain(prov, 0, false, 0);
        if sgnr.is_null() {
            return None;
        }
        let cert = WTHelperGetProvCertFromChain(sgnr, 0);
        if cert.is_null() || (*cert).pCert.is_null() {
            return None;
        }
        let pcert = (*cert).pCert;
        let len = CertGetNameStringW(pcert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, None);
        if len <= 1 {
            return None;
        }
        let mut name = vec![0u16; len as usize];
        CertGetNameStringW(
            pcert,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            None,
            Some(&mut name),
        );
        let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        let s = String::from_utf16_lossy(&name[..end]).trim().to_string();
        (!s.is_empty()).then_some(s)
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

#[cfg(test)]
mod tests {
    use super::*;

    // Systémový soubor bez embedded podpisu se pozná podle katalogu
    // podepsaného Microsoftem; nepodepsaný soubor v katalogu není.
    #[test]
    fn catalog_signer_tells_system_file_from_unsigned() {
        let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let mut nalezeno = 0;
        for jmeno in ["svchost.exe", "notepad.exe", "cmd.exe", "kernel32.dll"] {
            let p = Path::new(&sysroot).join("System32").join(jmeno);
            if !p.exists() || signer::embedded_subject(&p).is_some() {
                continue;
            }
            let s = catalog_signer(&p);
            assert!(
                s.as_deref().is_some_and(|s| s.contains("Microsoft")),
                "{jmeno}: {s:?}"
            );
            nalezeno += 1;
        }
        assert!(nalezeno > 0, "žádný katalogově podepsaný soubor k ověření");

        let tmp = std::env::temp_dir().join(format!("ws-trust-{}.exe", std::process::id()));
        std::fs::write(&tmp, b"MZ neni to program").unwrap();
        let s = catalog_signer(&tmp);
        let _ = std::fs::remove_file(&tmp);
        assert!(s.is_none(), "{s:?}");
    }
}
