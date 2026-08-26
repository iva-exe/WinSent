//! Diagnostika: co všechno GetIfTable2 vrací a s jakými čítači.
//!
//! Vzniklo kvůli tomu, že celkový provoz vycházel násobně vyšší, než
//! kolik šlo linkou. Viník: NDIS filtry (Npcap, VPN, antivirus) se
//! v tabulce tváří jako plnohodnotná ethernetová rozhraní a nesou
//! KOPII čítačů fyzického adaptéru. Poznají se jen podle bitfieldu.

use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

fn wide(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

fn main() {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    unsafe {
        if GetIfTable2(&mut table).is_err() {
            println!("GetIfTable2 selhalo");
            return;
        }
        let count = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), count);
        println!("{count} rozhraní\n");
        let mut rx_all = 0u64;
        let mut rx_hw = 0u64;
        for r in rows {
            let b = r.InterfaceAndOperStatusFlags._bitfield;
            let hw = b & 1 != 0;
            let filt = b & 2 != 0;
            let up = r.OperStatus.0 == 1;
            if up && r.Type != 24 {
                rx_all += r.InOctets;
                if hw && !filt {
                    rx_hw += r.InOctets;
                }
            }
            println!(
                "{:<42} typ={:<3} {:<5} hw={:<5} filtr={:<5} link={:>6} Mb/s  rx={:>14}  tx={:>14}",
                wide(&r.Alias),
                r.Type,
                if up { "up" } else { "down" },
                hw,
                filt,
                r.TransmitLinkSpeed / 1_000_000,
                r.InOctets,
                r.OutOctets
            );
        }
        println!("\nsoučet rx bez filtrace: {rx_all}");
        println!("součet rx jen hardware:  {rx_hw}");
        FreeMibTable(table as *const _);
    }
}
