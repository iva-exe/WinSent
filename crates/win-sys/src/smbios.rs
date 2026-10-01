//! Firmware tabulka SMBIOS (GetSystemFirmwareTable 'RSMB'): RAM moduly
//! (Type 17), základní deska (Type 2), BIOS/UEFI (Type 0) a stroj
//! (Type 1). Bez WMI — čte se jednou při startu, je to statická data.

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
    /// Typ paměti („DDR4"); prázdný, když ho deska nehlásí.
    pub mem_type: String,
    /// Skutečný takt paměti v MHz. U DDR je to polovina MT/s — dva
    /// přenosy na jeden takt. 0 = neznámo.
    pub clock_mhz: u32,
    /// Deska do pole rychlosti zapsala takt v MHz a `configured_mts`
    /// je z něj přepočtené. Nese se dál kvůli vysvětlení, proč se
    /// číslo liší od toho, co ukazují jiné nástroje.
    pub configured_was_clock: bool,
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
    // RawSMBIOSData hlavička: 8 bajtů, pak samotná tabulka. Bajty 1 a 2
    // jsou verze SMBIOS — podle ní se čte jednotka rychlosti paměti.
    if table.len() < 8 {
        return (Vec::new(), 0);
    }
    parse_type17(&table[8..], (table[1], table[2]))
}

/// Typ paměti z pole Memory Type (Type 17, offset 0x12) a meze rychlosti
/// generace v MT/s: (název, nejnižší, nejvyšší). `None` = paměť, která
/// není DDR, nebo typ, který deska nehlásí — u té takt a přenosy splývají.
///
/// Spodní mez je nejnižší rychlost, kterou generace podle JEDEC vůbec
/// má; horní je s rezervou nad nejrychlejšími přetaktovanými sadami.
/// Obě jsou schválně volné: slouží jen k poznání čísla, které jako MT/s
/// (nebo jako takt) nemůže existovat, ne k odhadu.
fn generace_ddr(kod: u8) -> Option<(&'static str, u32, u32)> {
    Some(match kod {
        0x12 => ("DDR", 200, 600),
        0x13 => ("DDR2", 400, 1300),
        0x14 => ("DDR2 FB-DIMM", 400, 1300),
        0x18 => ("DDR3", 800, 3300),
        0x1A => ("DDR4", 1600, 6000),
        0x1B => ("LPDDR", 200, 600),
        0x1C => ("LPDDR2", 200, 1300),
        0x1D => ("LPDDR3", 800, 2600),
        0x1E => ("LPDDR4", 1066, 5000),
        0x22 => ("DDR5", 3200, 12000),
        0x23 => ("LPDDR5", 1600, 11000),
        _ => return None,
    })
}

/// Rychlosti modulu tak, jak se mají ukázat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rychlosti {
    speed_mts: u32,
    configured_mts: u32,
    clock_mhz: u32,
    configured_was_clock: bool,
}

/// Přepočte pole rychlosti z Type 17 na MT/s.
///
/// Pole 0x20 („na čem modul běží") mělo ve specifikaci SMBIOS až do
/// verze 3.0 jednotku MHz a jmenovalo se Configured Memory Clock Speed;
/// na MT/s ho přejmenovala až verze 3.1. Desky se starší tabulkou (tenhle
/// stroj hlásí 2.8) tam proto píšou TAKT — u DDR4-3200 tedy 1600. Kdo to
/// číslo čte jako MT/s, ukáže poloviční rychlost a doporučí zapnout XMP,
/// které přitom zapnuté je. Přesně to se tu stalo.
///
/// Pole 0x15 („co modul umí") měla stará verze taky v MHz, jenže desky
/// do něj v praxi zapisují štítkovou rychlost v MT/s (tenhle stroj: 3200).
/// Pravidlo podle verze by ho proto zdvojnásobilo omylem; převádí se jen
/// tehdy, když je číslo pro danou generaci jako MT/s nemožné.
fn rychlosti(verze: (u8, u8), kod_typu: u8, speed_raw: u16, configured_raw: u16) -> Rychlosti {
    let speed_raw = speed_raw as u32;
    let configured_raw = configured_raw as u32;
    let Some((_, min, max)) = generace_ddr(kod_typu) else {
        // Typ, který deska nehlásí (nebo ne-DDR): čísla se nechají, jak
        // jsou, a takt se netvrdí — bez typu nejde říct, kolik přenosů
        // na takt paměť dělá.
        return Rychlosti {
            speed_mts: speed_raw,
            configured_mts: configured_raw,
            clock_mhz: 0,
            configured_was_clock: false,
        };
    };

    // Je hodnota takt (MHz), nebo už přenosy (MT/s)?
    //   · pod nejnižší rychlostí generace → jako MT/s neexistuje, je to takt;
    //   · dvojnásobek nad horní mezí → jako takt neexistuje, už jsou to MT/s;
    //   · mezi tím rozhodne verze tabulky, tedy co v ní podle specifikace být má.
    let je_takt = |v: u32, podle_verze: bool| -> bool {
        if v == 0 {
            false
        } else if v < min {
            true
        } else if v * 2 > max {
            false
        } else {
            podle_verze && verze < (3, 1)
        }
    };

    let configured_was_clock = je_takt(configured_raw, true);
    let configured_mts = if configured_was_clock { configured_raw * 2 } else { configured_raw };
    let speed_mts = if je_takt(speed_raw, false) { speed_raw * 2 } else { speed_raw };
    Rychlosti {
        speed_mts,
        configured_mts,
        clock_mhz: configured_mts / 2,
        configured_was_clock,
    }
}

