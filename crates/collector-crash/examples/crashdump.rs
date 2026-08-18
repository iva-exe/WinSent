//! Výpis pádů tak, jak je uvidí uživatel. Slouží k ověření překladu
//! na skutečných hlášeních konkrétního stroje.
fn main() {
    let crashes = collector_crash::report::app_crashes(12);
    println!("pádů v protokolu: {}\n", crashes.len());
    for c in &crashes {
        let (s, d) = collector_crash::report::describe(c, &[]);
        println!("── {s}");
        for line in d.lines() {
            println!("   {line}");
        }
        println!();
    }
}
