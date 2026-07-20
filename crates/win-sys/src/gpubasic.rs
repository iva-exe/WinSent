//! Základní identifikace GPU z registru — VENDOR-NEUTRÁLNÍ (SPEC 15.2,
//! stupeň „registry fallback"). Každý WDDM ovladač (NVIDIA, AMD, Intel,
//! Qualcomm) zapisuje do třídy displeje `{4d36e968-…}` DriverDesc a
//! HardwareInformation.qwMemorySize. Zjišťuje se jednou při startu.

use crate::registry::{enum_subkeys, read_string, read_u64, HKEY_LOCAL_MACHINE};

/// GUID třídy Display adapters.
const DISPLAY_CLASS: &str =
    r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

/// Základní údaje o GPU (bez běhových metrik).
#[derive(Debug, Clone, Default)]
pub struct BasicGpu {
    pub name: Option<String>,
    pub vram_total_mb: Option<u64>,
}

/// Najde adaptér s největší dedikovanou VRAM (= diskrétní GPU; při
/// remízě první v pořadí). Software adaptéry (0 B VRAM) prohrávají,
/// ale název dodá i stroj jen s iGPU/basic driverem.
pub fn detect() -> BasicGpu {
    let mut best = BasicGpu::default();
    let mut best_vram = 0u64;
    for sub in enum_subkeys(HKEY_LOCAL_MACHINE, DISPLAY_CLASS) {
        // Podklíče adaptérů jsou číselné („0000"…); „Properties" apod. ne.
        if !sub.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let key = format!("{DISPLAY_CLASS}\\{sub}");
        let Some(name) = read_string(HKEY_LOCAL_MACHINE, &key, "DriverDesc") else {
            continue;
        };
        let vram =
            read_u64(HKEY_LOCAL_MACHINE, &key, "HardwareInformation.qwMemorySize").unwrap_or(0);
        if best.name.is_none() || vram > best_vram {
            best_vram = vram;
            best = BasicGpu {
                name: Some(name),
                vram_total_mb: (vram > 0).then_some(vram / (1024 * 1024)),
            };
        }
    }
    best
}