/// Průchod SMBIOS strukturami: hlavička (type, length, handle) +
/// formátovaná část + string-set ukončený dvojitou nulou.
fn parse_type17(data: &[u8], verze: (u8, u8)) -> (Vec<RamModule>, u32) {
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
                let kod_typu = body.get(0x12).copied().unwrap_or(0);
                let r = rychlosti(verze, kod_typu, get_u16(body, 0x15), get_u16(body, 0x20));
                modules.push(RamModule {
                    size_mb,
                    speed_mts: r.speed_mts,
                    configured_mts: r.configured_mts,
                    slot,
                    manufacturer: get_string(strings, body.get(0x17).copied().unwrap_or(0)),
                    part_number: get_string(strings, body.get(0x1A).copied().unwrap_or(0)),
                    mem_type: generace_ddr(kod_typu).map(|g| g.0.to_string()).unwrap_or_default(),
                    clock_mhz: r.clock_mhz,
                    configured_was_clock: r.configured_was_clock,
                });
            }
        }
        off = strings_end + 2;
    }
    (modules, slots)
}

/// Základní deska, firmware a stroj — co je v SMBIOS čitelné.
/// Prázdný řetězec znamená „deska to nehlásí“, ne „nezjištěno“ —
/// nic se nedopočítává ani neodhaduje.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Board {
    pub manufacturer: String,
    pub product: String,
    pub version: String,
    pub serial: String,
    /// BIOS/UEFI (Type 0).
    pub bios_vendor: String,
    pub bios_version: String,
    pub bios_date: String,
    /// Stroj (Type 1) — u notebooků obvykle model, u sestav bývá prázdné.
    pub system_manufacturer: String,
    pub system_product: String,
}

/// Přečte desku + BIOS + stroj jedním průchodem tabulkou.
pub fn board() -> Board {
    let Some(table) = raw_table() else {
        return Board::default();
    };
    parse_board(&table[8..])
}

/// Syrová SMBIOS tabulka i s 8bajtovou hlavičkou RawSMBIOSData.
fn raw_table() -> Option<Vec<u8>> {
    // 'RSMB' big-endian signature dle dokumentace.
    let provider = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    // SAFETY: dvoufázové čtení tabulky dle kontraktu API.
    let table = unsafe {
        let len = GetSystemFirmwareTable(provider, 0, None);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let got = GetSystemFirmwareTable(provider, 0, Some(&mut buf));
        buf.truncate(got as usize);
        buf
    };
    (table.len() > 8).then_some(table)
}

fn parse_board(data: &[u8]) -> Board {
    let mut out = Board::default();
    for (stype, body, strings) in structures(data) {
        match stype {
            // Type 0 — BIOS Information.
            0 => {
                out.bios_vendor = get_string(strings, at(body, 0x04));
                out.bios_version = get_string(strings, at(body, 0x05));
                out.bios_date = get_string(strings, at(body, 0x08));
            }
            // Type 1 — System Information.
            1 => {
                out.system_manufacturer = get_string(strings, at(body, 0x04));
                out.system_product = get_string(strings, at(body, 0x05));
            }
            // Type 2 — Baseboard. Bereme první; další bývají riser karty.
            2 if out.product.is_empty() => {
                out.manufacturer = get_string(strings, at(body, 0x04));
                out.product = get_string(strings, at(body, 0x05));
                out.version = get_string(strings, at(body, 0x06));
                out.serial = get_string(strings, at(body, 0x07));
            }
            _ => {}
        }
    }
    out
}

