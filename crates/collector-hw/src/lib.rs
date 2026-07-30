//! collector-hw — hardwarový přehled (v9, SPEC kap. 15.1).
//!
//! Skládá dohromady to, co už umí win-sys: desku a firmware ze SMBIOS,
//! baterii z power API, zdraví disků ze SMART a tepelný stav CPU
//! z kaskády. Sám nic nepočítá a nic nemění — je to čtecí crate
//! (SPEC kap. 2, oddělené cesty).
//!
//! Dělení podle ceny: `board()` je statické (jednou při startu),
//! zbytek se obnovuje — ale i tak po sekundách, ne v cyklu, protože
//! tepelná kaskáda sahá na WMI.

use core_types::proc::{
    BatteryInfo, BoardInfo, CpuThermalInfo, DeviceRow, DiskHealthRow, HardwareReport, VolumeRow,
};

/// Všechna přítomná zařízení (SetupAPI). Cíl je úplnost — ne jen CPU
/// a disky, ale i řadiče, klávesnice, zvukovky a síťovky, se jménem
/// modelu a verzí ovladače.
pub fn devices() -> Vec<DeviceRow> {
    win_sys::devices::devices()
        .into_iter()
        .map(|d| DeviceRow {
            name: d.name,
            manufacturer: d.manufacturer,
            class: d.class,
            class_desc: d.class_desc,
            hardware_id: d.hardware_id,
            driver_version: d.driver_version,
            driver_date: d.driver_date,
            problem_code: d.problem_code,
        })
        .collect()
}

/// Statická část: deska, BIOS, stroj. Mění se leda aktualizací BIOSu.
pub fn board() -> BoardInfo {
    let b = win_sys::smbios::board();
    BoardInfo {
        manufacturer: b.manufacturer,
        product: b.product,
        version: b.version,
        bios_vendor: b.bios_vendor,
        bios_version: b.bios_version,
        bios_date: b.bios_date,
        system_manufacturer: b.system_manufacturer,
        system_product: b.system_product,
    }
}

/// Baterie. `None` = stroj žádnou nemá (desktop) — nikdy se
/// nepředstírá plné nabití.
pub fn battery() -> Option<BatteryInfo> {
    let b = win_sys::battery::battery()?;
    Some(BatteryInfo {
        percent: b.percent,
        ac_online: b.ac_online,
        charging: b.charging,
        remaining_s: b.remaining_s,
        design_mwh: b.design_mwh,
        full_mwh: b.full_mwh,
        cycles: b.cycles,
        wear_pct: b.wear_pct(),
    })
}

/// Tepelný stav CPU. Sahá na WMI — volat po 5–10 s, ne v cyklu.
/// COM musí být na vlákně inicializované.
pub fn cpu_thermal(n_cpus: usize) -> CpuThermalInfo {
    let t = win_sys::thermal::cpu_thermal(n_cpus);
    CpuThermalInfo {
        celsius: t.celsius,
        temp_source: t.source.as_str().to_string(),
        clock_mhz: t.clock_mhz,
        max_mhz: t.max_mhz,
        throttling: t.throttling(),
    }
}

/// Celý přehled najednou. Zdraví disků a svazky si volající dodá —
/// čte je už jinde (v4, SPEC 11.1) a nemá smysl skenovat dvakrát.
pub fn report(
    n_cpus: usize,
    disks: Vec<DiskHealthRow>,
    volumes: Vec<VolumeRow>,
    ts: i64,
) -> HardwareReport {
    HardwareReport {
        board: board(),
        battery: battery(),
        cpu_thermal: cpu_thermal(n_cpus),
        disks,
        volumes,
        devices: devices(),
        ts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Přehled musí projít na jakémkoliv stroji — desktop bez baterie,
    // SATA bez SMART logu, CPU bez teploty. Nic z toho není chyba.
    #[test]
    fn report_works_on_any_machine() {
        win_sys::wic::init_com_for_thread();
        let n = std::thread::available_parallelism().map_or(1, |n| n.get());
        let r = report(n, Vec::new(), Vec::new(), 1);
        // Takty jsou dostupné vždy — poslední stupeň kaskády.
        assert!(r.cpu_thermal.max_mhz > 0);
        // Teplota buď je i se zdrojem, nebo není a zdroj to říká.
        if r.cpu_thermal.celsius.is_none() {
            assert_eq!(r.cpu_thermal.temp_source, "nedostupné");
        } else {
            assert_ne!(r.cpu_thermal.temp_source, "nedostupné");
        }
    }

    // Opotřebení baterie se nikdy nevymyslí bez obou kapacit.
    #[test]
    fn battery_wear_only_with_capacities() {
        if let Some(b) = battery() {
            if b.design_mwh.is_none() || b.full_mwh.is_none() {
                assert!(b.wear_pct.is_none());
            }
        }
    }
}
