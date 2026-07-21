//! Výčet MFT přes USN žurnál (SPEC kap. 11.2): FSCTL_ENUM_USN_DATA
//! vrací záznam pro KAŽDÝ soubor NTFS svazku za sekundy — žádné vlastní
//! indexování disku, čte se přímo struktura souborového systému.
//! Vyžaduje admin (handle na svazek) — služba je SYSTEM.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_READ, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

use crate::Error;

/// FSCTL_ENUM_USN_DATA (winioctl.h).
const FSCTL_ENUM_USN_DATA: u32 = 0x0009_00b3;

/// Jeden soubor/adresář z MFT.
pub struct UsnEntry {
    /// FileReferenceNumber (identita v MFT).
    pub file_ref: u64,
    /// Rodičovský adresář (FileReferenceNumber).
    pub parent_ref: u64,
    /// FILE_ATTRIBUTE_* bity.
    pub attrs: u32,
    pub name: String,
}

/// MFT_ENUM_DATA_V0.
#[repr(C)]
struct MftEnumData {
    start_file_reference_number: u64,
    low_usn: i64,
    high_usn: i64,
}

/// Projde celou MFT svazku (např. 'C') a zavolá callback pro každý
/// záznam. Vrací počet záznamů. Callback dostává vlastněná data —
/// žádné odkazy do interního bufferu.
pub fn enum_volume(letter: char, mut on_entry: impl FnMut(UsnEntry)) -> Result<u64, Error> {
    let path: Vec<u16> = std::ffi::OsString::from(format!(r"\\.\{letter}:"))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: standardní otevření svazku; handle se vždy zavře.
    let handle: HANDLE = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map_err(|e| Error::Win32 {
        call: "CreateFileW(volume)",
        code: e.code().0,
    })?;

    let mut med = MftEnumData {
        start_file_reference_number: 0,
        low_usn: 0,
        high_usn: i64::MAX,
    };
    let mut buf = vec![0u8; 1 << 20]; // 1 MB na dávku
    let mut count = 0u64;

    // SAFETY: buffery jsou lokální a správně velké; parsování hlídá
    // meze každého záznamu.
    unsafe {
        loop {
            let mut returned = 0u32;
            let ok = DeviceIoControl(
                handle,
                FSCTL_ENUM_USN_DATA,
                Some(&med as *const _ as *const c_void),
                std::mem::size_of::<MftEnumData>() as u32,
                Some(buf.as_mut_ptr() as *mut c_void),
                buf.len() as u32,
                Some(&mut returned),
                None,
            );
            if ok.is_err() || returned < 8 {
                break; // ERROR_HANDLE_EOF = hotovo
            }
            // Prvních 8 bajtů = další StartFileReferenceNumber.
            med.start_file_reference_number =
                u64::from_le_bytes(buf[0..8].try_into().expect("8 bajtů"));

            let mut off = 8usize;
            while off + 60 <= returned as usize {
                let rec = &buf[off..];
                let rec_len = u32::from_le_bytes(rec[0..4].try_into().expect("4")) as usize;
                if rec_len < 60 || off + rec_len > returned as usize {
                    break;
                }
                // USN_RECORD_V2 offsety (pevné).
                let file_ref = u64::from_le_bytes(rec[8..16].try_into().expect("8"));
                let parent_ref = u64::from_le_bytes(rec[16..24].try_into().expect("8"));
                let attrs = u32::from_le_bytes(rec[52..56].try_into().expect("4"));
                let name_len = u16::from_le_bytes(rec[56..58].try_into().expect("2")) as usize;
                let name_off = u16::from_le_bytes(rec[58..60].try_into().expect("2")) as usize;
                if name_off + name_len <= rec_len {
                    let name_bytes = &rec[name_off..name_off + name_len];
                    let name_u16: Vec<u16> = name_bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    on_entry(UsnEntry {
                        file_ref,
                        parent_ref,
                        attrs,
                        name: String::from_utf16_lossy(&name_u16),
                    });
                    count += 1;
                }
                off += rec_len;
            }
        }
        let _ = CloseHandle(handle);
    }
    Ok(count)
}
