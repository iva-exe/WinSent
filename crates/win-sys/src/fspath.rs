//! Kanonický tvar cesty a odkazy v ní (v11) — pro validační vrstvu.
//!
//! Kontrola „smí se tohle smazat?" porovnává řetězce. To má smysl jen
//! nad cestou, která je JEDNOZNAČNÁ: `C:\PROGRA~1` je tatáž složka jako
//! `C:\Program Files`, `…\Temp\..\..\Windows` vede do Windows a junction
//! v Tempu může ukazovat kamkoli. Tady se cesta převede na plný dlouhý
//! tvar a zjistí se, jestli se po cestě někde neodbočuje.
//!
//! Jen čte metadata — nic neotevírá pro čtení obsahu (u souborů
//! OneDrive by to stáhlo data z cloudu).

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    FindClose, FindFirstFileW, GetFileAttributesW, GetFullPathNameW, GetLongPathNameW,
    GetShortPathNameW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES, WIN32_FIND_DATAW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Volání ve tvaru „zeptej se na délku, alokuj, zavolej znovu".
fn two_phase(f: impl Fn(Option<&mut [u16]>) -> u32) -> Option<String> {
    let need = f(None);
    if need == 0 {
        return None;
    }
    let mut buf = vec![0u16; need as usize + 1];
    let len = f(Some(&mut buf));
    if len == 0 || len as usize >= buf.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// Plná cesta (`GetFullPathNameW`) rozvinutá na dlouhá jména
/// (`GetLongPathNameW` — `PROGRA~1` → `Program Files`). Cesta musí
/// existovat, jinak dlouhá jména zjistit nejde → `None`.
pub fn full_long_path(path: &str) -> Option<String> {
    if path.contains('\0') {
        return None;
    }
    let w = wide(path);
    // SAFETY: vstup je NUL-ukončený, výstupní buffer má délku, kterou
    // si API samo řeklo.
    let full = two_phase(|buf| unsafe { GetFullPathNameW(PCWSTR(w.as_ptr()), buf, None) })?;
    let wf = wide(&full);
    // SAFETY: totéž.
    two_phase(|buf| unsafe { GetLongPathNameW(PCWSTR(wf.as_ptr()), buf) })
}

/// Krátký (8.3) tvar existující cesty. Pro testy obcházení kontrol
/// — v produkci se nevolá.
pub fn short_path(path: &str) -> Option<String> {
    let w = wide(path);
    // SAFETY: vstup je NUL-ukončený, buffer má délku od API.
    two_phase(|buf| unsafe { GetShortPathNameW(PCWSTR(w.as_ptr()), buf) })
}

/// Co leží na jednom místě cesty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Odkaz {
    /// Obyčejný soubor nebo složka.
    Zadny,
    /// Reparse point s tímto tagem (junction, symlink, cloud soubor…).
    Tag(u32),
    /// Atributy nejdou přečíst.
    Necitelne,
}

/// Je na cestě (soubor či složka) reparse point, a jaký?
pub fn reparse(path: &str) -> Odkaz {
    let w = wide(path);
    // SAFETY: NUL-ukončený vstup.
    let attrs = unsafe { GetFileAttributesW(PCWSTR(w.as_ptr())) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        return Odkaz::Necitelne;
    }
    if attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 {
        return Odkaz::Zadny;
    }
    // Tag nese WIN32_FIND_DATAW.dwReserved0 — bez otevírání souboru.
    let mut fd = WIN32_FIND_DATAW::default();
    // SAFETY: handle se zavírá; `fd` je lokální.
    unsafe {
        match FindFirstFileW(PCWSTR(w.as_ptr()), &mut fd) {
            Ok(h) => {
                let _ = FindClose(h);
                Odkaz::Tag(fd.dwReserved0)
            }
            Err(_) => Odkaz::Necitelne,
        }
    }
}

/// Tagy, které cestu NEPŘESMĚROVÁVAJÍ jinam: soubory OneDrive a jiných
/// cloudových synchronizací (rodina `IO_REPARSE_TAG_CLOUD_*`), deduplikace
/// a komprimace Windows (WOF). Junction, symlink, přípojný bod svazku
/// i linuxové symlinky WSL mají bit „name surrogate" a sem nepatří.
pub fn tag_neodkazuje(tag: u32) -> bool {
    const DEDUP: u32 = 0x8000_0013;
    const WOF: u32 = 0x8000_0017;
    // IO_REPARSE_TAG_CLOUD až CLOUD_F: 0x9000_001A, 0x9000_101A … 0x9000_F01A.
    let cloud = tag & 0xFFFF_0FFF == 0x9000_001A;
    cloud || tag == DEDUP || tag == WOF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plna_dlouha_cesta() {
        let tmp = std::env::temp_dir().join("winsent-fspath-test.tmp");
        std::fs::write(&tmp, b"x").expect("zapsat");
        let s = tmp.to_string_lossy().into_owned();
        let p = full_long_path(&s).expect("existující cesta");
        assert!(
            p.eq_ignore_ascii_case(&s)
                || p.to_ascii_lowercase().ends_with("winsent-fspath-test.tmp")
        );
        // Krátký tvar se rozvine zpátky.
        if let Some(short) = short_path(&s) {
            let back = full_long_path(&short).expect("zpět na dlouhý tvar");
            assert!(back.eq_ignore_ascii_case(&p), "{short} → {back} ≠ {p}");
        }
        assert_eq!(reparse(&s), Odkaz::Zadny);
        let _ = std::fs::remove_file(&tmp);
        assert!(full_long_path(&s).is_none(), "neexistující cesta");
        assert!(full_long_path("C:\\a\0b").is_none());
    }

    #[test]
    fn cloud_ano_junction_ne() {
        assert!(tag_neodkazuje(0x9000_001A));
        assert!(tag_neodkazuje(0x9000_601A));
        assert!(tag_neodkazuje(0x8000_0013));
        assert!(!tag_neodkazuje(0xA000_0003), "mount point / junction");
        assert!(!tag_neodkazuje(0xA000_000C), "symlink");
        assert!(!tag_neodkazuje(0xA000_001D), "WSL symlink");
        assert!(!tag_neodkazuje(0x8000_001B), "AppExecLink");
    }
}
