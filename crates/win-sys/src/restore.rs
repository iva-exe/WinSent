//! Bod obnovení systému (SPEC 17.5, striktní režim): před NEVRATNOU
//! T1 akcí se volá `SRSetRestorePoint`. srclient.dll se načítá
//! dynamicky — System Restore bývá vypnutý a nesmí to shodit službu;
//! selhání hlásíme volajícímu (ten rozhodne, zda akci zastavit).

use std::ffi::c_void;

use windows::core::s;
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

/// RESTOREPOINTINFOW (výřez dle sdk: dwEventType, dwRestorePtType,
/// llSequenceNumber, szDescription[257]).
#[repr(C)]
struct RestorePointInfoW {
    event_type: u32,
    restore_pt_type: u32,
    sequence_number: i64,
    description: [u16; 257],
}

/// STATEMGRSTATUS.
#[repr(C)]
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
        let lib = LoadLibraryA(s!("srclient.dll")).map_err(|e| crate::Error::Win32 {
            call: "LoadLibraryA(srclient)",
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
            description: [0; 257],
        };
        for (i, u) in description.encode_utf16().take(256).enumerate() {
            info.description[i] = u;
        }
        let mut status = StateMgrStatus {
            status: 0,
            sequence_number: 0,
        };
        let ok = f(&info as *const _, &mut status);
        // Uzavření události (END_SYSTEM_CHANGE = 101).
        if ok != 0 {
            let mut end = RestorePointInfoW {
                event_type: 101,
                restore_pt_type: 12,
                sequence_number: status.sequence_number,
                description: [0; 257],
            };
            end.description[0] = 0;
            let _ = f(&end as *const _, &mut status);
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
