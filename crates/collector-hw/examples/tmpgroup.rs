//! DOČASNÉ ověření slučování na živých datech.
fn main() {
    let rows = collector_hw::devices();
    let mut by: std::collections::BTreeMap<String, Vec<&core_types::proc::DeviceRow>> =
        Default::default();
    for r in &rows {
        by.entry(r.group_key.clone()).or_default().push(r);
    }
    println!("radku ze systemu: {}  skupin: {}", rows.len(), by.len());
    let mut v: Vec<_> = by.iter().collect();
    v.sort_by_key(|(_, m)| std::cmp::Reverse(m.len()));
    for (k, m) in v.iter().filter(|(_, m)| m.len() > 1) {
        let drivers: std::collections::BTreeSet<_> =
            m.iter().map(|r| r.driver_version.clone()).collect();
        let mfg: std::collections::BTreeSet<_> = m.iter().map(|r| r.manufacturer.clone()).collect();
        let probs = m.iter().filter(|r| r.problem_code != 0).count();
        println!(
            "{:3}x {:38} nazev='{}' ovladacu={} vyrobcu={} problemu={}",
            m.len(),
            k,
            m[0].group_name,
            drivers.len(),
            mfg.len(),
            probs
        );
        for r in m.iter() {
            println!("        [{}] {} | {}", r.class, r.name, r.hardware_id);
        }
    }
}
