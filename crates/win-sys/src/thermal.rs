//! Teplota CPU — degradační kaskáda (SPEC kap. 15.2).
//!
//! Windows teplotu CPU z userspace přímo nedávají. Existují ale cesty,
//! které nepotřebují vlastní kernel driver — a ty se zkoušejí v pořadí
//! od nejpřesnější k nejdostupnější:
//!
//! 1. **HWiNFO** sdílená paměť — když si ji uživatel spustil sám.
//! 2. **LibreHardwareMonitor / OpenHardwareMonitor** přes WMI — totéž.
//! 3. **ACPI thermal zone** (`root\WMI`) — bez cizího nástroje, ale
//!    desktopy tudy často hlásí čipset, ne jádra.
//! 4. **Takty + throttling** — vždy, na 100 % strojů. Nedá stupně, ale
//!    odpoví na otázku, kvůli které se lidi na teplotu ptají:
//!    „zpomaluje mi to kvůli teplu?"
//!
//! Železné pravidlo: **nikdy nepředstírej číslo, které nemáš.** Když
//! teplotu nikdo nehlásí, `celsius` je `None` a zdroj to řekne nahlas.
//! Vlastní kernel driver se neshipuje — WinRing0 je na blocklistu
//! Microsoftu a destabilizace systému je přesně to, co tenhle nástroj
//! nemá dělat.

use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery, FILE_MAP_READ,
    MEMORY_BASIC_INFORMATION,
};

/// Odkud teplota přišla. Jde do UI vedle čísla — uživatel má vždycky
/// vidět, čemu věří.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempSource {
    /// HWiNFO (běží u uživatele, my jen čteme jeho sdílenou paměť).
    Hwinfo,
    /// LibreHardwareMonitor / OpenHardwareMonitor přes WMI.
    Lhm,
    /// ACPI thermal zone — často čipset, ne jádra.
    Acpi,
    /// Teplotu nikdo nehlásí; máme jen takty a throttling.
    Unavailable,
}

impl TempSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TempSource::Hwinfo => "HWiNFO",
            TempSource::Lhm => "LibreHardwareMonitor",
            TempSource::Acpi => "ACPI",
            TempSource::Unavailable => "nedostupné",
        }
    }
}

/// Tepelný stav CPU.
#[derive(Debug, Clone, PartialEq)]
pub struct CpuThermal {
    /// Teplota ve °C, když ji někdo hlásí.
    pub celsius: Option<f32>,
    pub source: TempSource,
    /// Aktuální takt (MHz) a maximum, které procesor umí.
    pub clock_mhz: u32,
    pub max_mhz: u32,
}

impl CpuThermal {
    /// Běží procesor pod svým maximem, tedy je něčím brzděný?
    /// Prahová hodnota 95 % — malé odchylky jsou běžné i bez omezení.
    pub fn throttling(&self) -> bool {
        self.max_mhz > 0 && (self.clock_mhz as f32) < (self.max_mhz as f32 * 0.95)
    }
}

/// Přečte tepelný stav CPU celou kaskádou.
///
/// Volá se **po 5–10 s, ne v sekundovém cyklu** — kroky 1–3 sahají na
/// WMI a sdílenou paměť cizího procesu. COM musí být na vlákně
/// inicializované (`wic::init_com_for_thread`).
pub fn cpu_thermal(n_cpus: usize) -> CpuThermal {
    let (clock_mhz, max_mhz) = crate::sysinfo::cpu_clocks(n_cpus).unwrap_or((0, 0));
    // Nula ani 255 °C nejsou teploty, jsou to nepřečtené hodnoty.
    // Zdroj, který vrátí úspěch a proměnnou nechá ležet, poslal
    // do záznamu „CPU 0 °C" jako fakt — radši se jde o krok dál
    // v kaskádě a nakonec se přizná, že teplotu nikdo nehlásí.
    let ok = |c: f32| (5.0..=120.0).contains(&c).then_some(c);
    let (celsius, source) = hwinfo_cpu_temp()
        .and_then(ok)
        .map(|c| (Some(c), TempSource::Hwinfo))
        .or_else(|| lhm_cpu_temp().and_then(ok).map(|c| (Some(c), TempSource::Lhm)))
        .or_else(|| acpi_temp().and_then(ok).map(|c| (Some(c), TempSource::Acpi)))
        .unwrap_or((None, TempSource::Unavailable));
    CpuThermal {
        celsius,
        source,
        clock_mhz,
        max_mhz,
    }
}

// ── 1. HWiNFO sdílená paměť ────────────────────────────────────────

