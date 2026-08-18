//! Ovladače — co v počítači běží, od koho to je a jak je to staré
//! (v10, SPEC kap. 6).
//!
//! Sekce jen čte. Instalovat ovladače Winsent neumí a nebude — od toho
//! je Windows Update, který to má vyzkoušené na milionech strojů a umí
//! se vrátit zpátky. Ukázat, **který ovladač je z roku 2015 a od koho**,
//! je práce, kterou Správce zařízení dělá mizerně a která nikoho nemůže
//! rozbít.
//!
//! Zdroj je SetupAPI (`SetupDiEnumDeviceInfo`), ne WMI: `Win32_PnPEntity`
//! je pomalý a `Win32_PnPSignedDriver` je pověstný tím, že se na něm
//! dotazy zaseknou na desítky sekund.

use core_types::proc::{DriverRow, DriversReport};

/// Ovladače seskupené po skutečných zařízeních.
///
/// Jedno zařízení má v systému několik uzlů (rozhraní, HID kolekce) —
/// všechny se stejným ovladačem. Kdyby se seznam nesloučil, měl by
/// uživatel u jedné myši šestnáct řádků o témž ovladači. Klíč skupiny
/// počítá `collector_hw::device_group_key`, aby Hardware a Drivers
/// mluvily o týchž kusech hardwaru.
pub fn report() -> DriversReport {
    let devices = win_sys::devices::devices();

    // Nejdřív jména skupin z Hardwaru, ať se zařízení jmenuje všude stejně.
    let rows_hw = collector_hw::devices();
    let mut name_of: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for r in &rows_hw {
        name_of.entry(&r.group_key).or_insert(&r.group_name);
    }

    let mut best: std::collections::HashMap<String, DriverRow> = std::collections::HashMap::new();
    for d in &devices {
        // Zařízení bez ovladače nemá v téhle sekci co dělat.
        if d.driver_version.is_empty() && d.driver_inf.is_empty() {
            continue;
        }
        let key = collector_hw::device_group_key(d);
        let row = DriverRow {
            device: name_of
                .get(key.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| d.name.clone()),
            group_key: key.clone(),
            class: d.class.clone(),
            class_desc: d.class_desc.clone(),
            provider: d.driver_provider.clone(),
            version: d.driver_version.clone(),
            date: d.driver_date.clone(),
            inf: d.driver_inf.clone(),
            // `oem<číslo>.inf` = doinstalováno zvenčí. Všechno ostatní
            // přišlo s Windows.
            third_party: is_oem_inf(&d.driver_inf),
            problem_code: d.problem_code,
        };
        // Ve skupině vyhrává ten uzel, který o ovladači ví nejvíc:
        // rozhraní často hlásí obecný ovladač Microsoftu, zatímco
        // zařízení samo ten pravý od výrobce.
        match best.get(&key) {
            Some(old) if rank(old) <= rank(&row) => {}
            _ => {
                best.insert(key, row);
            }
        }
    }

    let mut drivers: Vec<DriverRow> = best.into_values().collect();
    // Nejdřív problémy, pak nejstarší — to je pořadí, ve kterém se na
    // ovladače člověk ptá.
    drivers.sort_by(|a, b| {
        (b.problem_code != 0)
            .cmp(&(a.problem_code != 0))
            .then_with(|| a.date_sortable().cmp(&b.date_sortable()))
            .then_with(|| a.device.cmp(&b.device))
    });

    DriversReport {
        third_party: drivers.iter().filter(|d| d.third_party).count() as u32,
        with_problem: drivers.iter().filter(|d| d.problem_code != 0).count() as u32,
        drivers,
    }
}

/// Čím nižší číslo, tím lepší kandidát na reprezentanta skupiny.
fn rank(r: &DriverRow) -> u8 {
    // Ovladač od výrobce vypovídá víc než obecný od Microsoftu.
    let vendor = !r.provider.is_empty() && r.provider != "Microsoft";
    match (r.third_party || vendor, !r.version.is_empty()) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    }
}

/// Doinstalovaný ovladač? `oem12.inf` ano, `usbport.inf` ne.
fn is_oem_inf(inf: &str) -> bool {
    let lc = inf.to_ascii_lowercase();
    lc.starts_with("oem") && lc.ends_with(".inf") && lc[3..lc.len() - 4].parse::<u32>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Rozpoznání doinstalovaného ovladače podle jména INF souboru.
    #[test]
    fn oem_inf_is_recognised() {
        assert!(is_oem_inf("oem42.inf"));
        assert!(is_oem_inf("OEM7.INF"));
        // Ovladače z Windows se jmenují po tom, co dělají.
        assert!(!is_oem_inf("usbport.inf"));
        assert!(!is_oem_inf("nvlddmkm.inf"));
        // „oem" v názvu ještě nedělá oem INF.
        assert!(!is_oem_inf("oemsetup.inf"));
        assert!(!is_oem_inf(""));
    }

    // Na živém stroji musí být ovladače a nesmí se opakovat zařízení.
    #[test]
    fn report_lists_drivers_once_per_device() {
        win_sys::wic::init_com_for_thread();
        let r = report();
        assert!(!r.drivers.is_empty(), "žádné ovladače");
        let mut seen = std::collections::HashSet::new();
        for d in &r.drivers {
            assert!(
                seen.insert(d.group_key.clone()),
                "zařízení {} je v seznamu dvakrát",
                d.device
            );
        }
    }
}
