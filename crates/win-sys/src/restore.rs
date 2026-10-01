//! Bod obnovení systému (SPEC 17.5, striktní režim): před NEVRATNOU
//! T1 akcí se volá `SRSetRestorePoint`. srclient.dll se načítá
//! dynamicky — System Restore bývá vypnutý a nesmí to shodit službu;
//! selhání hlásíme volajícímu (ten rozhodne, zda akci zastavit).

use std::ffi::c_void;

use windows::core::{s, w};
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

/// RESTOREPOINTINFOW (dwEventType, dwRestorePtType, llSequenceNumber,
/// szDescription[MAX_DESC_W = 256]).
///
/// SrRestorePtApi.h obě struktury deklaruje pod `#pragma pack(1)`, proto
/// `packed`. U STATEMGRSTATUS na tom záleží: s `repr(C)` leželo
/// llSequenceNumber na offsetu 8 místo 4, četl se jen horní dword čísla
/// (prakticky vždy 0) a END_SYSTEM_CHANGE pak uzavíral sekvenci 0 místo
/// bodu, který právě vznikl. Na pole packed struktur se nesmí brát
/// reference — čte se hodnotou.
#[repr(C, packed)]
struct RestorePointInfoW {
    event_type: u32,
    restore_pt_type: u32,
    sequence_number: i64,
    description: [u16; 256],
}

/// STATEMGRSTATUS (12 B: nStatus @0, llSequenceNumber @4).
#[repr(C, packed)]
struct StateMgrStatus {
    status: u32,
    sequence_number: i64,
}

type FnSetRestorePoint =
    unsafe extern "system" fn(*const RestorePointInfoW, *mut StateMgrStatus) -> i32;

/// Vytvoří bod obnovení. Err = SR nedostupné/selhalo (kód Win32).
pub fn create_restore_point(description: &str) -> Result<(), crate::Error> {
    // SAFETY: dynamické načtení + volání dle kontraktu API; knihovna
    // se vždy uvolní.
    unsafe {
        // Jen ze System32. Holé jméno by služba (SYSTEM) hledala i po
        // PATH, kam může zapisovat běžný uživatel — podvržená knihovna
        // by se načetla s nejvyššími právy (stejná díra jako u nvml.dll).
        let lib = LoadLibraryExW(w!("srclient.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
            .map_err(|e| crate::Error::Win32 {
            call: "LoadLibraryExW(srclient)",
            code: e.code().0,
        })?;
        let Some(f) = GetProcAddress(lib, s!("SRSetRestorePointW")) else {
            let _ = FreeLibrary(lib);
            return Err(crate::Error::Win32 {
                call: "GetProcAddress(SRSetRestorePointW)",
                code: -1,
            });
        };
        let f: FnSetRestorePoint =
            std::mem::transmute::<unsafe extern "system" fn() -> isize, FnSetRestorePoint>(f);

        let mut info = RestorePointInfoW {
            event_type: 100,     // BEGIN_SYSTEM_CHANGE
            restore_pt_type: 12, // MODIFY_SETTINGS
            sequence_number: 0,
            description: [0; 256],
        };
        // 255 znaků — poslední prvek zůstane nulovým terminátorem.
        let mut desc = [0u16; 256];
        for (i, u) in description.encode_utf16().take(255).enumerate() {
            desc[i] = u;
        }
        info.description = desc;
        let mut status = StateMgrStatus {
            status: 0,
            sequence_number: 0,
        };
        let ok = f(&info as *const _, &mut status);
        // Uzavření události (END_SYSTEM_CHANGE = 101) se sekvencí, kterou
        // vrátil BEGIN. Vlastní status, ať chybový kód BEGIN zůstane.
        if ok != 0 {
            let seq = status.sequence_number;
            let end = RestorePointInfoW {
                event_type: 101,
                restore_pt_type: 12,
                sequence_number: seq,
                description: [0; 256],
            };
            let mut end_status = StateMgrStatus {
                status: 0,
                sequence_number: 0,
            };
            // Bod vznikl už při BEGIN, akci kvůli tomu nezastavujeme —
            // ale tiše to zahodit nejde, dřív se tak skryla chyba ABI.
            if f(&end as *const _, &mut end_status) == 0 {
                let code = end_status.status;
                tracing::warn!(code, seq, "SRSetRestorePointW(END_SYSTEM_CHANGE) selhal");
            }
        }
        let _ = FreeLibrary(lib);
        if ok != 0 {
            Ok(())
        } else {
            Err(crate::Error::Win32 {
                call: "SRSetRestorePointW",
                code: status.status as i32,
            })
        }
    }
}

// Potlačení varování na nepoužitý c_void import při některých cílech.
#[allow(dead_code)]
fn _t(_: *const c_void) {}

#[cfg(test)]
mod tests {
    use super::*;

    // Rozložení musí sedět na `#pragma pack(1)` ze SrRestorePtApi.h.
    #[test]
    fn rozlozeni_jako_v_sdk() {
        assert_eq!(std::mem::size_of::<StateMgrStatus>(), 12);
        assert_eq!(std::mem::offset_of!(StateMgrStatus, sequence_number), 4);
        assert_eq!(std::mem::size_of::<RestorePointInfoW>(), 528);
        assert_eq!(std::mem::offset_of!(RestorePointInfoW, sequence_number), 8);
        assert_eq!(std::mem::offset_of!(RestorePointInfoW, description), 16);
    }
}
