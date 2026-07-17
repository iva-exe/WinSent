//! Fyzické disky: kumulativní čítače čtení/zápisu (IOCTL_DISK_PERFORMANCE)
//! a model disku (IOCTL_STORAGE_QUERY_PROPERTY). Enumerace jednou při
//! startu (otevřené handly se drží), čítače se čtou 1×/s — levné.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

use crate::Error;

// IOCTL kódy (winioctl.h).
const IOCTL_DISK_PERFORMANCE: u32 = 0x0007_0020;
const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002d_1400;

/// DISK_PERFORMANCE (výřez — čteme jen bajty, layout je stabilní).
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct DiskPerformance {
    bytes_read: i64,
    bytes_written: i64,
    read_time: i64,
    write_time: i64,
    idle_time: i64,
    read_count: u32,
    write_count: u32,
    queue_depth: u32,
    split_count: u32,
    query_time: i64,
    storage_device_number: u32,
    storage_manager_name: [u16; 8],
}

/// STORAGE_PROPERTY_QUERY (StorageDeviceProperty / PropertyStandardQuery).
#[repr(C)]
struct StoragePropertyQuery {
    property_id: u32,
    query_type: u32,
    additional: [u8; 1],
}

/// Jeden otevřený fyzický disk.
pub struct Disk {
    pub index: u32,
    pub model: String,
    handle: HANDLE,
}

// SAFETY: handle disku se používá výhradně z jednoho sampler vlákna.
unsafe impl Send for Disk {}

impl Drop for Disk {
    fn drop(&mut self) {
        // SAFETY: handle jsme otevřeli my.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Kumulativní bajty disku.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiskCounters {
    pub read_bytes: u64,
    pub write_bytes: u64,
}

/// Otevře všechny dostupné fyzické disky (PhysicalDrive0–15).
pub fn open_disks() -> Vec<Disk> {
    let mut disks = Vec::new();
    for index in 0..16u32 {
        let path: Vec<u16> = std::ffi::OsString::from(format!(r"\\.\PhysicalDrive{index}"))
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: standardní otevření zařízení; přístup 0 (jen IOCTL).
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        };
        let Ok(handle) = handle else { continue };
        let model = query_model(handle, index).unwrap_or_else(|| format!("Disk {index}"));
        disks.push(Disk {
            index,
            model,
            handle,
        });
    }
    disks
}

/// Přečte kumulativní čítače disku.
pub fn counters(disk: &Disk) -> Result<DiskCounters, Error> {
    let mut perf = DiskPerformance::default();
    let mut returned = 0u32;
    // SAFETY: výstupní struktura žije po dobu volání; velikost sedí.
    unsafe {
        DeviceIoControl(
            disk.handle,
            IOCTL_DISK_PERFORMANCE,
            None,
            0,
            Some(&mut perf as *mut _ as *mut c_void),
            std::mem::size_of::<DiskPerformance>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .map_err(|e| Error::Win32 {
        call: "DeviceIoControl(IOCTL_DISK_PERFORMANCE)",
        code: e.code().0,
    })?;
    Ok(DiskCounters {
        read_bytes: perf.bytes_read.max(0) as u64,
        write_bytes: perf.bytes_written.max(0) as u64,
    })
}

/// Model disku: Vendor+Product ze StorageDeviceProperty; NVMe disky
/// tam často nic nedají → fallback na registry
/// `SYSTEM\CurrentControlSet\Services\disk\Enum` (Prod_… segment).
fn query_model(handle: HANDLE, index: u32) -> Option<String> {
    let from_ioctl = query_model_ioctl(handle);
    if from_ioctl.is_some() {
        return from_ioctl;
    }
    query_model_registry(index)
}

fn query_model_ioctl(handle: HANDLE) -> Option<String> {
    let query = StoragePropertyQuery {
        property_id: 0, // StorageDeviceProperty
        query_type: 0,  // PropertyStandardQuery
        additional: [0],
    };
    let mut buf = [0u8; 1024];
    let mut returned = 0u32;
    // SAFETY: vstup i výstup jsou lokální buffery správných velikostí.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const c_void),
            std::mem::size_of::<StoragePropertyQuery>() as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            buf.len() as u32,
            Some(&mut returned),
            None,
        )
    };
    if ok.is_err() {
        return None;
    }
    // STORAGE_DEVICE_DESCRIPTOR: VendorIdOffset @8, ProductIdOffset @12.
    let read_str = |off_pos: usize| -> String {
        let Ok(bytes) = buf[off_pos..off_pos + 4].try_into() else {
            return String::new();
        };
        let off = u32::from_le_bytes(bytes) as usize;
        if off == 0 || off >= buf.len() {
            return String::new();
        }
        let end = buf[off..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| p + off)
            .unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[off..end]).trim().to_string()
    };
    let vendor = read_str(8);
    let product = read_str(12);
    let s = format!("{vendor} {product}").trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Fallback: friendly název z `Services\disk\Enum\{index}` — hodnota
/// obsahuje device instance id se segmentem `Prod_<model>`.
fn query_model_registry(index: u32) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    let subkey = HSTRING::from(r"SYSTEM\CurrentControlSet\Services\disk\Enum");
    let value = HSTRING::from(index.to_string());
    let mut len = 0u32;
    // SAFETY: dvoufázové čtení dle kontraktu RegGetValueW.
    unsafe {
        if RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        if RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let id = String::from_utf16_lossy(&buf[..end]);
        // Např. "SCSI\Disk&Ven_NVMe&Prod_Samsung SSD 970\4&..."
        let model = id
            .split("Prod_")
            .nth(1)
            .map(|rest| rest.split('\\').next().unwrap_or(rest))
            .map(|s| s.replace('_', " ").trim().to_string())
            .filter(|s| !s.is_empty());
        model
    }
}
