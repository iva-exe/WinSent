//! Statické informace o CPU: název a základní takt z registru,
//! fyzická jádra a cache z GetLogicalProcessorInformationEx.
//! Zjišťuje se jednou při startu služby.

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationAll, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

/// Statický popis CPU.
#[derive(Debug, Clone, Default)]
pub struct CpuStatic {
    pub name: String,
    pub base_mhz: u32,
    pub physical_cores: u32,
    pub logical_cores: u32,
    /// Součty cache přes všechna jádra (jako Správce úloh), v kB.
    pub l1_kb: u32,
    pub l2_kb: u32,
    pub l3_kb: u32,
}

/// Načte statické CPU info. Chyby jednotlivých zdrojů degradují na
/// prázdné hodnoty — nikdy neshodí start služby.
pub fn cpu_static() -> CpuStatic {
    let mut out = CpuStatic {
        logical_cores: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1),
        ..Default::default()
    };

    // Název + základní takt z registru (zapisuje je tam Windows).
    let key = w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
    out.name = reg_string(key, w!("ProcessorNameString")).unwrap_or_default();
    out.base_mhz = reg_dword(key, w!("~MHz")).unwrap_or(0);

    // Fyzická jádra + cache.
    let mut len = 0u32;
    // SAFETY: první volání zjistí délku, druhé plní buffer té délky.
    unsafe {
        let _ = GetLogicalProcessorInformationEx(RelationAll, None, &mut len);
        let mut buf = vec![0u8; len as usize];
        if GetLogicalProcessorInformationEx(
            RelationAll,
            Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut len,
        )
        .is_err()
        {
            return out;
        }

        // Ruční průchod: hlavička = Relationship u32 + Size u32; union od
        // offsetu 8. RelationProcessorCore=0, RelationCache=2.
        let mut off = 0usize;
        while off + 8 <= len as usize {
            let rel = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
            let size = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap()) as usize;
            if size == 0 || off + size > len as usize {
                break;
            }
            match rel {
                0 => out.physical_cores += 1,
                2 => {
                    // CACHE_RELATIONSHIP: Level u8 @8, CacheSize u32 @12.
                    let level = buf[off + 8];
                    let cache_kb =
                        u32::from_le_bytes(buf[off + 12..off + 16].try_into().unwrap()) / 1024;
                    match level {
                        1 => out.l1_kb += cache_kb,
                        2 => out.l2_kb += cache_kb,
                        3 => out.l3_kb += cache_kb,
                        _ => {}
                    }
                }
                _ => {}
            }
            off += size;
        }
    }
    out
}

/// REG_SZ hodnota z HKLM.
fn reg_string(subkey: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut len = 0u32;
    // SAFETY: dvoufázové čtení dle kontraktu RegGetValueW.
    unsafe {
        if RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
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
            subkey,
            value,
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
        Some(String::from_utf16_lossy(&buf[..end]).trim().to_string())
    }
}

/// REG_DWORD hodnota z HKLM.
fn reg_dword(subkey: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut out = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    // SAFETY: výstup je lokální DWORD.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut out as *mut _ as *mut _),
            Some(&mut len),
        )
        .is_ok()
        .then_some(out)
    }
}
