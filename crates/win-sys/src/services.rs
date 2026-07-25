//! Služby Windows (SPEC kap. 7, backend 4): výčet automaticky
//! startujících služeb přes `EnumServicesStatusExW` a přepnutí
//! start typu přes `ChangeServiceConfigW` (auto ↔ demand — NIKDY
//! mazání služby).

use std::ffi::c_void;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW,
    ENUM_SERVICE_STATUS_PROCESSW, SC_ENUM_PROCESS_INFO, SC_MANAGER_CONNECT,
    SC_MANAGER_ENUMERATE_SERVICE, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG, SERVICE_DEMAND_START,
    SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG, SERVICE_STATE_ALL, SERVICE_WIN32,
};

use crate::Error;

/// Jedna služba se startem při bootu.
#[derive(Debug, Clone)]
pub struct AutoService {
    pub name: String,
    pub display_name: String,
    /// Běží právě teď? (SERVICE_RUNNING = 4)
    pub running: bool,
    /// Automatický start (jinak už je přepnutá na ruční).
    pub auto_start: bool,
}

/// Vyjmenuje služby s automatickým startem. Start typ se čte
/// z registru (levné) — SCM by vyžadoval otevřít každou zvlášť.
pub fn auto_services() -> Result<Vec<AutoService>, Error> {
    let mut out = Vec::new();
    // SAFETY: SCM handle se vždy zavírá; dvoufázové čtení bufferu.
    unsafe {
        let scm = OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        )
        .map_err(|e| Error::Win32 {
            call: "OpenSCManagerW",
            code: e.code().0,
        })?;

        let mut needed = 0u32;
        let mut returned = 0u32;
        let mut resume = 0u32;
        // První volání zjistí velikost.
        let _ = EnumServicesStatusExW(
            scm,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            None,
            &mut needed,
            &mut returned,
            Some(&mut resume),
            PCWSTR::null(),
        );
        if needed == 0 {
            let _ = CloseServiceHandle(scm);
            return Ok(out);
        }
        let mut buf = vec![0u8; needed as usize + 1024];
        let ok = EnumServicesStatusExW(
            scm,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            Some(&mut buf),
            &mut needed,
            &mut returned,
            Some(&mut resume),
            PCWSTR::null(),
        );
        let _ = CloseServiceHandle(scm);
        ok.map_err(|e| Error::Win32 {
            call: "EnumServicesStatusExW",
            code: e.code().0,
        })?;

        let items = std::slice::from_raw_parts(
            buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
            returned as usize,
        );
        for it in items {
            let name = it.lpServiceName.to_string().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            // Start typ z registru: 2 = auto, 3 = demand, 4 = disabled.
            let key = format!(r"SYSTEM\CurrentControlSet\Services\{name}");
            let start =
                crate::registry::read_u64(crate::registry::HKEY_LOCAL_MACHINE, &key, "Start");
            let auto_start = start == Some(2);
            if !auto_start && start != Some(3) {
                continue; // disabled/boot/system drivery neukazujeme
            }
            out.push(AutoService {
                display_name: it
                    .lpDisplayName
                    .to_string()
                    .unwrap_or_else(|_| name.clone()),
                running: it.ServiceStatusProcess.dwCurrentState.0 == 4,
                auto_start,
                name,
            });
        }
    }
    out.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    Ok(out)
}

/// Přepne start typ služby: auto ↔ ruční (SPEC 7). Nikdy nemaže.
/// Vyžaduje admin — služba běží jako SYSTEM.
pub fn set_service_auto_start(name: &str, auto: bool) -> Result<(), Error> {
    // SAFETY: handly se vždy zavírají; měníme jen dwStartType.
    unsafe {
        let scm =
            OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT).map_err(|e| {
                Error::Win32 {
                    call: "OpenSCManagerW(change)",
                    code: e.code().0,
                }
            })?;
        let svc = OpenServiceW(
            scm,
            &HSTRING::from(name),
            SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG,
        );
        let svc = match svc {
            Ok(s) => s,
            Err(e) => {
                let _ = CloseServiceHandle(scm);
                return Err(Error::Win32 {
                    call: "OpenServiceW",
                    code: e.code().0,
                });
            }
        };
        let start_type = if auto {
            SERVICE_AUTO_START
        } else {
            SERVICE_DEMAND_START
        };
        let r = ChangeServiceConfigW(
            svc,
            windows::Win32::System::Services::ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            start_type,
            windows::Win32::System::Services::SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        );
        let _ = CloseServiceHandle(svc);
        let _ = CloseServiceHandle(scm);
        r.map_err(|e| Error::Win32 {
            call: "ChangeServiceConfigW",
            code: e.code().0,
        })
    }
}

#[allow(dead_code)]
fn _unused(_: *const c_void) {}