/// Hlavička `HWiNFO_SENSORS_SHARED_MEM2` — jen offsety, které čteme.
const HWINFO_SHM: &str = r"Global\HWiNFO_SENS_SM2";
const HDR_SIGNATURE: usize = 0x00;
const HDR_READING_OFFSET: usize = 0x20;
const HDR_READING_SIZE: usize = 0x24;
const HDR_READING_COUNT: usize = 0x28;
/// Typ měření: 1 = teplota.
const READING_TYPE_TEMP: u32 = 1;
/// Popisek v prvku měření (`szLabelOrig`, 128 B).
const READ_LABEL: usize = 12;
/// Hodnota měření (`Value`, double) v prvku: za tReading, dwSensorIndex,
/// dwReadingID (3×4 B), szLabelOrig[128], szLabelUser[128] a szUnit[16]
/// pod #pragma pack(1), tedy 12 + 128 + 128 + 16 = 284. Pevný offset,
/// ne "velikost prvku − 32": novější HWiNFO má prvek 460 B a za čtyřmi
/// double ještě utfLabelUser[128] a utfUnit[16] — odečet od konce pak
/// mířil doprostřed popisku a teplota z HWiNFO tiše nevycházela.
const READ_VALUE: usize = READ_LABEL + 128 + 128 + 16;
/// Konec hlavičky, kterou čteme (`HDR_READING_COUNT` + 4).
const HDR_END: usize = HDR_READING_COUNT + 4;

/// Vejdou se všechny prvky měření, jak je popisuje hlavička, do
/// namapované oblasti? Hlavičce se dřív věřilo naslepo — nekonzistentní
/// nebo podvržená sekce znamenala čtení za koncem pohledu a access
/// violation, která shodí celou službu (není to panic, nic ji nechytí).
/// Prvek musí pojmout popisek (12 + 128 B) i čtyři `double` na konci,
/// jinak by čtení přesahovalo do sousedního prvku nebo za konec dat.
fn readings_fit(region: usize, off: usize, size: usize, count: usize) -> bool {
    if size < READ_VALUE + 32 || count == 0 || count >= 10_000 {
        return false;
    }
    count
        .checked_mul(size)
        .and_then(|n| n.checked_add(off))
        .is_some_and(|end| end <= region)
}

/// Teplota CPU z HWiNFO, když uživatel má zapnutou sdílenou paměť.
fn hwinfo_cpu_temp() -> Option<f32> {
    // SAFETY: mapování se vždy odmapuje a handle zavře; ze sdílené
    // paměti se jen čte, podle velikostí hlášených v hlavičce.
    unsafe {
        let name: Vec<u16> = HWINFO_SHM
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let map = OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr())).ok()?;
        let view = MapViewOfFile(map, FILE_MAP_READ, 0, 0, 0);
        if view.Value.is_null() {
            let _ = CloseHandle(map);
            return None;
        }
        let base = view.Value as *const u8;
        let rd = |off: usize| -> u32 { std::ptr::read_unaligned(base.add(off) as *const u32) };
        // Skutečná velikost pohledu (zaokrouhlená na stránky — i to je
        // čitelná paměť). Hlavička sama o sobě nic nezaručuje.
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let region = if VirtualQuery(
            Some(view.Value as *const _),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        ) == 0
        {
            0
        } else {
            mbi.RegionSize
        };

        let mut out = None;
        // "HWiS" — bez podpisu tomu nevěříme.
        if region >= HDR_END && rd(HDR_SIGNATURE) == u32::from_le_bytes(*b"HWiS") {
            let off = rd(HDR_READING_OFFSET) as usize;
            let size = rd(HDR_READING_SIZE) as usize;
            let count = rd(HDR_READING_COUNT) as usize;
            if readings_fit(region, off, size, count) {
                let value_off = READ_VALUE;
                let mut best: Option<f32> = None;
                for i in 0..count {
                    let e = off + i * size;
                    if std::ptr::read_unaligned(base.add(e) as *const u32) != READING_TYPE_TEMP {
                        continue;
                    }
                    let label = cstr(base.add(e + READ_LABEL), 128).to_ascii_lowercase();
                    // „CPU Package" je teplota celého pouzdra — přesně
                    // to, co lidi myslí „teplotou procesoru".
                    let score = if label.contains("cpu package") {
                        3
                    } else if label.contains("cpu") && label.contains("die") {
                        2
                    } else if label.starts_with("core ") || label.contains("cpu") {
                        1
                    } else {
                        0
                    };
                    if score == 0 {
                        continue;
                    }
                    let v = std::ptr::read_unaligned(base.add(e + value_off) as *const f64) as f32;
                    if (1.0..=125.0).contains(&v) && (score == 3 || best.is_none()) {
                        best = Some(v);
                        if score == 3 {
                            break;
                        }
                    }
                }
                out = best;
            }
        }
        let _ = UnmapViewOfFile(view);
        let _ = CloseHandle(map);
        out
    }
}

/// Nulou ukončený ASCII řetězec z pevně velkého pole.
/// # Safety
/// `ptr` musí ukazovat na `max` čitelných bajtů.
unsafe fn cstr(ptr: *const u8, max: usize) -> String {
    let slice = std::slice::from_raw_parts(ptr, max);
    let end = slice.iter().position(|&b| b == 0).unwrap_or(max);
    String::from_utf8_lossy(&slice[..end]).into_owned()
}

// ── 2. LibreHardwareMonitor / OpenHardwareMonitor přes WMI ─────────

