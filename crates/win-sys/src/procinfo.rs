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

/// Ukončí proces, ale JEN tu instanci, která vznikla v `create_time`
/// (FILETIME ze snapshotu). `Ok(false)` = pod tím PID teď běží něco
/// jiného (nebo už nic) a nic se neukončilo.
///
/// Čas vzniku se čte z TÉHOŽ handle, kterým se pak ukončuje. Kontrola
/// snapshotem a teprve potom `terminate(pid)` nechává okno, ve kterém
/// původní proces skončí a jeho PID dostane jiný — a padl by ten cizí.
/// Otevřený handle drží objekt procesu, takže mezi kontrolou
/// a ukončením se PID recyklovat nemůže.
pub fn terminate_instance(pid: u32, create_time: i64) -> Result<bool, crate::Error> {
    use windows::Win32::Foundation::{E_INVALIDARG, FILETIME};
    use windows::Win32::System::Threading::{GetProcessTimes, TerminateProcess, PROCESS_TERMINATE};
    // SAFETY: handle zavírá OwnedHandle; výstupní FILETIME jsou lokální.
    unsafe {
        let h = match OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        ) {
            Ok(h) => OwnedHandle(h),
            // Neplatný PID = proces mezitím skončil, není co ukončit.
            Err(e) if e.code() == E_INVALIDARG => return Ok(false),
            Err(e) => {
                return Err(crate::Error::Win32 {
                    call: "OpenProcess(TERMINATE)",
                    code: e.code().0,
                })
            }
        };
        let mut vznik = FILETIME::default();
        let (mut konec, mut jadro, mut uziv) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(h.0, &mut vznik, &mut konec, &mut jadro, &mut uziv).map_err(|e| {
            crate::Error::Win32 {
                call: "GetProcessTimes",
                code: e.code().0,
            }
        })?;
        let ct = ((vznik.dwHighDateTime as i64) << 32) | vznik.dwLowDateTime as i64;
        if ct != create_time {
            return Ok(false);
        }
        TerminateProcess(h.0, 1).map_err(|e| crate::Error::Win32 {
            call: "TerminateProcess",
            code: e.code().0,
        })?;
        Ok(true)
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
/// ProcessProtectionInformation → PS_PROTECTION (1 bajt: Type:3, Audit:1,
/// Signer:4). Přímá odpověď na „je to PP/PPL?“; extended basic info je
/// jen záloha pro případ, že by třída 61 selhala.
const PROCESS_PROTECTION_INFORMATION: u32 = 61;
/// PROCESS_EXTENDED_BASIC_INFORMATION.Flags: bit 0 = IsProtectedProcess.
/// Bit 1 (0x2) je IsWow64Process — dřív se testoval omylem ten, takže
/// každá 32bitová aplikace vyšla jako PPL a validace jí zakázala kill.
const EXT_FLAG_PROTECTED: u32 = 0x1;

/// Rozhodnutí o PPL z dostupných odpovědí OS. `ps_protection` = bajt
/// PS_PROTECTION, když se ho podařilo přečíst (má přednost),
/// `ext_flags` = Flags z extended basic info jako záloha.
fn is_protected(ps_protection: Option<u8>, ext_flags: Option<u32>) -> bool {
    match ps_protection {
        Some(level) => level != 0,
        None => ext_flags.is_some_and(|f| f & EXT_FLAG_PROTECTED != 0),
    }
}

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
    /// Kill shodí systém (BSOD) nebo přihlášenou relaci (obraz,
    /// přihlašování). Zakázáno, šedě + zámek.
    Critical,
    /// PPL — kill technicky nemožný.
    Protected,
    /// Běží pod SYSTEM/SERVICE účtem — kill jen za potvrzením.
    System,
    /// Běžný uživatelský proces.
    User,
}

/// Procesy, jejichž ukončení systém neustojí — nezávisle na tom, co
/// hlásí příznaky OS (BreakOnTermination ani PPL nejsou zapnuté všude:
/// lsass bez RunAsPPL je obyčejný proces SYSTEMu, a přesto je jeho
/// konec okamžitý restart). Patří sem i to, bez čeho se rozpadne
/// přihlášená relace: dwm (obraz), fontdrvhost (písma), LogonUI
/// a userinit (přihlášení).
///
/// JEDEN seznam pro zobrazení (`protection`) i pro validační vrstvu.
/// Dřív byly dva a rozešly se: UI u lsass nabízelo „Ukončit", protože
/// ho tenhle seznam neznal, a odmítla to až služba po potvrzení.
///
/// Malými písmeny; porovnává se bez ohledu na velikost. svchost.exe
/// tu záměrně není — kritická je jen ta instance, která to sama hlásí
/// (BreakOnTermination), ne všech šedesát.
pub const CRITICAL_NAMES: &[&str] = &[
    "system",
    "registry",
    "memory compression",
    "memcompression",
    "secure system",
    "idle",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "ntoskrnl.exe",
    "dwm.exe",
    "fontdrvhost.exe",
    "logonui.exe",
    "userinit.exe",
];

