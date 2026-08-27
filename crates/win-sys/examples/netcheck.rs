//! Brána: celkový provoz se nesmí počítat vícekrát.
//!
//! NDIS filtry (QoS Packet Scheduler, WFP MAC Layer, Npcap, filtry
//! antivirů) se v GetIfTable2 tváří jako ethernetová rozhraní se stejným
//! ifType i stavem a nesou KOPII čítačů adaptéru, na kterém sedí.
//! Na vývojovém stroji jsou tři, takže se provoz sčítal čtyřikrát:
//! 52,7 GB místo 13,2 GB. Porovnává se součet, který vrací win-sys,
//! proti největšímu jednotlivému hardwarovému rozhraní.

use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

fn main() {
    let totals = match win_sys::net::net_totals() {
        Ok(t) => t,
        Err(e) => {
            println!("FAIL: net_totals selhalo: {e}");
            std::process::exit(1);
        }
    };

    // Referenční hodnota: největší hardwarové nefiltrové rozhraní.
    let mut best_rx = 0u64;
    let mut hw_rx = 0u64;
    let mut filters = 0usize;
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    unsafe {
        if GetIfTable2(&mut table).is_err() {
            println!("FAIL: GetIfTable2 selhalo");
            std::process::exit(1);
        }
        let count = (*table).NumEntries as usize;
        for r in std::slice::from_raw_parts((*table).Table.as_ptr(), count) {
            if r.Type == 24 || r.OperStatus.0 != 1 {
                continue;
            }
            let b = r.InterfaceAndOperStatusFlags._bitfield;
            if b & 2 != 0 {
                filters += 1;
                continue;
            }
            if b & 1 != 0 {
                hw_rx = hw_rx.saturating_add(r.InOctets);
                best_rx = best_rx.max(r.InOctets);
            }
        }
        FreeMibTable(table as *const _);
    }

    println!("  filtrových rozhraní: {filters}");
    println!("  součet z win-sys:    {} B rx", totals.rx_bytes);
    println!("  hardware bez filtrů: {hw_rx} B rx");

    // Obě čísla vznikla ze dvou různých čtení tabulky, mezi kterými
    // stihl protéct provoz — čítače jsou kumulativní a rostou pořád.
    // Porovnává se proto s tolerancí; hledá se násobek, ne bajt.
    // (Naměřeno: brána padala na rozdílu 66 bajtů.)
    let rozdil = totals.rx_bytes.abs_diff(hw_rx);
    let tolerance = (hw_rx / 100).max(1_000_000);
    if hw_rx > 0 && rozdil > tolerance {
        println!("FAIL: součet neodpovídá hardwarovým rozhraním (rozdíl {rozdil} B)");
        std::process::exit(1);
    }
    // Dvojnásobek nejsilnějšího rozhraní by znamenal, že se něco počítá
    // dvakrát; víc hardwarových adaptérů naráz je vzácné, ale možné,
    // proto se hlídá jen hrubý nepoměr.
    if best_rx > 0 && totals.rx_bytes > best_rx.saturating_mul(4) {
        println!("FAIL: součet je {}× větší než nejsilnější rozhraní", totals.rx_bytes / best_rx);
        std::process::exit(1);
    }
    println!("OK: síťový součet bez duplicit");
}
