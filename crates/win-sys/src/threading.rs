//! Priority vláken — retenční smyčka a flusher běží na BELOW_NORMAL
//! (SPEC kap. 3.4, 8), aby údržba nikdy nekonkurovala sběru ani systému.

use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};

use crate::Error;

/// Sníží prioritu aktuálního vlákna na BELOW_NORMAL.
pub fn set_current_thread_below_normal() -> Result<(), Error> {
    // SAFETY: GetCurrentThread vrací pseudo-handle, který se nezavírá.
    let ok = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
    ok.map_err(|e| Error::Win32 {
        call: "SetThreadPriority",
        code: e.code().0,
    })
}
