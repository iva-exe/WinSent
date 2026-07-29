//! Per-PID informace pro identitu: cesta k image, MSIX package family
//! a ochranná třída procesu (SPEC kap. 4.1, 4.3).
//!
//! Vše přes PROCESS_QUERY_LIMITED_INFORMATION — funguje i na chráněné
//! procesy. Volá se JEN pro nově viděné procesy (cache drží identity
//! engine), ne v každém ticku.

use std::ffi::c_void;

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Ukončí proces (v7, SPEC 17.5 — T1 akce). Volá se VÝHRADNĚ
/// z exekutoru po `Verdict::Allow`; sama žádnou kontrolu nedělá.
/// Handle se otevírá s minimálním právem PROCESS_TERMINATE.
pub fn terminate(pid: u32) -> Result<(), crate::Error> {
    use windows::Win32::System::Threading::{TerminateProcess, PROCESS_TERMINATE};
    // SAFETY: handle se vždy zavírá; exit kód 1 = ukončeno zvenčí.
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, false, pid).map_err(|e| crate::Error::Win32 {
            call: "OpenProcess(TERMINATE)",
            code: e.code().0,
        })?;
        let r = TerminateProcess(h, 1);
        let _ = CloseHandle(h);
        r.map_err(|e| crate::Error::Win32 {
            call: "TerminateProcess",
            code: e.code().0,
        })
    }
}

// NtQueryInformationProcess — nedokumentované třídy pro ochranné třídy.
#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        handle: HANDLE,
        class: u32,
        info: *mut c_void,
        len: u32,
        ret_len: *mut u32,
    ) -> i32;
}

const PROCESS_BREAK_ON_TERMINATION: u32 = 29;
const PROCESS_BASIC_INFORMATION_CLASS: u32 = 0;

/// PROCESS_EXTENDED_BASIC_INFORMATION (výřez: hlavička + flags).
#[repr(C)]
#[derive(Default)]
struct ExtendedBasicInfo {
    size: usize,
    // PROCESS_BASIC_INFORMATION tělo (6 × pointer-sized)
    basic: [usize; 6],
    flags: u32,
}

/// Ochranná třída procesu (SPEC kap. 4.3) — zatím jen pro zobrazení.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    /// Kill způsobí BSOD. Zakázáno, šedě + zámek.
    Critical,
    /// PPL — kill technicky nemožný.
    Protected,
    /// Běží pod SYSTEM/SERVICE účtem — kill jen za potvrzením.
    System,
    /// Běžný uživatelský proces.
    User,
}

/// Pojistka pro případ, kdy dotaz na kritičnost selže (SPEC 4.3:
/// nespoléhat jen na seznam, ale mít ho jako fallback).
const CRITICAL_NAMES: &[&str] = &[
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "smss.exe",
    "services.exe",
    "System",
    "Registry",
    "Memory Compression",
];

/// Otevře proces pro čtení metadat. None = nedostupný (zanikl, práva).
fn open(pid: u32) -> Option<OwnedHandle> {
    // SAFETY: handle vlastníme a zavíráme v Drop.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    Some(OwnedHandle(h))
}

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: handle pochází z OpenProcess.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Plná cesta k image procesu (Win32 formát).
pub fn image_path(pid: u32) -> Option<String> {
    let h = open(pid)?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: buffer s pevnou kapacitou, délku vrací API.
    unsafe {
        QueryFullProcessImageNameW(
            h.0,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()?;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// PackageFamilyName pro MSIX/UWP procesy; None = nebalený proces.
pub fn package_family(pid: u32) -> Option<String> {
    let h = open(pid)?;
    let mut len = 0u32;
    // První volání zjistí délku (APPMODEL_ERROR_NO_PACKAGE → None).
    // SAFETY: dvoufázový vzor dle kontraktu API.
    unsafe {
        let _ = GetPackageFamilyName(h.0, &mut len, None);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        if GetPackageFamilyName(h.0, &mut len, Some(PWSTR(buf.as_mut_ptr()))).is_err() {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]);
        (!s.is_empty()).then_some(s)
    }
}

/// Určí ochrannou třídu procesu. Čerstvé dotazy na OS, s fallbackem na
/// seznam jmen, když handle nejde otevřít.
pub fn protection(pid: u32, name: &str) -> Protection {
    if pid == 0 || pid == 4 {
        return Protection::Critical;
    }
    let Some(h) = open(pid) else {
        // Nedostupný proces: pojistka podle jména.
        if CRITICAL_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            return Protection::Critical;
        }
        return Protection::System;
    };

    // SAFETY: výstupy jsou lokální buffery správných velikostí.
    unsafe {
        // Critical: BreakOnTermination.
        let mut brk = 0u32;
        let mut ret = 0u32;
        if NtQueryInformationProcess(
            h.0,
            PROCESS_BREAK_ON_TERMINATION,
            &mut brk as *mut _ as *mut c_void,
            std::mem::size_of::<u32>() as u32,
            &mut ret,
        ) == 0
            && brk != 0
        {
            return Protection::Critical;
        }
        if CRITICAL_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            return Protection::Critical;
        }

        // Protected (PPL): flags bit 1 v extended basic info.
        let mut ext = ExtendedBasicInfo {
            size: std::mem::size_of::<ExtendedBasicInfo>(),
            ..Default::default()
        };
        if NtQueryInformationProcess(
            h.0,
            PROCESS_BASIC_INFORMATION_CLASS,
            &mut ext as *mut _ as *mut c_void,
            std::mem::size_of::<ExtendedBasicInfo>() as u32,
            &mut ret,
        ) == 0
            && ext.flags & 0x2 != 0
        {
            return Protection::Protected;
        }

        // System: token patří SYSTEM / LOCAL SERVICE / NETWORK SERVICE.
        if is_service_token(h.0) {
            return Protection::System;
        }
    }
    Protection::User
}

/// Je vlastníkem tokenu SYSTEM/LocalService/NetworkService?
fn is_service_token(process: HANDLE) -> bool {
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    // SAFETY: handle tokenu i buffery vlastníme a uvolňujeme.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr() as *mut _),
            len,
            &mut len,
        );
        let _ = CloseHandle(token);
        if ok.is_err() {
            return false;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = windows::core::PWSTR::null();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid_str).is_err() {
            return false;
        }
        let sid = sid_str.to_string().unwrap_or_default();
        let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            sid_str.0 as _,
        )));
        matches!(sid.as_str(), "S-1-5-18" | "S-1-5-19" | "S-1-5-20")
    }
}
