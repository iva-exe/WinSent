//! Připojené obrazovky a jejich režim (v9, SPEC kap. 15.1).
//!
//! Proč tady a ne ve službě: `EnumDisplayDevicesW` odpovídá za relaci
//! volajícího. Služba běží jako SYSTEM v session 0, kde žádná plocha
//! není — dostala by prázdný seznam. UI proces běží v relaci uživatele
//! a vidí přesně ty obrazovky, na které se uživatel dívá.
//!
//! Stejný důvod jako u odinstalátoru (`uninstall.rs`): co je vázané na
//! relaci, dělá UI; co je vázané na systém, dělá služba.

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW,
    DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, ENUM_CURRENT_SETTINGS,
};

/// Připojené obrazovky s aktuálním režimem.
pub fn displays() -> Vec<core_types::proc::DisplayRow> {
    let mut out = Vec::new();
    // SAFETY: struktury mají vyplněné `cb`, index se posouvá dle
    // kontraktu EnumDisplayDevicesW a řetězce žijí přes celé volání.
    unsafe {
        let mut i = 0u32;
        loop {
            let mut adapter = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if !EnumDisplayDevicesW(PCWSTR::null(), i, &mut adapter, 0).as_bool() {
                break;
            }
            i += 1;
            // Adaptéry bez plochy jsou virtuální nebo odpojené.
            if (adapter.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP).0 == 0 {
                continue;
            }
            let dev_name = wide_arr(&adapter.DeviceName);
            let wname: Vec<u16> = dev_name.encode_utf16().chain(std::iter::once(0)).collect();

            let mut row = core_types::proc::DisplayRow {
                adapter: wide_arr(&adapter.DeviceString),
                primary: (adapter.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE).0 != 0,
                ..Default::default()
            };
            // Druhá úroveň enumerace = samotný monitor (jméno z EDID).
            let mut mon = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if EnumDisplayDevicesW(PCWSTR(wname.as_ptr()), 0, &mut mon, 0).as_bool() {
                row.monitor = wide_arr(&mon.DeviceString);
            }
            let mut mode = DEVMODEW {
                dmSize: std::mem::size_of::<DEVMODEW>() as u16,
                ..Default::default()
            };
            if EnumDisplaySettingsW(PCWSTR(wname.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode)
                .as_bool()
            {
                row.width = mode.dmPelsWidth;
                row.height = mode.dmPelsHeight;
                row.refresh_hz = mode.dmDisplayFrequency;
            }
            out.push(row);
        }
    }
    out
}

/// Řetězec z pevného UTF-16 pole ve struktuře.
fn wide_arr(arr: &[u16]) -> String {
    let end = arr.iter().position(|&c| c == 0).unwrap_or(arr.len());
    String::from_utf16_lossy(&arr[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test běží v relaci uživatele, takže obrazovku vidět musí —
    // a ta hlavní musí mít rozlišení.
    #[test]
    fn primary_display_has_mode() {
        let d = displays();
        assert!(!d.is_empty(), "žádná připojená obrazovka");
        let p = d.iter().find(|x| x.primary).unwrap_or(&d[0]);
        assert!(p.width > 0 && p.height > 0, "obrazovka bez rozlišení");
        assert!(p.refresh_hz > 0, "obrazovka bez obnovovací frekvence");
    }
}
