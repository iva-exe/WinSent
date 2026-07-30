//! Seznam zařízení: `cargo run -p win-sys --example devcheck`.

use std::collections::BTreeMap;

fn main() {
    let devs = win_sys::devices::devices();
    println!("zařízení celkem: {}\n", devs.len());

    let mut by_class: BTreeMap<String, Vec<&win_sys::devices::Device>> = BTreeMap::new();
    for d in &devs {
        by_class
            .entry(if d.class_desc.is_empty() {
                d.class.clone()
            } else {
                d.class_desc.clone()
            })
            .or_default()
            .push(d);
    }
    for (class, list) in &by_class {
        println!("── {class} ({})", list.len());
        for d in list.iter().take(4) {
            println!(
                "   {} | {} | ovladač {} ({}){}",
                d.name,
                d.manufacturer,
                if d.driver_version.is_empty() {
                    "—"
                } else {
                    &d.driver_version
                },
                if d.driver_date.is_empty() {
                    "—"
                } else {
                    &d.driver_date
                },
                if d.has_problem() {
                    format!("  ⚠ problém {}", d.problem_code)
                } else {
                    String::new()
                }
            );
        }
        if list.len() > 4 {
            println!("   … a další {}", list.len() - 4);
        }
    }

    // Obrazovky se sem nevypisují — jsou vázané na relaci a čte je
    // UI proces (viz ui/src-tauri/src/display.rs).
}
