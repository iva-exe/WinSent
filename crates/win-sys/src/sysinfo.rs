//! Systémové metriky: celkové CPU časy a stav paměti.
//! Levná dokumentovaná API — GetSystemTimes + GlobalMemoryStatusEx.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Power::{CallNtPowerInformation, PROCESSOR_POWER_INFORMATION};
use windows::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows::Win32::System::Threading::GetSystemTimes;

use crate::Error;

/// Uptime systému v sekundách (od bootu).
pub fn system_uptime_s() -> u64 {
    // SAFETY: čisté čtení čítače.
    unsafe { GetTickCount64() / 1000 }
}

/// Takty CPU: (aktuální průměr MHz, max MHz) přes CallNtPowerInformation
/// (SPEC kap. 15.2 stupeň 3 — dostupné na 100 % strojů).
pub fn cpu_clocks(n_cpus: usize) -> Result<(u32, u32), Error> {
    let mut info = vec![PROCESSOR_POWER_INFORMATION::default(); n_cpus];
    // SAFETY: výstupní pole má přesnou velikost dle kontraktu API.
    let status = unsafe {
        CallNtPowerInformation(
            windows::Win32::System::Power::ProcessorInformation,
            None,
            0,
            Some(info.as_mut_ptr() as *mut _),
            (info.len() * std::mem::size_of::<PROCESSOR_POWER_INFORMATION>()) as u32,
        )
    };
    if status.0 != 0 {
        return Err(Error::Win32 {
            call: "CallNtPowerInformation(ProcessorInformation)",
            code: status.0,
        });
    }
    let cur =
        (info.iter().map(|p| p.CurrentMhz as u64).sum::<u64>() / info.len().max(1) as u64) as u32;
    let max = info.iter().map(|p| p.MaxMhz).max().unwrap_or(0);
    Ok((cur, max))
}

/// Kumulativní systémové časy v jednotkách 100 ns (od bootu).
/// `kernel` ZAHRNUJE `idle` — busy = (kernel - idle) + user.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

fn filetime_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// Přečte kumulativní CPU časy systému.
pub fn system_times() -> Result<SystemTimes, Error> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: výstupní struktury žijí po dobu volání.
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.map_err(
        |e| Error::Win32 {
            call: "GetSystemTimes",
            code: e.code().0,
        },
    )?;
    Ok(SystemTimes {
        idle: filetime_u64(idle),
        kernel: filetime_u64(kernel),
        user: filetime_u64(user),
    })
}

// Per-core časy: NtQuerySystemInformation(SystemProcessorPerformanceInformation).
#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(
        class: u32,
        info: *mut std::ffi::c_void,
        len: u32,
        ret_len: *mut u32,
    ) -> i32;
}

const SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION_CLASS: u32 = 8;

/// SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION (winternl, stabilní layout).
#[repr(C)]
#[derive(Clone)]
struct ProcessorPerf {
    idle: i64,
    kernel: i64,
    user: i64,
    dpc: i64,
    interrupt: i64,
    interrupt_count: u32,
}

/// Kumulativní časy jednoho jádra (100 ns). `kernel` zahrnuje `idle`.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoreTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

/// Časy jednotlivých logických jader.
///
/// `n_cpus` je jen odhad, kolik jich čekat. Buffer se v případě potřeby
/// zvětší a dotaz zopakuje — a to není opatrnost do zásoby:
/// `available_parallelism()` vrací počet procesorů dostupných PROCESU,
/// což na strojích s víc než 64 logickými jádry znamená jednu skupinu
/// procesorů, kdežto tenhle dotaz chce buffer na jádra VŠECHNA. Menší
/// buffer skončí na STATUS_INFO_LENGTH_MISMATCH, dotaz selže při každém
/// pokusu a s ním padal celý vzorek — na takovém stroji zůstala sekce
/// Tasks navždy prázdná, přestože služba jela.
pub fn core_times(n_cpus: usize) -> Result<Vec<CoreTimes>, Error> {
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
    let mut want = n_cpus.max(1);
    let mut status = 0;
    let mut ret_len = 0u32;
    let mut buf = Vec::new();

    // Nanejvýš pár pokusů: 2048 jader nemá ani ten největší server.
    for _ in 0..6 {
        buf = vec![
            ProcessorPerf {
                idle: 0,
                kernel: 0,
                user: 0,
                dpc: 0,
                interrupt: 0,
                interrupt_count: 0,
            };
            want
        ];
        ret_len = 0;
        // SAFETY: buffer má přesnou velikost pole struktur; délku validujeme.
        status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION_CLASS,
                buf.as_mut_ptr() as *mut _,
                (buf.len() * std::mem::size_of::<ProcessorPerf>()) as u32,
                &mut ret_len,
            )
        };
        if status != STATUS_INFO_LENGTH_MISMATCH {
            break;
        }
        want = (want * 2).min(2048);
    }
    if status != 0 {
        return Err(Error::Win32 {
            call: "NtQuerySystemInformation(SystemProcessorPerformanceInformation)",
            code: status,
        });
    }
    let filled = ret_len as usize / std::mem::size_of::<ProcessorPerf>();
    Ok(buf
        .iter()
        .take(filled)
        .map(|p| CoreTimes {
            idle: p.idle.max(0) as u64,
            kernel: p.kernel.max(0) as u64,
            user: p.user.max(0) as u64,
        })
        .collect())
}

/// Stav fyzické paměti: (použito MB, celkem MB).
pub fn memory_status_mb() -> Result<(u64, u64), Error> {
    let mut mem = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: struktura s vyplněnou délkou dle kontraktu API.
    unsafe { GlobalMemoryStatusEx(&mut mem) }.map_err(|e| Error::Win32 {
        call: "GlobalMemoryStatusEx",
        code: e.code().0,
    })?;
    let total = mem.ullTotalPhys / (1024 * 1024);
    let used = (mem.ullTotalPhys - mem.ullAvailPhys) / (1024 * 1024);
    Ok((used, total))
}
