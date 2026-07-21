//! Logické svazky (SPEC kap. 11.1): písmena, label, souborový systém,
//! kapacita a volné místo. Levné — volá se on-demand.

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

/// Jeden logický svazek.
#[derive(Debug, Clone)]
pub struct Volume {
    pub letter: char,
    pub label: String,
    pub fs: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Jen pevné disky (DRIVE_FIXED) mají true — síťové/USB se v UI liší.
    pub fixed: bool,
}

/// Vyjmenuje připojené svazky (bez CD/RAM disků).
pub fn volumes() -> Vec<Volume> {
    let mut out = Vec::new();
    // SAFETY: čisté čtení; buffery mají pevné velikosti dle API.
    unsafe {
        let mask = GetLogicalDrives();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain([0]).collect();
            let dtype = GetDriveTypeW(PCWSTR(root.as_ptr()));
            // 3 = DRIVE_FIXED, 2 = DRIVE_REMOVABLE; CD (5) a RAM (6) ne.
            if dtype != 3 && dtype != 2 {
                continue;
            }
            let mut label = [0u16; 64];
            let mut fs = [0u16; 32];
            let ok = GetVolumeInformationW(
                PCWSTR(root.as_ptr()),
                Some(&mut label),
                None,
                None,
                None,
                Some(&mut fs),
            );
            if ok.is_err() {
                continue;
            }
            let mut free = 0u64;
            let mut total = 0u64;
            let _ = GetDiskFreeSpaceExW(
                PCWSTR(root.as_ptr()),
                None,
                Some(&mut total),
                Some(&mut free),
            );
            let str_of = |b: &[u16]| {
                let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
                String::from_utf16_lossy(&b[..end])
            };
            out.push(Volume {
                letter,
                label: str_of(&label),
                fs: str_of(&fs),
                total_bytes: total,
                free_bytes: free,
                fixed: dtype == 3,
            });
        }
    }
    out
}
