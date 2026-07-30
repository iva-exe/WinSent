//! Zařazení zařízení do kategorií UI: skončilo všechno tam, kde to
//! uživatel čeká? `cargo run -p win-sys --example devcheck`
//!
//! Obrazovka Hardware dělí zařízení na hrubé kategorie podle toho, co
//! znamenají pro uživatele — ne podle tříd Windows. Tenhle výpis hlídá,
//! že na žádnou třídu nezapomínáme a nic nekončí v „Ostatní".

use std::collections::BTreeMap;

fn main() {
    let devs = win_sys::devices::devices();
    println!("zařízení celkem: {}\n", devs.len());

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut other: Vec<&win_sys::devices::Device> = Vec::new();
    for d in &devs {
        let cat = category(&d.class, &d.class_desc, &d.hardware_id.to_uppercase());
        *counts.entry(cat).or_default() += 1;
        if cat == "Ostatní zařízení" {
            other.push(d);
        }
    }
    for (cat, n) in &counts {
        println!("{cat:22} {n}");
    }
    if other.is_empty() {
        println!("\nOK  nic nezůstalo nezařazené");
    } else {
        println!("\n!!  nezařazená ({}):", other.len());
        for d in &other {
            println!("   [{}] {} — {}", d.class, d.name, d.class_desc);
        }
    }

    let problems: Vec<_> = devs.iter().filter(|d| d.has_problem()).collect();
    println!("\nzařízení s problémem: {}", problems.len());
    for p in &problems {
        println!("   {} — kód {}", p.name, p.problem_code);
    }
}

/// Stejné dělení jako obrazovka Hardware.
fn category(class: &str, class_desc: &str, hwid: &str) -> &'static str {
    match class {
        // Procesor, grafika a disky mají v UI vlastní řádky nahoře.
        "Processor" | "Display" | "DiskDrive" => "Komponenty",
        "Monitor" => "Zobrazení",
        "Keyboard" | "Mouse" | "HIDClass" | "WPD" | "Image" | "Camera" | "Bluetooth"
        | "Biometric" => "Periferie",
        "MEDIA" | "AudioEndpoint" | "AudioProcessingObject" => "Zvuková zařízení",
        "Net" => "Síť",
        "USB" | "HDC" | "SCSIAdapter" | "Ports" | "Volume" | "FloppyDisk" => "Řadiče a porty",
        "PrintQueue" | "Printer" | "PrinterPort" => "Tisk",
        "System" | "Computer" | "Firmware" | "SoftwareDevice" | "SecurityDevices" => {
            "Systémová zařízení"
        }
        // Výrobci si zakládají vlastní třídy („Focusrite Audio",
        // „Razer Device"). Rozhodne název třídy, pak sběrnice: co visí
        // na HID nebo USB, je z pohledu uživatele periferie.
        _ => {
            let c = format!("{class} {class_desc}").to_lowercase();
            if c.contains("audio") || c.contains("zvuk") {
                "Zvuková zařízení"
            } else if c.contains("net") || c.contains("síť") {
                "Síť"
            } else if hwid.contains("VID_") && hwid.contains("PID_") {
                // Vlastní sběrnice výrobců (RAZER\, RZCONTROL\…) mají
                // pořád VID/PID — pořízené přes USB, tedy periferie.
                "Periferie"
            } else {
                "Ostatní zařízení"
            }
        }
    }
}
