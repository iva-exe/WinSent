//! Brána: model fyzického disku se čte ze správných offsetů.
//!
//! STORAGE_DEVICE_DESCRIPTOR má VendorIdOffset až na bajtu 12 —
//! před ním leží čtyři jednobajtové příznaky. Čtení od 8 a 12 bylo
//! o čtyři bajty vedle, takže se jako model bral vendor a v záznamu
//! stálo u disku jen „NVMe" nebo vůbec nic.

fn main() {
    let disks = win_sys::disk::open_disks();
    if disks.is_empty() {
        println!("OK: žádný fyzický disk k otevření (nic k ověření)");
        return;
    }
    let mut bad = 0;
    for d in &disks {
        let h = win_sys::smart::nvme_health(d.index);
        println!(
            "  disk {} model={:?} smart={}",
            d.index,
            d.model,
            if h.is_some() { "ano" } else { "ne" }
        );
        // Model složený z jediného slova jako „NVMe" nebo prázdný
        // znamená, že se přečetl špatný kus deskriptoru.
        let m = d.model.trim();
        if m.is_empty() || m.eq_ignore_ascii_case("NVMe") || m.len() < 5 {
            println!("  CHYBA: model disku {} vypadá uříznutě: {m:?}", d.index);
            bad += 1;
        }
    }
    if bad > 0 {
        println!("FAIL: {bad} disků bez použitelného modelu");
        std::process::exit(1);
    }
    println!("OK: modely disků čitelné ({} disků)", disks.len());
}
