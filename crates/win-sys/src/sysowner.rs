//! sysowner — „položil ten soubor na disk servisní stack Windows?"
//!
//! Jediná otázka: je vlastníkem `NT SERVICE\TrustedInstaller`? Instalátor
//! třetí strany si tuhle ownership nenastaví — i když běží jako správce,
//! nechá `BUILTIN\Administrators` nebo `NT AUTHORITY\SYSTEM`.
//!
//! Podpis se tu ZÁMĚRNĚ nečte, i když ho projekt umí (`trust.rs`):
//! vlastník stojí zlomek milisekundy, Authenticode jednotky až desítky
//! na soubor — při stovce unikátních cest na každý sken je to rozdíl
//! mezi „nepoznat" a „sekat UI". Navíc `Microsoft Windows Hardware
//! Compatibility Publisher` podepisuje i cizí ovladače, takže by podpis
//! lhal právě tam, kde ho umíme přečíst.
//!
//! Vlastnictví samo o sobě neznamená „vyrobil to Microsoft" — v System32
//! leží soubory NVIDIA nebo HP, které tam servisní stack taky položil.
//! Proto je tenhle modul jen JEDEN vstup do rozhodování ve `validate`,
//! ne odpověď.

use windows::core::HSTRING;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};

/// SID účtu `NT SERVICE\TrustedInstaller`. Je well-known — na každé
/// instalaci Windows tentýž —, takže se porovnává řetězcem.
/// `LookupAccountSidW` by se v doméně ptal řadiče a uměl by zatuhnout.
pub const TRUSTED_INSTALLER_SID: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

/// SID vlastníka souboru. `None` znamená „NEVÍM" (soubor neexistuje,
/// přístup odepřen) — volající to nesmí číst jako „ne".
pub fn file_owner_sid(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let wide = HSTRING::from(path);
    let mut owner = PSID::default();
    let mut sd = PSECURITY_DESCRIPTOR::default();

    // SAFETY: GetNamedSecurityInfoW zapíše ukazatel na deskriptor, který
    // musíme uvolnit LocalFree. `owner` ukazuje DOVNITŘ deskriptoru,
    // takže se čte, dokud deskriptor žije.
    unsafe {
        let rc = GetNamedSecurityInfoW(
            &wide,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            &mut sd,
        );
        if rc.is_err() || sd.is_invalid() || owner.is_invalid() {
            if !sd.is_invalid() {
                let _ = LocalFree(Some(HLOCAL(sd.0)));
            }
            return None;
        }
        let mut text = windows::core::PWSTR::null();
        let ok = ConvertSidToStringSidW(owner, &mut text).is_ok();
        let out = if ok && !text.is_null() {
            let s = text.to_string().ok();
            let _ = LocalFree(Some(HLOCAL(text.0 as *mut _)));
            s
        } else {
            None
        };
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        out
    }
}

/// Vlastní soubor TrustedInstaller? `None` = nešlo zjistit.
pub fn owned_by_trusted_installer(path: &str) -> Option<bool> {
    file_owner_sid(path).map(|sid| sid.eq_ignore_ascii_case(TRUSTED_INSTALLER_SID))
}
