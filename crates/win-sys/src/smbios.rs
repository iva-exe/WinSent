//! Firmware tabulka SMBIOS (GetSystemFirmwareTable 'RSMB'): RAM moduly
//! (Type 17), základní deska (Type 2), BIOS/UEFI (Type 0) a stroj
//! (Type 1). Bez WMI — čte se jednou při startu, je to statická data.

use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, FIRMWARE_TABLE_PROVIDER};

/// Jeden osazený RAM modul.
#[derive(Debug, Clone, Default)]
pub struct RamModule {
    /// 0 = deska velikost nehlásí (0xFFFF), nic se neodhaduje.
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
/// generace v MT/s. `None` = paměť, která není DDR, nebo typ, který deska
/// nehlásí — u té takt a přenosy splývají.
struct Generace {
    nazev: &'static str,
    /// Nejnižší rychlost, kterou generace podle JEDEC vůbec má. Číslo pod
    /// ní jako MT/s neexistuje, takže je to takt.
    min: u32,
    /// Nejnižší rychlost, na které se generace v praxi opravdu provozuje
    /// (DDR4-1600 a 1866 sice v JEDEC jsou, ale osazené je nikdo nemá;
    /// desky i procesory začínají na 2133). Pod ní je číslo ve staré
    /// tabulce mnohem spíš takt než skutečná rychlost.
    bezna_min: u32,
    /// S rezervou nad nejrychlejšími přetaktovanými sadami. Dvojnásobek
    /// nad ní jako takt neexistuje, takže jsou to už MT/s.
    max: u32,
}

fn generace_ddr(kod: u8) -> Option<Generace> {
    let (nazev, min, bezna_min, max) = match kod {
        0x12 => ("DDR", 200, 266, 600),
        0x13 => ("DDR2", 400, 533, 1300),
        0x14 => ("DDR2 FB-DIMM", 400, 533, 1300),
        0x18 => ("DDR3", 800, 1066, 3300),
        0x1A => ("DDR4", 1600, 2133, 6000),
        0x1B => ("LPDDR", 200, 266, 600),
        0x1C => ("LPDDR2", 200, 800, 1300),
        0x1D => ("LPDDR3", 800, 1333, 2600),
        0x1E => ("LPDDR4", 1066, 2133, 5000),
        0x22 => ("DDR5", 3200, 4000, 12000),
        0x23 => ("LPDDR5", 1600, 4266, 11000),
        _ => return None,
    };
    Some(Generace { nazev, min, bezna_min, max })
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
///
/// Jenže ani pole 0x20 nepíšou staré desky jednotně: jiné (OEM z éry
/// Skylake) do něj dávají MT/s, a slepé „stará verze = takt“ pak z
/// DDR4-2400 bez XMP udělalo 4800 MT/s s přesvědčivým, ale nepravdivým
/// vysvětlením. Ve staré tabulce se proto číslo bere jako takt jen tehdy,
/// když to jinak nedává smysl:
///   · je přesně polovinou štítku (tenhle stroj: 3200 → 1600), nebo
///   · je pod běžnou spodní rychlostí generace (DDR4 pod 2133) — jako
///     MT/s by to byla rychlost, na které DDR4 nikdo neprovozuje, kdežto
///     jako takt je to obyčejné XMP (1600 → 3200, 1800 → 3600).
/// Všechno ostatní jsou MT/s: 2400 pod štítkem 2666 je notebook, kterému
/// rychlost přibrzdil procesor, 2133 pod štítkem 3200 je vypnuté XMP —
/// jako takt by to byly sady DDR4-4800 a 4266, které se skoro nevidí.
/// Zkoušelo se i „pod štítkem a ne přesně polovina = nejisté“, jenže tím
/// se rozbil častý JEDEC štítek 2133 s XMP 3200 (takt 1600): ukázalo se
/// 1600 MT/s a záznam o PC radil zapnout XMP. Totéž pravidlo „pod běžnou
/// rychlostí = takt“ platí ve staré tabulce i pro štítek: deska, která do
/// obou polí píše takt (DDR4-3200 jako 1600/1600), vyjde jako 3200.
#[cfg(test)]
fn rychlosti(verze: (u8, u8), kod_typu: u8, speed_raw: u16, configured_raw: u16) -> Rychlosti {
    rychlosti_modulu(verze, kod_typu, speed_raw, configured_raw, false)
}

/// Totéž s vědomím, že modul XMP mít nemůže (ECC nebo registrovaný).
///
/// Takové moduly osazují pracovní stanice a servery (Xeon E5 v3/v4) a ty
/// DDR4 opravdu provozují i na 1600 a 1866 MT/s — při dvou nebo třech
/// modulech na kanál rychlost snižují. Pravidlo „pod běžnou rychlostí
/// generace = takt" by je zdvojnásobilo na 3200 a 3732. Bez XMP se ve
/// staré tabulce za takt bere jen číslo, které jako MT/s neexistuje,
/// nebo přesná polovina štítku.
fn rychlosti_modulu(
    verze: (u8, u8),
    kod_typu: u8,
    speed_raw: u16,
    configured_raw: u16,
    bez_xmp: bool,
) -> Rychlosti {
    let speed_raw = speed_raw as u32;
    let configured_raw = configured_raw as u32;
    let Some(Generace { min, bezna_min, max, .. }) = generace_ddr(kod_typu) else {
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

    let stara = verze < (3, 1);
    // Štítek (0x15): takt jen tehdy, když jako MT/s neexistuje — a ve
    // staré tabulce i když je pod běžnou rychlostí generace.
    let spodni = if stara { bezna_min } else { min };
    let speed_mts = if speed_raw != 0 && speed_raw < spodni { speed_raw * 2 } else { speed_raw };

    // Nastavená rychlost (0x20): takt (MHz), nebo už přenosy (MT/s)?
    let v = configured_raw;
    let takt = if v == 0 {
        false
    } else if v < min {
        // Jako MT/s pro generaci neexistuje — takt v každé verzi tabulky.
        true
    } else if !stara {
        // Od 3.1 je pole podle specifikace v MT/s.
        false
    } else if v * 2 > max {
        // Jako takt by to byla neexistující rychlost — už jsou to MT/s.
        false
    } else if v * 2 == speed_mts {
        // Přesně polovina štítku: takt rychlosti, na kterou modul je.
        true
    } else {
        // Pod běžnou rychlostí generace je to takt (XMP), jinak MT/s
        // (stejné jako štítek, přibrzděné procesorem, vypnuté XMP).
        // Modul bez XMP pod běžnou rychlostí opravdu běží.
        !bez_xmp && v < bezna_min
    };

    let configured_mts = if takt { v * 2 } else { v };
    Rychlosti {
        speed_mts,
        configured_mts,
        clock_mhz: configured_mts / 2,
        configured_was_clock: takt,
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
            // Přes get_u16: zkrácená (poškozená) struktura s length < 0x0E
            // dřív indexovala mimo rozsah a panika s panic=abort shazovala
            // službu při každém startu. Teď je to prázdný slot.
            let size_raw = get_u16(body, 0x0C);
            if size_raw != 0 {
                // 0x7FFF → skutečná velikost v Extended Size (u32 MB @0x1C).
                // 0xFFFF = „velikost neznámá“ → 0; dřív padla do větve kB
                // a vyšlo vymyšlených 31 MB. Totéž 0x7FFF bez Extended Size.
                let size_mb = if size_raw == 0xFFFF {
                    0
                } else if size_raw == 0x7FFF {
                    if length >= 0x20 {
                        u32::from_le_bytes(body[0x1C..0x20].try_into().unwrap()) as u64
                    } else {
                        0
                    }
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
                // ECC (celková šířka > datová) nebo registrovaný modul
                // (Type Detail, bit 13) XMP nemá. 0xFFFF = šířka neznámá.
                let (celkova, datova) = (get_u16(body, 0x08), get_u16(body, 0x0A));
                let ecc = celkova != 0xFFFF && datova != 0xFFFF && celkova > datova;
                let registrovany = get_u16(body, 0x13) & (1 << 13) != 0;
                let r = rychlosti_modulu(
                    verze,
                    kod_typu,
                    get_u16(body, 0x15),
                    get_u16(body, 0x20),
                    ecc || registrovany,
                );
                modules.push(RamModule {
                    size_mb,
                    speed_mts: r.speed_mts,
                    configured_mts: r.configured_mts,
                    slot,
                    manufacturer: get_string(strings, body.get(0x17).copied().unwrap_or(0)),
                    part_number: get_string(strings, body.get(0x1A).copied().unwrap_or(0)),
                    mem_type: generace_ddr(kod_typu).map(|g| g.nazev.to_string()).unwrap_or_default(),
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
        // poznat; podle verze tabulky ano. Krátce to tu vycházelo jako
        // „nejisté 1600“ a záznam o PC pak radil zapnout XMP — nesmí.
        let r = rychlosti((2, 8), DDR4, 2133, 1600);
        assert_eq!(r.configured_mts, 3200);
        assert_eq!(r.clock_mhz, 1600);
        assert!(r.configured_was_clock);
        assert_eq!(r.speed_mts, 2133);
    }

    #[test]
    fn stara_tabulka_oem_pise_mt_s() {
        // Nález 15: SMBIOS 2.8/3.0, DDR4-2400 bez XMP, deska do 0x20 píše
        // MT/s. Dřív vyšlo 4800 MT/s „z taktu“.
        let r = rychlosti((3, 0), DDR4, 2400, 2400);
        assert_eq!(r.configured_mts, 2400);
        assert_eq!(r.clock_mhz, 1200);
        assert!(!r.configured_was_clock);
        let r = rychlosti((2, 8), DDR4, 2133, 2133);
        assert_eq!(r.configured_mts, 2133);
        assert!(!r.configured_was_clock);
        let r = rychlosti((3, 0), DDR4, 2666, 2666);
        assert_eq!(r.configured_mts, 2666);
    }

    #[test]
    fn stara_tabulka_pod_stitkem_je_skutecna_rychlost() {
        // Notebook s DDR4-2666, kterému procesor dovolí jen 2400: MT/s.
        // Jako takt by to byla DDR4-4800, kterou nikdo do notebooku nedá.
        let r = rychlosti((3, 0), DDR4, 2666, 2400);
        assert_eq!(r.configured_mts, 2400);
        assert!(!r.configured_was_clock);
        // Sada DDR4-3200 s vypnutým XMP běží na JEDEC 2133. To se musí
        // ukázat jako 2133 — záznam o PC pak správně radí zapnout XMP.
        let r = rychlosti((2, 8), DDR4, 3200, 2133);
        assert_eq!(r.configured_mts, 2133);
        assert!(!r.configured_was_clock);
        // XMP nad JEDEC štítkem, deska píše MT/s.
        let r = rychlosti((2, 8), DDR4, 2133, 2400);
        assert_eq!(r.configured_mts, 2400);
    }

    #[test]
    fn stara_tabulka_presna_polovina_stitku_je_takt() {
        // DDR4-4266 hlášená jako 4266/2133: přesná polovina má přednost
        // před pravidlem běžné rychlosti.
        let r = rychlosti((2, 8), DDR4, 4266, 2133);
        assert_eq!(r.configured_mts, 4266);
        assert!(r.configured_was_clock);
        // XMP 3600 hlášené taktem pod JEDEC štítkem.
        let r = rychlosti((2, 8), DDR4, 2133, 1800);
        assert_eq!(r.configured_mts, 3600);
    }

    #[test]
    fn ecc_a_registrovane_moduly_bez_xmp() {
        // Xeon, dva moduly na kanál: deska píše MT/s 1866 — žádný takt.
        let r = rychlosti_modulu((2, 8), DDR4, 2133, 1866, true);
        assert_eq!(r.configured_mts, 1866);
        assert!(!r.configured_was_clock);
        // Stejný tvar u běžného modulu je XMP hlášené taktem.
        let r = rychlosti_modulu((2, 8), DDR4, 2133, 1866, false);
        assert_eq!(r.configured_mts, 3732);
        // Přesná polovina štítku platí i bez XMP (deska, která píše takt).
        let r = rychlosti_modulu((2, 8), DDR4, 2400, 1200, true);
        assert_eq!(r.configured_mts, 2400);
    }

    #[test]
    fn stara_tabulka_takt_v_obou_polich() {
        // Deska, která do obou polí píše takt: DDR4-3200 jako 1600/1600.
        let r = rychlosti((2, 8), DDR4, 1600, 1600);
        assert_eq!(r.speed_mts, 3200);
        assert_eq!(r.configured_mts, 3200);
        // V nové tabulce je 1600 platné MT/s (DDR4-1600) a nechá se.
        let r = rychlosti((3, 2), DDR4, 1600, 1600);
        assert_eq!(r.configured_mts, 1600);
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

    /// Jedna struktura Type 17 s danou formátovanou částí + konec tabulky.
    fn tabulka_s_type17(body: &[u8]) -> Vec<u8> {
        let mut d = body.to_vec();
        d.extend_from_slice(&[0, 0]); // prázdný string-set
        d.extend_from_slice(&[127, 4, 0, 0, 0, 0]);
        d
    }

    #[test]
    fn zkracena_type17_nespadne() {
        // length 0x0C: velikost @0x0C už leží mimo strukturu. Dřív panika.
        let mut b = vec![0u8; 0x0C];
        b[0] = 17;
        b[1] = 0x0C;
        let (m, sloty) = parse_type17(&tabulka_s_type17(&b), (2, 8));
        assert!(m.is_empty());
        assert_eq!(sloty, 1);
    }

    #[test]
    fn velikost_ffff_je_neznama() {
        let mut b = vec![0u8; 0x28];
        b[0] = 17;
        b[1] = 0x28;
        b[0x0C] = 0xFF;
        b[0x0D] = 0xFF;
        let (m, _) = parse_type17(&tabulka_s_type17(&b), (3, 3));
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].size_mb, 0);
        // A běžná velikost v MB projde beze změny.
        b[0x0C] = 0x00;
        b[0x0D] = 0x20; // 8192 MB
        let (m, _) = parse_type17(&tabulka_s_type17(&b), (3, 3));
        assert_eq!(m[0].size_mb, 8192);
    }
}
