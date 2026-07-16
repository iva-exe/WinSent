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

/// nvmlMemory_t.
#[repr(C)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

type FnInit = unsafe extern "C" fn() -> i32;
type FnDeviceByIndex = unsafe extern "C" fn(u32, *mut *mut c_void) -> i32;
type FnUtilization = unsafe extern "C" fn(*mut c_void, *mut NvmlUtilization) -> i32;
type FnTemperature = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32;
type FnMemoryInfo = unsafe extern "C" fn(*mut c_void, *mut NvmlMemory) -> i32;
type FnPower = unsafe extern "C" fn(*mut c_void, *mut u32) -> i32;
type FnClock = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32;
type FnName = unsafe extern "C" fn(*mut c_void, *mut u8, u32) -> i32;

/// Doplňkové údaje GPU pro detail sekci.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuDetails {
    pub temp_c: Option<f32>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub power_w: Option<f32>,
    pub clock_mhz: Option<u32>,
}

/// Inicializovaný NVML kontext: knihovna + handle prvního GPU.
/// Doplňkové symboly jsou volitelné (starší ovladače).
pub struct Nvml {
    lib: windows::Win32::Foundation::HMODULE,
    device: *mut c_void,
    get_utilization: FnUtilization,
    get_temperature: Option<FnTemperature>,
    get_memory: Option<FnMemoryInfo>,
    get_power: Option<FnPower>,
    get_clock: Option<FnClock>,
    get_name: Option<FnName>,
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
                get_temperature: load(lib, s!("nvmlDeviceGetTemperature"))
                    .map(|f| std::mem::transmute::<_, FnTemperature>(f)),
                get_memory: load(lib, s!("nvmlDeviceGetMemoryInfo"))
                    .map(|f| std::mem::transmute::<_, FnMemoryInfo>(f)),
                get_power: load(lib, s!("nvmlDeviceGetPowerUsage"))
                    .map(|f| std::mem::transmute::<_, FnPower>(f)),
                get_clock: load(lib, s!("nvmlDeviceGetClockInfo"))
                    .map(|f| std::mem::transmute::<_, FnClock>(f)),
                get_name: load(lib, s!("nvmlDeviceGetName"))
                    .map(|f| std::mem::transmute::<_, FnName>(f)),
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

    /// Název GPU (např. "NVIDIA GeForce RTX 3070").
    pub fn name(&self) -> Option<String> {
        let f = self.get_name?;
        let mut buf = [0u8; 96];
        // SAFETY: buffer má velikost předanou API.
        let rc = unsafe { f(self.device, buf.as_mut_ptr(), buf.len() as u32) };
        if rc != 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).trim().to_string())
    }

    /// Doplňkové údaje (teplota, VRAM, spotřeba, takt) — co ovladač dá.
    pub fn details(&self) -> GpuDetails {
        // SAFETY: všechny výstupy jsou lokální, device platí.
        unsafe {
            let mut d = GpuDetails::default();
            if let Some(f) = self.get_temperature {
                let mut t = 0u32;
                // 0 = NVML_TEMPERATURE_GPU
                if f(self.device, 0, &mut t) == 0 {
                    d.temp_c = Some(t as f32);
                }
            }
            if let Some(f) = self.get_memory {
                let mut m = NvmlMemory {
                    total: 0,
                    free: 0,
                    used: 0,
                };
                if f(self.device, &mut m) == 0 {
                    d.vram_used_mb = Some(m.used / (1024 * 1024));
                    d.vram_total_mb = Some(m.total / (1024 * 1024));
                }
            }
            if let Some(f) = self.get_power {
                let mut mw = 0u32;
                if f(self.device, &mut mw) == 0 {
                    d.power_w = Some(mw as f32 / 1000.0);
                }
            }
            if let Some(f) = self.get_clock {
                let mut mhz = 0u32;
                // 0 = NVML_CLOCK_GRAPHICS
                if f(self.device, 0, &mut mhz) == 0 {
                    d.clock_mhz = Some(mhz);
                }
            }
            d
        }
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
