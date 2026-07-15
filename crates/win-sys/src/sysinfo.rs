//! Systémové metriky: celkové CPU časy a stav paměti.
//! Levná dokumentovaná API — GetSystemTimes + GlobalMemoryStatusEx.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

use crate::Error;

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