/// Desetinné číslo z WMI řetězce. wmi::get_prop převádí float na text
/// podle lokalizace účtu — služba běží jako SYSTEM a ten má na české
/// instalaci desetinnou čárku („45,875“), na které `f32::from_str`
/// selže. Bez téhle pojistky LHM na českých Windows nikdy neprošel.
fn parse_wmi_float(s: &str) -> Option<f32> {
    s.trim().replace(',', ".").parse().ok()
}

/// Teplota CPU z LHM/OHM, když uživatel jeden z nich má spuštěný.
fn lhm_cpu_temp() -> Option<f32> {
    for ns in [r"root\LibreHardwareMonitor", r"root\OpenHardwareMonitor"] {
        let rows = crate::wmi::query(
            ns,
            "SELECT Name, Value, SensorType FROM Sensor WHERE SensorType = 'Temperature'",
            &["Name", "Value"],
        );
        let mut best: Option<f32> = None;
        for r in &rows {
            // Nečitelný řádek přeskočit — `?` tu dřív ukončil celou
            // funkci a s ní i druhý jmenný prostor.
            let Some(name) = r.get("Name").map(|s| s.to_ascii_lowercase()) else {
                continue;
            };
            let Some(v) = r.get("Value").and_then(|s| parse_wmi_float(s)) else {
                continue;
            };
            if !(1.0..=125.0).contains(&v) {
                continue;
            }
            // Package je celek; jednotlivá jádra jen když package není.
            if name.contains("package") {
                return Some(v);
            }
            if name.contains("cpu") || name.contains("core") {
                best = Some(best.map_or(v, |b: f32| b.max(v)));
            }
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

// ── 3. ACPI thermal zone ───────────────────────────────────────────

/// Teplota z ACPI thermal zone. Desktopy tudy hlásí spíš čipset než
/// jádra — proto je až třetí a v UI se u ní ukazuje zdroj.
fn acpi_temp() -> Option<f32> {
    let rows = crate::wmi::query(
        r"root\WMI",
        "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature",
        &["CurrentTemperature"],
    );
    // ACPI hlásí desetiny kelvinu.
    rows.iter()
        .filter_map(|r| r.get("CurrentTemperature")?.parse::<f64>().ok())
        .map(|k| (k / 10.0 - 273.15) as f32)
        .filter(|c| (1.0..=125.0).contains(c))
        .fold(None, |acc: Option<f32>, c| {
            Some(acc.map_or(c, |a| a.max(c)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Throttling se hlásí jen proti známému maximu, nikdy z ničeho.
    #[test]
    fn throttling_needs_known_max() {
        let t = CpuThermal {
            celsius: None,
            source: TempSource::Unavailable,
            clock_mhz: 2000,
            max_mhz: 0,
        };
        assert!(!t.throttling());

        let t = CpuThermal { max_mhz: 4000, ..t };
        assert!(t.throttling());

        let t = CpuThermal {
            clock_mhz: 3900,
            ..t
        };
        assert!(!t.throttling());
    }

    // SYSTEM na české instalaci formátuje float s čárkou.
    #[test]
    fn wmi_float_s_desetinnou_carkou() {
        assert_eq!(parse_wmi_float("45,875"), Some(45.875));
        assert_eq!(parse_wmi_float("45.5"), Some(45.5));
        assert_eq!(parse_wmi_float(" 45 "), Some(45.0));
        assert_eq!(parse_wmi_float("n/a"), None);
    }

    // Hlavička HWiNFO, která slibuje víc prvků, než sekce obsahuje,
    // se odmítne místo čtení za koncem pohledu.
    #[test]
    fn hwinfo_hlavicka_nesmi_prerust_mapovani() {
        // Skutečné rozložení: 316 B na prvek.
        assert!(readings_fit(4096 * 4, 0x2C, 316, 40));
        // 9999 prvků v 4 KiB sekci.
        assert!(!readings_fit(4096, 0x2C, 316, 9999));
        // Prvek menší než popisek + čtyři double.
        assert!(!readings_fit(1 << 20, 0x2C, 45, 10));
        // Přetečení násobení/sčítání nesmí projít.
        assert!(!readings_fit(usize::MAX, usize::MAX - 10, 316, 1));
        assert!(!readings_fit(4096, 0x2C, 316, 0));
    }

    // Kaskáda vždy něco vrátí a nikdy si teplotu nevymyslí: buď je
    // číslo se skutečným zdrojem, nebo None a „nedostupné".
    #[test]
    fn cascade_never_fakes_a_number() {
        crate::wic::init_com_for_thread();
        let n = std::thread::available_parallelism().map_or(1, |n| n.get());
        let t = cpu_thermal(n);
        match t.celsius {
            Some(c) => {
                assert_ne!(t.source, TempSource::Unavailable);
                assert!((1.0..=125.0).contains(&c), "nesmyslná teplota {c}");
            }
            None => assert_eq!(t.source, TempSource::Unavailable),
        }
        // Takty jsou dostupné vždy — to je smysl posledního stupně.
        assert!(t.max_mhz > 0, "maximální takt se nepodařilo přečíst");
    }
}
