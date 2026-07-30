//! Mazání DO KOŠE (SPEC kap. 18.2) — jediná povolená cesta odstranění.
//!
//! `SHFileOperationW` s `FOF_ALLOWUNDO` = soubor jde vrátit z koše,
//! takže i „nevratná" akce má cestu zpět. Force delete a zakázaný
//! vzor `DUPLICATE_CLOSE_SOURCE` se v projektu NEIMPLEMENTUJÍ.
//!
//! Pozor: služba běží jako SYSTEM a nemá vlastní koš — mazání se
//! proto provádí jen tam, kde koš existuje; jinak akce selže a řekne
//! to (nikdy potichu nesmaže natvrdo).

use windows::core::PCWSTR;
use windows::Win32::UI::Shell::{
    SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
    SHFILEOPSTRUCTW,
};

/// Přesune cesty do koše. Vrací chybový kód operace (0 = OK).
/// `paths` musí být absolutní; adresáře jdou taky.
pub fn to_recycle_bin(paths: &[String]) -> Result<(), crate::Error> {
    if paths.is_empty() {
        return Ok(());
    }
    // Double-NUL terminated seznam, jak SHFileOperation vyžaduje.
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(p.encode_utf16());
        from.push(0);
    }
    from.push(0);

    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        // ALLOWUNDO = koš (bez něj by se mazalo natvrdo!), SILENT +
        // NOCONFIRMATION + NOERRORUI = žádné dialogy ve službě.
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    // SAFETY: `from` žije po dobu volání; struktura je správně vyplněná.
    let rc = unsafe { SHFileOperationW(&mut op) };
    if rc != 0 {
        return Err(crate::Error::Win32 {
            call: "SHFileOperationW(FO_DELETE)",
            code: rc,
        });
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err(crate::Error::Win32 {
            call: "SHFileOperationW(přerušeno)",
            code: -1,
        });
    }
    Ok(())
}
