//! Zdraví disků (SPEC kap. 11.1): NVMe health log přes
//! IOCTL_STORAGE_QUERY_PROPERTY (protocol-specific, LID 0x02) — bez
//! vendor SDK, standardní NVMe specifikace. SATA/ATA SMART zatím
//! poctivě None (doplní se později); UI ukazuje jen to, co víme.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002d_1400;

/// NVMe SMART / health informace.
#[derive(Debug, Clone, Copy)]
pub struct NvmeHealth {
    pub temp_c: i32,
    /// Zbývající rezerva bloků (%).
    pub spare_pct: u8,
    /// Odhad opotřebení (0 = nový, 100+ = za návrhovou životností).
    pub used_pct: u8,
    pub power_on_hours: u64,
    /// Bitové pole kritických varování (0 = OK).
    pub critical_warning: u8,
}

/// Přečte NVMe health log disku. None = není NVMe / nejde číst.
pub fn nvme_health(index: u32) -> Option<NvmeHealth> {
    let path: Vec<u16> = std::ffi::OsString::from(format!(r"\\.\PhysicalDrive{index}"))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: otevření disku jen pro IOCTL; handle se vždy zavře.
    unsafe {
        let handle = CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()?;

        // STORAGE_PROPERTY_QUERY (8 B hlavička) +
        // STORAGE_PROTOCOL_SPECIFIC_DATA (40 B) + 512 B log.
        let mut buf = vec![0u8; 8 + 40 + 512];
        buf[0..4].copy_from_slice(&50u32.to_le_bytes()); // StorageDeviceProtocolSpecificProperty
        buf[4..8].copy_from_slice(&0u32.to_le_bytes()); // PropertyStandardQuery
        let sp = &mut buf[8..48];
        sp[0..4].copy_from_slice(&3u32.to_le_bytes()); // ProtocolTypeNvme
        sp[4..8].copy_from_slice(&2u32.to_le_bytes()); // NVMeDataTypeLogPage
        sp[8..12].copy_from_slice(&2u32.to_le_bytes()); // LID 0x02 = health
        sp[12..16].copy_from_slice(&0u32.to_le_bytes());
        sp[16..20].copy_from_slice(&40u32.to_le_bytes()); // ProtocolDataOffset
        sp[20..24].copy_from_slice(&512u32.to_le_bytes()); // ProtocolDataLength

        let mut returned = 0u32;
        let ok = DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(buf.as_ptr() as *const c_void),
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            buf.len() as u32,
            Some(&mut returned),
            None,
        );
        let _ = CloseHandle(handle);
        ok.ok()?;

        // Výstup: STORAGE_PROTOCOL_DATA_DESCRIPTOR (8 B) + specific data;
        // log leží na 8 + ProtocolDataOffset.
        let data_off = u32::from_le_bytes(buf[8 + 16..8 + 20].try_into().ok()?) as usize;
        let log = buf.get(8 + data_off..8 + data_off + 512)?;
        let kelvin = u16::from_le_bytes(log[1..3].try_into().ok()?);
        if kelvin == 0 {
            return None; // prázdný log = disk není NVMe
        }
        Some(NvmeHealth {
            temp_c: kelvin as i32 - 273,
            spare_pct: log[3],
            used_pct: log[5],
            power_on_hours: u64::from_le_bytes(log[128..136].try_into().ok()?),
            critical_warning: log[0],
        })
    }
}
