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
    let raw = win_sys::devices::devices();

    // Nejdřív klíče, pak jména: jméno skupiny se vybírá ze VŠECH jejích
    // členů, takže se musí projít dvakrát.
    let keys: Vec<String> = raw.iter().map(device_group_key).collect();
    let names = group_names(&raw, &keys);

    raw.into_iter()
        .zip(keys)
        .map(|(d, key)| DeviceRow {
            group_name: names.get(&key).cloned().unwrap_or_else(|| d.name.clone()),
            group_key: key,
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

/// Klíč fyzického zařízení — pod ním patří k sobě všechno, co ve
/// skutečnosti JE jedno zařízení.
///
/// Windows totiž jeden kus hardwaru rozepíšou na řadu samostatných
/// zařízení. Naměřeno na jednom stroji: bezdrátový přijímač Logitech
/// zabral 10 řádků, herní klávesnice 11 a myš Razer rovnou 16 —
/// pokaždé táž dvojice VID/PID, jen jiné rozhraní (`MI_00`), jiná HID
/// kolekce (`Col03`) nebo jiná sběrnice (`USB\`, `HID\`, `RZVIRTUAL\`).
/// Uživatel přitom má na stole jednu myš.
///
/// Klíčem je proto identita MODELU, ne konkrétního uzlu:
/// * `VID`+`PID` — u USB a HID; zahazuje se sběrnice, revize firmwaru,
///   číslo rozhraní i kolekce, takže se všechny kusy jedné myši sejdou,
///   ale dvě různé myši ne (naměřeno: `USB Input Device` je šestkrát,
///   ale jsou to dvě zařízení po třech rozhraních).
/// * `VEN`+`DEV` — u PCI a ACPI, kde tenhle pár určuje model čipu.
/// * jinak celé ID **a k tomu jméno**. Bez jména by to slepilo věci,
///   které jen sdílejí sběrnici: pět zvukových vstupů a výstupů má
///   shodné `MMDEVAPI\AudioEndpoints`, a přitom je to pět různých věcí.
pub fn device_group_key(d: &win_sys::devices::Device) -> String {
    let id = d.hardware_id.to_ascii_uppercase();
    if let (Some(vid), Some(pid)) = (field(&id, "VID_"), field(&id, "PID_")) {
        return format!("dev:{vid}:{pid}");
    }
    if let (Some(ven), Some(dev)) = (field(&id, "VEN_"), field(&id, "DEV_")) {
        return format!("chip:{ven}:{dev}");
    }
    format!("id:{}|{}", id, d.name.to_ascii_uppercase())
}

/// Hodnota za značkou (`VID_046D&PID_C54D` → `046D`). Bere jen
/// alfanumerické znaky, tedy končí na `&`, `\` i na konci řetězce.
fn field(id: &str, tag: &str) -> Option<String> {
    let start = id.find(tag)? + tag.len();
    let v: String = id[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (!v.is_empty()).then_some(v)
}

/// Nejvýstižnější jméno pro každou skupinu.
///
/// Pořadí je dané tím, jak moc jméno vypovídá o skutečném zařízení:
/// 1. jak se hlásí samo zařízení (`bus_desc`) a zároveň není jen jedním
///    z mnoha rozhraní — u přijímače Logitech to dá „USB Receiver",
///    u klávesnice rovnou její model,
/// 2. jméno, které nezačíná obecným popisem třídy („HID-compliant…",
///    „USB Input Device") — takové aspoň jmenuje výrobce,
/// 3. první, co je po ruce; lepší nic není.
fn group_names(
    raw: &[win_sys::devices::Device],
    keys: &[String],
) -> std::collections::HashMap<String, String> {
    let mut best: std::collections::HashMap<String, (u8, String)> =
        std::collections::HashMap::new();
    for (d, key) in raw.iter().zip(keys) {
        // Rozhraní a kolekce jsou části zařízení, ne zařízení samo —
        // jejich jméno („RzVirt_04") o hardwaru nevypovídá.
        let id = d.hardware_id.to_ascii_uppercase();
        let is_part = id.contains("&MI_") || id.contains("&COL");
        let (rank, name) = if !d.bus_desc.is_empty() && !is_part {
            (0, d.bus_desc.clone())
        } else if !generic_name(&d.name) {
            (1, d.name.clone())
        } else {
            (2, d.name.clone())
        };
        match best.get(key) {
            Some((r, _)) if *r <= rank => {}
            _ => {
                best.insert(key.clone(), (rank, name));
            }
        }
    }
    best.into_iter().map(|(k, (_, n))| (k, n)).collect()
}

/// Jméno, které jen opakuje třídu zařízení a o modelu neříká nic.
fn generic_name(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "HID-compliant",
        "HID Keyboard",
        "HID Mouse",
        "USB Input Device",
        "USB Composite Device",
        "Generic ",
        "Standard ",
        "PCI ",
        "Composite ",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
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

    // Klíč skupiny na skutečných ID z jednoho stroje: co je jedno
    // zařízení, musí se sejít; co jsou dvě, se sejít nesmí.
    #[test]
    fn group_key_merges_one_device_and_splits_two() {
        let dev = |hw: &str, name: &str| win_sys::devices::Device {
            name: name.into(),
            hardware_id: hw.into(),
            ..Default::default()
        };

        // Jeden přijímač Logitech: tři rozhraní na USB a k tomu HID
        // kolekce — v seznamu deset řádků, ve skutečnosti jeden kus.
        let a = device_group_key(&dev(r"USB\VID_046D&PID_C54D&REV_1403&MI_00", "USB Input Device"));
        let b = device_group_key(&dev(r"USB\VID_046D&PID_C54D&REV_1403&MI_02", "USB Input Device"));
        let c = device_group_key(&dev(
            r"HID\VID_046D&PID_C54D&REV_1403&MI_01&Col02",
            "HID Keyboard Device",
        ));
        assert_eq!(a, b);
        assert_eq!(a, c, "sběrnice se liší, zařízení je totéž");

        // Druhá klávesnice se stejným obecným jménem — jiný model.
        let other = device_group_key(&dev(
            r"USB\VID_342D&PID_E40F&REV_2027&MI_00",
            "USB Input Device",
        ));
        assert_ne!(a, other, "dvě různá zařízení se sloučit nesmějí");

        // Myš Razer se hlásí přes tři vlastní sběrnice.
        let r1 = device_group_key(&dev(r"RAZER\VirtualBus\VID_1532&PID_0306", "Razer 0306 Device"));
        let r2 = device_group_key(&dev(r"RZCONTROL\VID_1532&PID_0306&MI_00", "Razer Control Device"));
        let r3 = device_group_key(&dev(
            r"RZVIRTUAL\VID_1532&PID_0306&MI_00&Col03",
            "HID-compliant mouse",
        ));
        assert_eq!(r1, r2);
        assert_eq!(r1, r3);

        // PCI/ACPI: model určuje dvojice VEN+DEV.
        let p1 = device_group_key(&dev(
            r"PCI\VEN_1022&DEV_1482&SUBSYS_14531022&REV_00",
            "PCI standard host CPU bridge",
        ));
        let p2 = device_group_key(&dev(r"PCI\VEN_1022&DEV_1482", "PCI standard host CPU bridge"));
        assert_eq!(p1, p2);
        let p3 = device_group_key(&dev(r"PCI\VEN_1022&DEV_1484", "PCI-to-PCI Bridge"));
        assert_ne!(p1, p3);

        // Bez VID/PID i VEN/DEV rozhoduje ID a jméno: pět zvukových
        // vstupů a výstupů sdílí ID, ale je to pět různých věcí.
        let e1 = device_group_key(&dev(r"MMDEVAPI\AudioEndpoints", "Reproduktory (Focusrite)"));
        let e2 = device_group_key(&dev(r"MMDEVAPI\AudioEndpoints", "MAG 341C OLED (NVIDIA)"));
        assert_ne!(e1, e2, "různé zvukové koncové body nejsou jedno zařízení");
        // Osm svazků se naopak jmenuje stejně a patří k sobě.
        let v1 = device_group_key(&dev(r"STORAGE\Volume", "Volume"));
        let v2 = device_group_key(&dev(r"STORAGE\Volume", "Volume"));
        assert_eq!(v1, v2);
    }

    // Skupina se pojmenuje podle toho, jak se hlásí samo zařízení —
    // ne podle obecného popisu od ovladače.
    #[test]
    fn group_name_prefers_what_the_device_calls_itself() {
        let mk = |hw: &str, name: &str, bus: &str| win_sys::devices::Device {
            name: name.into(),
            hardware_id: hw.into(),
            bus_desc: bus.into(),
            ..Default::default()
        };
        let raw = vec![
            // Zařízení samo (bez MI_/Col) → jeho jméno vyhrává.
            mk(r"USB\VID_046D&PID_C54D&REV_1403", "USB Composite Device", "USB Receiver"),
            mk(r"USB\VID_046D&PID_C54D&REV_1403&MI_00", "USB Input Device", ""),
            mk(r"HID\VID_046D&PID_C54D&MI_01&Col02", "HID Keyboard Device", ""),
        ];
        let keys: Vec<String> = raw.iter().map(device_group_key).collect();
        let names = group_names(&raw, &keys);
        assert_eq!(names[&keys[0]], "USB Receiver");

        // Když se zařízení nehlásí, vezme se aspoň jméno, které není
        // jen opakováním třídy.
        let raw2 = vec![
            mk(r"RZVIRTUAL\VID_1532&PID_0306&MI_00&Col03", "HID-compliant mouse", ""),
            mk(r"RAZER\VirtualBus\VID_1532&PID_0306", "Razer 0306 Device", ""),
        ];
        let keys2: Vec<String> = raw2.iter().map(device_group_key).collect();
        let names2 = group_names(&raw2, &keys2);
        assert_eq!(names2[&keys2[0]], "Razer 0306 Device");
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
