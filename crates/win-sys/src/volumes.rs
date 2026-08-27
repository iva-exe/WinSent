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
    /// Index fyzického disku (IOCTL extents) — spojení s SMART kartou.
    pub disk_index: Option<u32>,
}

/// IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS.
const IOCTL_GET_EXTENTS: u32 = 0x0056_0000;

/// Fyzický disk pod svazkem (první extent — spanned svazky jsou vzácné).
fn disk_of(letter: char) -> Option<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    let path: Vec<u16> = std::ffi::OsString::from(format!(r"\\.\{letter}:"))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: standardní IOCTL čtení; handle se vždy zavře.
    unsafe {
        let h = CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()?;
        // VOLUME_DISK_EXTENTS: NumberOfDiskExtents u32 + extents
        // (DiskNumber u32 @ offset 8 prvního extentu).
        let mut buf = [0u8; 0x70];
        let mut ret = 0u32;
        let ok = DeviceIoControl(
            h,
            IOCTL_GET_EXTENTS,
            None,
            0,
            Some(buf.as_mut_ptr() as *mut _),
            buf.len() as u32,
            Some(&mut ret),
            None,
        );
        let _ = CloseHandle(h);
        ok.ok()?;
        let n = u32::from_le_bytes(buf[0..4].try_into().ok()?);
        (n >= 1).then(|| u32::from_le_bytes(buf[8..12].try_into().unwrap_or_default()))
    }
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
                disk_index: disk_of(letter),
            });
        }
    }
    out
}

/// Kolik volného místa je na svazku, kam ta cesta patří.
///
/// Používá se před přesunem databáze: nabídnout uživateli místo, kam se
/// nevejde, by znamenalo rozbít mu sběr dat až při příštím startu služby.
pub fn free_bytes(path: &std::path::Path) -> Option<u64> {
    let mut s: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
    // GetDiskFreeSpaceExW chce adresář nebo kořen; nul-ukončení povinné.
    s.push(0);
    let mut volne: u64 = 0;
    // SAFETY: buffer je nul-ukončený a výstup je lokální proměnná.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(s.as_ptr()),
            Some(&mut volne),
            None,
            None,
        )
        .ok()?;
    }
    Some(volne)
}
