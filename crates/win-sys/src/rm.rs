//! Restart Manager (SPEC kap. 18.1): kdo drží soubor.
//!
//! Oficiální mechanismus Windows — používá ho i Windows Update.
//! `RmStartSession` → `RmRegisterResources` → `RmGetList` vrátí
//! držitele VČETNĚ klasifikace (`RM_APP_TYPE`), takže rovnou víme,
//! jestli je to kritický systémový proces, služba, nebo běžná
//! aplikace s oknem. Čistě čtecí — nic neukončuje.

use windows::core::PCWSTR;
use windows::Win32::System::RestartManager::{
    RmEndSession, RmGetList, RmRegisterResources, RmStartSession, RM_PROCESS_INFO,
};

/// Jak Restart Manager držitele klasifikuje.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderKind {
    /// Kritický systémový proces — akce se musí zamknout.
    Critical,
    /// Windows služba.
    Service,
    /// Aplikace s oknem (jde ukončit korektně).
    Window,
    /// Konzolová aplikace.
    Console,
    /// Explorer — zvláštní zacházení, nikdy neukončovat naslepo.
    Explorer,
    Unknown,
}

impl HolderKind {
    fn from_raw(t: u32) -> HolderKind {
        match t {
            1 => HolderKind::Window,   // RmMainWindow
            2 => HolderKind::Window,   // RmOtherWindow
            3 => HolderKind::Service,  // RmService
            4 => HolderKind::Explorer, // RmExplorer
            5 => HolderKind::Console,  // RmConsole
            1000 => HolderKind::Critical,
            _ => HolderKind::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HolderKind::Critical => "critical",
            HolderKind::Service => "service",
            HolderKind::Window => "window",
            HolderKind::Console => "console",
            HolderKind::Explorer => "explorer",
            HolderKind::Unknown => "unknown",
        }
    }
}

/// Jeden proces, který soubor drží.
#[derive(Debug, Clone)]
pub struct Holder {
    pub pid: u32,
    /// Jméno aplikace, jak ho hlásí Restart Manager.
    pub name: String,
    pub kind: HolderKind,
    /// Název služby (jen když kind == Service).
    pub service: Option<String>,
}

/// Kdo drží dané soubory. Prázdný seznam = nikdo (nebo RM nemá co říct).
pub fn holders(paths: &[String]) -> Result<Vec<Holder>, crate::Error> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut session: u32 = 0;
    let mut key = [0u16; 256]; // CCH_RM_SESSION_KEY+1
                               // SAFETY: session se vždy uzavře; buffery mají velikosti dle API.
    unsafe {
        let rc = RmStartSession(&mut session, None, windows::core::PWSTR(key.as_mut_ptr()));
        if rc.0 != 0 {
            return Err(crate::Error::Win32 {
                call: "RmStartSession",
                code: rc.0 as i32,
            });
        }

        // Cesty musí žít po celou dobu volání (PCWSTR na ně ukazuje).
        let wide: Vec<Vec<u16>> = paths
            .iter()
            .map(|p| p.encode_utf16().chain(std::iter::once(0)).collect())
            .collect();
        let ptrs: Vec<PCWSTR> = wide.iter().map(|w| PCWSTR(w.as_ptr())).collect();
        let rc = RmRegisterResources(session, Some(&ptrs), None, None);
        if rc.0 != 0 {
            let _ = RmEndSession(session);
            return Err(crate::Error::Win32 {
                call: "RmRegisterResources",
                code: rc.0 as i32,
            });
        }

        // Dvoufázově: první volání řekne, kolik procesů drží.
        let mut needed = 0u32;
        let mut count = 0u32;
        let mut reason = 0u32; // RmRebootReasonNone
        let rc = RmGetList(session, &mut needed, &mut count, None, &mut reason);
        // 234 = ERROR_MORE_DATA (očekávané), 0 = nikdo nedrží.
        if rc.0 != 0 && rc.0 != 234 {
            let _ = RmEndSession(session);
            return Err(crate::Error::Win32 {
                call: "RmGetList",
                code: rc.0 as i32,
            });
        }
        if needed == 0 {
            let _ = RmEndSession(session);
            return Ok(Vec::new());
        }

        let mut infos = vec![RM_PROCESS_INFO::default(); needed as usize];
        count = needed;
        let rc = RmGetList(
            session,
            &mut needed,
            &mut count,
            Some(infos.as_mut_ptr()),
            &mut reason,
        );
        let _ = RmEndSession(session);
        if rc.0 != 0 {
            return Err(crate::Error::Win32 {
                call: "RmGetList(2)",
                code: rc.0 as i32,
            });
        }

        let read = |b: &[u16]| {
            let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
            String::from_utf16_lossy(&b[..end])
        };
        Ok(infos
            .iter()
            .take(count as usize)
            .map(|i| {
                let kind = HolderKind::from_raw(i.ApplicationType.0 as u32);
                let service = read(&i.strServiceShortName);
                Holder {
                    pid: i.Process.dwProcessId,
                    name: read(&i.strAppName),
                    kind,
                    service: (!service.is_empty()).then_some(service),
                }
            })
            .collect())
    }
}