/// Průchod SMBIOS strukturami — vrací (typ, formátovaná část, stringy).
/// Sdílí ho parsování všech typů, ať se logika hlaviček píše jednou.
fn structures(data: &[u8]) -> Vec<(u8, &[u8], &[u8])> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 4 <= data.len() {
        let stype = data[off];
        let length = data[off + 1] as usize;
        if length < 4 || off + length > data.len() {
            break;
        }
        // Konec string-setu: dvojitá nula za formátovanou částí.
        let mut end = off + length;
        while end + 1 < data.len() && !(data[end] == 0 && data[end + 1] == 0) {
            end += 1;
        }
        if stype == 127 {
            break; // End-of-table
        }
        out.push((stype, &data[off..off + length], &data[off + length..end]));
        off = end + 2;
    }
    out
}

/// Index stringu na dané pozici formátované části (0 = není).
fn at(body: &[u8], off: usize) -> u8 {
    body.get(off).copied().unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    const DDR4: u8 = 0x1A;
    const DDR5: u8 = 0x22;

    #[test]
    fn stara_tabulka_hlasi_takt_a_ten_se_prepocte() {
        // Skutečná data z tohohle stroje: SMBIOS 2.8, Kingston
        // KHX3200C16D4 se zapnutým XMP. Pole 0x15 = 3200, pole 0x20 = 1600.
        let r = rychlosti((2, 8), DDR4, 3200, 1600);
        assert_eq!(r.configured_mts, 3200);
        assert_eq!(r.speed_mts, 3200);
        assert_eq!(r.clock_mhz, 1600);
        assert!(r.configured_was_clock);
    }

    #[test]
    fn stara_tabulka_se_stitkem_jedec_pod_xmp() {
        // Častý tvar: „umí" je výchozí JEDEC rychlost modulu (2133),
        // běží se na XMP 3200, takt 1600. Podle poměru by to nešlo
        // poznat; podle verze tabulky ano.
        let r = rychlosti((2, 8), DDR4, 2133, 1600);
        assert_eq!(r.configured_mts, 3200);
        assert_eq!(r.speed_mts, 2133);
    }

    #[test]
    fn nova_tabulka_uz_ma_mt_s() {
        // Od SMBIOS 3.1 je pole v MT/s a nesmí se zdvojovat.
        let r = rychlosti((3, 3), DDR4, 3200, 3200);
        assert_eq!(r.configured_mts, 3200);
        assert_eq!(r.clock_mhz, 1600);
        assert!(!r.configured_was_clock);
        // Ani když je nastavená rychlost poloviční — tam to může být
        // opravdu vypnuté XMP a z dat se nedá říct opak.
        let r = rychlosti((3, 2), DDR4, 4800, 2400);
        assert_eq!(r.configured_mts, 2400);
    }

    #[test]
    fn nemozne_mt_s_je_takt_i_v_nove_tabulce() {
        // DDR4 pod 1600 MT/s neexistuje — 1200 je takt DDR4-2400,
        // ať tabulka tvrdí cokoliv.
        let r = rychlosti((3, 2), DDR4, 2400, 1200);
        assert_eq!(r.configured_mts, 2400);
        assert!(r.configured_was_clock);
    }

    #[test]
    fn nemozny_takt_se_nezdvojuje_ani_ve_stare_tabulce() {
        // Stará tabulka, ale deska už píše MT/s: 3200 jako takt by byla
        // DDR4-6400, a ta neexistuje.
        let r = rychlosti((2, 8), DDR4, 3200, 3200);
        assert_eq!(r.configured_mts, 3200);
        assert!(!r.configured_was_clock);
    }

    #[test]
    fn stitek_v_mhz_pod_minimem_generace() {
        // Deska, která i do „umí" píše takt: 1200 u DDR4 je takt.
        let r = rychlosti((2, 8), DDR4, 1200, 1200);
        assert_eq!(r.speed_mts, 2400);
        assert_eq!(r.configured_mts, 2400);
    }

    #[test]
    fn ddr5_v_nove_tabulce() {
        let r = rychlosti((3, 5), DDR5, 6000, 6000);
        assert_eq!(r.configured_mts, 6000);
        assert_eq!(r.clock_mhz, 3000);
    }

    #[test]
    fn neznamy_typ_se_nehada() {
        let r = rychlosti((2, 8), 0x02, 3200, 1600);
        assert_eq!(r.configured_mts, 1600);
        assert_eq!(r.clock_mhz, 0);
        assert!(!r.configured_was_clock);
    }

    #[test]
    fn nula_zustava_nulou() {
        let r = rychlosti((2, 8), DDR4, 0, 0);
        assert_eq!((r.speed_mts, r.configured_mts, r.clock_mhz), (0, 0, 0));
    }
}
