//! GPU využití přes NVML (SPEC kap. 15.2) — oficiální NVIDIA API,
//! mluví s už nainstalovaným ovladačem, žádný vlastní kernel driver.
//! nvml.dll se načítá dynamicky: na strojích bez NVIDIA prostě není
//! a GPU metrika je poctivě nedostupná (None), nikdy vymyšlená.
//! AMD (ADLX) a Intel (IGCL) přijdou ve v3 se senzory.

use std::ffi::c_void;

use windows::core::{s, PCSTR};
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

/// nvmlUtilization_t.
#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

type FnInit = unsafe extern "C" fn() -> i32;
type FnDeviceByIndex = unsafe extern "C" fn(u32, *mut *mut c_void) -> i32;
type FnUtilization = unsafe extern "C" fn(*mut c_void, *mut NvmlUtilization) -> i32;

/// Inicializovaný NVML kontext: knihovna + handle prvního GPU.
pub struct Nvml {
    lib: windows::Win32::Foundation::HMODULE,
    device: *mut c_void,
    get_utilization: FnUtilization,
}

// SAFETY: NVML je vláknově bezpečné (dokumentované), handle je opaque
// ukazatel bez vazby na vlákno; používá se z jediného sampler vlákna.
unsafe impl Send for Nvml {}

impl Nvml {
    /// Zkusí načíst NVML a získat první GPU. None = NVIDIA není.
    pub fn init() -> Option<Nvml> {
        // SAFETY: standardní dynamické načtení; symboly ověřujeme.
        unsafe {
            let lib = LoadLibraryA(s!("nvml.dll")).ok()?;
            let init: FnInit = std::mem::transmute(load(lib, s!("nvmlInit_v2"))?);
            let by_index: FnDeviceByIndex =
                std::mem::transmute(load(lib, s!("nvmlDeviceGetHandleByIndex_v2"))?);
            let get_utilization: FnUtilization =
                std::mem::transmute(load(lib, s!("nvmlDeviceGetUtilizationRates"))?);

            if init() != 0 {
                let _ = FreeLibrary(lib);
                return None;
            }
            let mut device: *mut c_void = std::ptr::null_mut();
            if by_index(0, &mut device) != 0 || device.is_null() {
                let _ = FreeLibrary(lib);
                return None;
            }
            Some(Nvml {
                lib,
                device,
                get_utilization,
            })
        }
    }

    /// Aktuální využití GPU v % (0–100). None při chybě čtení.
    pub fn utilization_pct(&self) -> Option<f32> {
        let mut util = NvmlUtilization { gpu: 0, memory: 0 };
        // SAFETY: device i výstupní struktura jsou platné po dobu volání.
        let rc = unsafe { (self.get_utilization)(self.device, &mut util) };
        (rc == 0).then_some(util.gpu.min(100) as f32)
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: knihovnu jsme načetli my; shutdown symbol je volitelný.
        unsafe {
            if let Some(shutdown) = load(self.lib, s!("nvmlShutdown")) {
                let f: FnInit = std::mem::transmute(shutdown);
                let _ = f();
            }
            let _ = FreeLibrary(self.lib);
        }
    }
}

/// GetProcAddress helper.
unsafe fn load(
    lib: windows::Win32::Foundation::HMODULE,
    name: PCSTR,
) -> Option<unsafe extern "system" fn() -> isize> {
    GetProcAddress(lib, name)
}
