//! Teplota CPU — degradační kaskáda (SPEC kap. 15.2).
//!
//! Windows teplotu CPU z userspace přímo nedávají. Existují ale cesty,
//! které nepotřebují vlastní kernel driver — a ty se zkoušejí v pořadí
//! od nejpřesnější k nejdostupnější:
//!
//! 1. **PawnIO** — open-source ovladač, který si uživatel nainstaloval
//!    sám (nebo mu ho nabídl LibreHardwareMonitor); služba přes něj čte
//!    čidlo procesoru napřímo (viz `pawnio`).
//! 2. **ACPI thermal zone** (`root\WMI`) — bez cizího nástroje, ale
//!    desktopy tudy často hlásí čipset, ne jádra.
//! 3. **Takty + throttling** — vždy, na 100 % strojů. Nedá stupně, ale
//!    odpoví na otázku, kvůli které se lidi na teplotu ptají:
//!    „zpomaluje mi to kvůli teplu?"
//!
//! Čte se jen z otevřených nástrojů s dohledatelným zdrojákem a
//! bezpečným ovladačem. Dřívější čtení přes WMI z LibreHardwareMonitoru
//! a OpenHardwareMonitoru je pryč: LibreHardwareMonitor WMI od verze
//! 0.9.5 nemá, takže tudy teplota chodila jen ze starých verzí a z OHM
//! — a ty běží na zranitelném ovladači WinRing0.
//!
//! Železné pravidlo: **nikdy nepředstírej číslo, které nemáš.** Když
//! teplotu nikdo nehlásí, `celsius` je `None` a zdroj to řekne nahlas.
//! Vlastní kernel driver se neshipuje — WinRing0 je na blocklistu
//! Microsoftu a destabilizace systému je přesně to, co tenhle nástroj
//! nemá dělat.

/// Odkud teplota přišla. Jde do UI vedle čísla — uživatel má vždycky
/// vidět, čemu věří.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempSource {
    /// PawnIO — čidlo procesoru napřímo přes ovladač uživatele.
    PawnIo,
    /// ACPI thermal zone — často čipset, ne jádra.
    Acpi,
    /// Teplotu nikdo nehlásí; máme jen takty a throttling.
    Unavailable,
}

impl TempSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TempSource::PawnIo => "PawnIO",
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
/// Volá se **po 5–10 s, ne v sekundovém cyklu** — krok 1 sahá na
/// ovladač, krok 2 na WMI. COM musí být na vlákně
/// inicializované (`wic::init_com_for_thread`).
pub fn cpu_thermal(n_cpus: usize) -> CpuThermal {
    let (clock_mhz, max_mhz) = crate::sysinfo::cpu_clocks(n_cpus).unwrap_or((0, 0));
    // Nula ani 255 °C nejsou teploty, jsou to nepřečtené hodnoty.
    // Zdroj, který vrátí úspěch a proměnnou nechá ležet, poslal
    // do záznamu „CPU 0 °C" jako fakt — radši se jde o krok dál
    // v kaskádě a nakonec se přizná, že teplotu nikdo nehlásí.
    let ok = |c: f32| (5.0..=120.0).contains(&c).then_some(c);
    let (celsius, source) = crate::pawnio::cpu_temp()
        .and_then(ok)
        .map(|c| (Some(c), TempSource::PawnIo))
        .or_else(|| acpi_temp().and_then(ok).map(|c| (Some(c), TempSource::Acpi)))
        .unwrap_or((None, TempSource::Unavailable));
    CpuThermal {
        celsius,
        source,
        clock_mhz,
        max_mhz,
    }
}

// ── ACPI thermal zone ───────────────────────────────────────────

/// Teplota z ACPI thermal zone. Desktopy tudy hlásí spíš čipset než
/// jádra — proto je až za PawnIO a v UI se u ní ukazuje zdroj.
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
