//! VERSIONINFO zdrojů binárky: ProductName + CompanyName (SPEC kap. 4.1
//! krok 4 a 5.2 heuristika). Volá se jen při prvním setkání s cestou.

use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

/// ProductName a CompanyName z VERSIONINFO (co soubor nese).
#[derive(Debug, Clone, Default)]
pub struct VersionStrings {
    pub product_name: Option<String>,
    pub company_name: Option<String>,
}

/// Načte řetězce z prvního jazyka v translation tabulce.
pub fn version_strings(path: &str) -> VersionStrings {
    let wide = HSTRING::from(path);
    let mut out = VersionStrings::default();

    // SAFETY: buffery vlastníme; VerQueryValueW vrací ukazatele DOVNITŘ
    // našeho bufferu `data`, který žije po celou dobu čtení.
    unsafe {
        let size = GetFileVersionInfoSizeW(&wide, None);
        if size == 0 {
            return out;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(&wide, None, size, data.as_mut_ptr() as *mut _).is_err() {
            return out;
        }

        // Translation tabulka: pole (lang, codepage) párů.
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let (lang, cp) = if VerQueryValueW(
            data.as_ptr() as *const _,
            &HSTRING::from(r"\VarFileInfo\Translation"),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            && len >= 4
        {
            let pair = *(ptr as *const [u16; 2]);
            (pair[0], pair[1])
        } else {
            (0x0409, 0x04B0) // en-US / Unicode default
        };

        let read = |name: &str| -> Option<String> {
            let query = format!(r"\StringFileInfo\{lang:04x}{cp:04x}\{name}");
            let mut sptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut slen = 0u32;
            if VerQueryValueW(
                data.as_ptr() as *const _,
                &HSTRING::from(query),
                &mut sptr,
                &mut slen,
            )
            .as_bool()
                && slen > 1
            {
                let chars = std::slice::from_raw_parts(sptr as *const u16, slen as usize);
                let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
                let s = String::from_utf16_lossy(&chars[..end]).trim().to_string();
                return (!s.is_empty()).then_some(s);
            }
            None
        };

        out.product_name = read("ProductName");
        out.company_name = read("CompanyName");
    }
    out
}
