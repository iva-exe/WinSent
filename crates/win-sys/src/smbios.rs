//! RAM moduly ze SMBIOS (GetSystemFirmwareTable 'RSMB', struktura
//! Type 17 — Memory Device). Bez WMI, čte se jednou při startu.

use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, FIRMWARE_TABLE_PROVIDER};

/// Jeden osazený RAM modul.
#[derive(Debug, Clone, Default)]
pub struct RamModule {
    pub size_mb: u64,
    /// Maximální rychlost modulu (MT/s).
    pub speed_mts: u32,
    /// Nakonfigurovaná rychlost (MT/s) — na té reálně běží.
    pub configured_mts: u32,
    /// Slot (DeviceLocator, např. "DIMM_A1").
    pub slot: String,
    pub manufacturer: String,
    pub part_number: String,
}

/// Výsledek: (osazené moduly, celkový počet slotů).
pub fn ram_modules() -> (Vec<RamModule>, u32) {
    // 'RSMB' big-endian signature dle dokumentace.
    let provider = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    // SAFETY: dvoufázové čtení tabulky dle kontraktu API.
    let table = unsafe {
        let len = GetSystemFirmwareTable(provider, 0, None);
        if len == 0 {
            return (Vec::new(), 0);
        }
        let mut buf = vec![0u8; len as usize];
        let got = GetSystemFirmwareTable(provider, 0, Some(&mut buf));
        buf.truncate(got as usize);
        buf
    };
    // RawSMBIOSData hlavička: 8 bajtů, pak samotná tabulka.
    if table.len() < 8 {
        return (Vec::new(), 0);
    }
    parse_type17(&table[8..])
}

/// Průchod SMBIOS strukturami: hlavička (type, length, handle) +
/// formátovaná část + string-set ukončený dvojitou nulou.
fn parse_type17(data: &[u8]) -> (Vec<RamModule>, u32) {
    let mut modules = Vec::new();
    let mut slots = 0u32;
    let mut off = 0usize;

    while off + 4 <= data.len() {
        let stype = data[off];
        let length = data[off + 1] as usize;
        if length < 4 || off + length > data.len() {
            break;
        }
        let body = &data[off..off + length];

        // Konec string-setu: dvojitá nula za formátovanou částí.
        let mut strings_end = off + length;
        while strings_end + 1 < data.len()
            && !(data[strings_end] == 0 && data[strings_end + 1] == 0)
        {
            strings_end += 1;
        }
        let strings = &data[off + length..strings_end.min(data.len())];

        if stype == 127 {
            break; // End-of-table
        }
        if stype == 17 {
            slots += 1;
            let size_raw = u16::from_le_bytes([body[0x0C], body[0x0D]]);
            if size_raw != 0 {
                // 0x7FFF → skutečná velikost v Extended Size (u32 MB @0x1C).
                let size_mb = if size_raw == 0x7FFF && length >= 0x20 {
                    u32::from_le_bytes(body[0x1C..0x20].try_into().unwrap()) as u64
                } else if size_raw & 0x8000 != 0 {
                    (size_raw & 0x7FFF) as u64 / 1024 // jednotky kB
                } else {
                    size_raw as u64
                };
                // DeviceLocator; když je prázdný nebo generický, doplní
                // ho BankLocator (desky často hlásí obojí různě).
                let device = get_string(strings, body.get(0x10).copied().unwrap_or(0));
                let bank = get_string(strings, body.get(0x11).copied().unwrap_or(0));
                let slot = if device.is_empty() {
                    bank
                } else if !bank.is_empty() && bank != device {
                    format!("{device} ({bank})")
                } else {
                    device
                };
                modules.push(RamModule {
                    size_mb,
                    speed_mts: get_u16(body, 0x15) as u32,
                    configured_mts: get_u16(body, 0x20) as u32,
                    slot,
                    manufacturer: get_string(strings, body.get(0x17).copied().unwrap_or(0)),
                    part_number: get_string(strings, body.get(0x1A).copied().unwrap_or(0)),
                });
            }
        }
        off = strings_end + 2;
    }
    (modules, slots)
}

fn get_u16(body: &[u8], off: usize) -> u16 {
    if off + 2 <= body.len() {
        u16::from_le_bytes([body[off], body[off + 1]])
    } else {
        0
    }
}

/// N-tý string ze string-setu (1-based index dle SMBIOS).
fn get_string(strings: &[u8], index: u8) -> String {
    if index == 0 {
        return String::new();
    }
    strings
        .split(|&b| b == 0)
        .nth(index as usize - 1)
        .map(|s| String::from_utf8_lossy(s).trim().to_string())
        .unwrap_or_default()
}