/// Je jméno procesu na tvrdém seznamu kritických?
pub fn is_critical_name(name: &str) -> bool {
    let n = name.trim();
    CRITICAL_NAMES.iter().any(|c| c.eq_ignore_ascii_case(n))
}

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
        if is_critical_name(name) {
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
        if is_critical_name(name) {
            return Protection::Critical;
        }

        // Protected (PP/PPL): primárně PS_PROTECTION, záložně bit 0
        // z extended basic info (ne bit 1 — to je WoW64).
        let mut level = 0u8;
        let ps = if NtQueryInformationProcess(
            h.0,
            PROCESS_PROTECTION_INFORMATION,
            &mut level as *mut _ as *mut c_void,
            1,
            &mut ret,
        ) == 0
        {
            Some(level)
        } else {
            None
        };
        let ext_flags = if ps.is_none() {
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
            {
                Some(ext.flags)
            } else {
                None
            }
        } else {
            None
        };
        if is_protected(ps, ext_flags) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wow64_bit_neni_ppl() {
        // Flags 0x2 (IsWow64Process) i 0xA (WoW64 + cross-session) dřív
        // vycházely jako Protected; ochranu znamená jen bit 0.
        assert!(!is_protected(None, Some(0x2)));
        assert!(!is_protected(None, Some(0xA)));
        assert!(is_protected(None, Some(0x1)));
        assert!(!is_protected(None, None));
        // PS_PROTECTION má přednost před záložními flagy.
        assert!(!is_protected(Some(0), Some(0x1)));
        assert!(is_protected(Some(0x31), None)); // PPL, signer Antimalware
    }

    #[test]
    fn kriticka_jmena_jeden_seznam() {
        for n in [
            "lsass.exe",
            "csrss.exe",
            "WinLogon.exe",
            "System",
            "Secure System",
            "Memory Compression",
            "dwm.exe",
            "fontdrvhost.exe",
            " LogonUI.exe ",
        ] {
            assert!(is_critical_name(n), "{n} musí být na tvrdém seznamu");
        }
        // svchost rozhoduje BreakOnTermination, ne jméno.
        for n in ["notepad.exe", "svchost.exe", "explorer.exe", ""] {
            assert!(!is_critical_name(n), "{n} na seznamu být nemá");
        }
    }

    // Jiná instance pod týmž PID se neukončí: vlastní dítě s vymyšleným
    // časem vzniku musí přežít, se skutečným časem skončit.
    #[test]
    fn terminate_instance_hlida_cas_vzniku() {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        use std::os::windows::process::CommandExt;
        let exe = std::path::Path::new(&windir).join(r"System32\PING.EXE");
        // CREATE_NO_WINDOW: ani probliknutí konzole na ploše.
        let Ok(mut child) = std::process::Command::new(&exe)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
        else {
            return;
        };
        let pid = child.id();
        let mut buf = Vec::new();
        let ct = crate::proc::snapshot_processes(&mut buf)
            .ok()
            .and_then(|ps| ps.iter().find(|p| p.pid == pid).map(|p| p.create_time));
        let Some(ct) = ct else {
            let _ = child.kill();
            let _ = child.wait();
            return;
        };
        let cizi = terminate_instance(pid, ct + 1);
        let prezil = child.try_wait().ok().flatten().is_none();
        let prava = terminate_instance(pid, ct);
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            matches!(cizi, Ok(false)),
            "cizí instance se nesmí ukončit: {cizi:?}"
        );
        assert!(prezil, "dítě s jiným časem vzniku mělo přežít");
        assert!(
            matches!(prava, Ok(true)),
            "pravá instance se má ukončit: {prava:?}"
        );
    }

    #[test]
    fn bezny_32bitovy_proces_neni_chraneny() {
        // Vlastní dítě testu z SysWOW64; na stroji bez WoW64 se přeskočí.
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let exe = std::path::Path::new(&windir).join(r"SysWOW64\cmd.exe");
        if !exe.exists() {
            return;
        }
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: test nesmí na ploše probliknout konzolí.
        let Ok(mut child) = std::process::Command::new(&exe)
            .args(["/c", "ping", "-n", "3", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
        else {
            return;
        };
        let p = protection(child.id(), "cmd.exe");
        let _ = child.kill();
        let _ = child.wait();
        assert_ne!(p, Protection::Protected);
    }
}
