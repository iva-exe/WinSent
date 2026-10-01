//! Celkový síťový provoz: kumulativní bajty přes GetIfTable2.
//! Levné dokumentované API — deltu na bps počítá kolektor přes
//! [`NetMeter`].

use std::collections::HashMap;

use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

use crate::Error;

/// Kumulativní součty bajtů přes všechna fyzická rozhraní (bez
/// loopbacku a bez rozhraní, která nejsou v provozu).
#[derive(Debug, Clone, Copy, Default)]
pub struct NetTotals {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

// IANA ifType software loopback (MIB_IF_ROW2.Type).
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
// MIB_IF_ROW2.OperStatus — IfOperStatusUp.
const IF_OPER_STATUS_UP: i32 = 1;
// MIB_IF_ROW2.InterfaceAndOperStatusFlags — bit 0 HardwareInterface,
// bit 1 FilterInterface.
const FLAG_HARDWARE: u8 = 1;
const FLAG_FILTER: u8 = 2;

/// Čítače jednoho rozhraní (bez loopbacku a NDIS filtrů).
#[derive(Debug, Clone, Copy)]
struct IfCounter {
    /// InterfaceLuid — stabilní identita rozhraní mezi čteními.
    luid: u64,
    rx: u64,
    tx: u64,
    hardware: bool,
    up: bool,
}

/// Přečte čítače všech nesmyčkových nefiltrových rozhraní, i těch,
/// která právě nejsou v provozu (čítač dole stojícího rozhraní neroste,
/// ale jeho identitu potřebuje [`NetMeter`]).
fn if_counters() -> Result<Vec<IfCounter>, Error> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 alokuje tabulku, párové uvolnění FreeMibTable.
    unsafe {
        let status = GetIfTable2(&mut table);
        if status.is_err() {
            return Err(Error::Win32 {
                call: "GetIfTable2",
                code: status.0 as i32,
            });
        }

        let count = (*table).NumEntries as usize;
        // Řádky leží inline za hlavičkou tabulky.
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), count);
        let mut out = Vec::with_capacity(count);
        for row in rows {
            if row.Type == IF_TYPE_SOFTWARE_LOOPBACK {
                continue;
            }
            let flags = row.InterfaceAndOperStatusFlags._bitfield;
            // NDIS filtry (QoS Packet Scheduler, WFP MAC Layer, Npcap,
            // antivirové filtry) se v tabulce tváří jako ethernetová
            // rozhraní se stejným ifType i stavem — a nesou KOPII čítačů
            // adaptéru, na kterém sedí. Na tomhle stroji jsou tři, takže
            // se celkový provoz počítal čtyřikrát: 52,7 GB místo 13,2 GB.
            // Rozliší je jen bitfield.
            if flags & FLAG_FILTER != 0 {
                continue;
            }
            out.push(IfCounter {
                luid: row.InterfaceLuid.Value,
                rx: row.InOctets,
                tx: row.OutOctets,
                hardware: flags & FLAG_HARDWARE != 0,
                up: row.OperStatus.0 == IF_OPER_STATUS_UP,
            });
        }
        FreeMibTable(table as *const _);
        Ok(out)
    }
}

/// Sečte InOctets/OutOctets všech aktivních nesmyčkových rozhraní.
///
/// Okamžitý snímek, NE monotónní čítač: rozhraní, které spadne, ze
/// součtu zmizí a po návratu se vrátí s celým svým čítačem. Na rychlost
/// je proto [`NetMeter`]; tohle zůstává pro bránu `netcheck`.
pub fn net_totals() -> Result<NetTotals, Error> {
    let counters = if_counters()?;
    let mut totals = NetTotals::default();
    // Záchranný součet pro případ, že se žádné rozhraní neoznačí za
    // hardwarové (virtuální stroje, exotické ovladače). Lepší číslo
    // s filtry než nula.
    let mut fallback = NetTotals::default();
    for c in counters.iter().filter(|c| c.up) {
        // Virtuální adaptéry (Wi-Fi Direct, tunely, VPN) se přeskočí —
        // jejich provoz už prošel fyzickou linkou a započítal by se
        // podruhé.
        fallback.rx_bytes = fallback.rx_bytes.saturating_add(c.rx);
        fallback.tx_bytes = fallback.tx_bytes.saturating_add(c.tx);
        if !c.hardware {
            continue;
        }
        totals.rx_bytes = totals.rx_bytes.saturating_add(c.rx);
        totals.tx_bytes = totals.tx_bytes.saturating_add(c.tx);
    }
    if totals.rx_bytes == 0 && totals.tx_bytes == 0 {
        return Ok(fallback);
    }
    Ok(totals)
}

/// Měřič provozu mezi dvěma čteními — delty se počítají PO ROZHRANÍCH.
///
/// Dřív kolektor odečítal dva součty [`net_totals`]. Ten ale není
/// monotónní: když Wi-Fi na pár sekund vypadla, součet spadl na
/// fallback (pár MB z virtuálních rozhraní) a po návratu skočil zpět
/// o celý kumulativní čítač adaptéru — do grafu i do minutových
/// průměrů se zapsalo „5 GB/s". Tady má každé rozhraní vlastní
/// předchozí hodnotu; rozhraní, které nově přibylo nebo se vrátilo,
/// dá v prvním vzorku 0 a vynulovaný čítač také 0.
#[derive(Debug, Default)]
pub struct NetMeter {
    prev: HashMap<u64, (u64, u64)>,
}

impl NetMeter {
    /// Přečte výchozí stav čítačů.
    pub fn new() -> Result<NetMeter, Error> {
        Ok(NetMeter {
            prev: snapshot(&if_counters()?),
        })
    }

    /// Bajty přenesené od minulého volání (ne rychlost — dělí volající
    /// skutečnou dobou). Při chybě se předchozí stav nemění.
    pub fn delta(&mut self) -> Result<NetTotals, Error> {
        let cur = if_counters()?;
        let d = sum_deltas(&self.prev, &cur);
        self.prev = snapshot(&cur);
        Ok(d)
    }
}

fn snapshot(cur: &[IfCounter]) -> HashMap<u64, (u64, u64)> {
    cur.iter().map(|c| (c.luid, (c.rx, c.tx))).collect()
}

/// Součet kladných delt rozhraní známých z minula. Sčítají se
/// hardwarová rozhraní; jen když žádné hardwarové není V PROVOZU (VM,
/// exotický ovladač USB Wi-Fi nebo tetheringu), sčítají se všechna
/// nefiltrová. Rozhoduje stav Up: notebook má skoro vždy hardwarový
/// Ethernet nebo Bluetooth PAN, který je dole — kdyby stačila jeho
/// přítomnost, sčítal by se jen on a provoz přes rozhraní bez
/// hardwarového bitu by zůstal trvale na nule. Přepnutí sady skok
/// nezpůsobí: delty se počítají po rozhraních proti minulému stavu.
fn sum_deltas(prev: &HashMap<u64, (u64, u64)>, cur: &[IfCounter]) -> NetTotals {
    let any_hw = cur.iter().any(|c| c.hardware && c.up);
    let mut out = NetTotals::default();
    for c in cur.iter().filter(|c| c.hardware || !any_hw) {
        if let Some(&(rx, tx)) = prev.get(&c.luid) {
            out.rx_bytes = out.rx_bytes.saturating_add(c.rx.saturating_sub(rx));
            out.tx_bytes = out.tx_bytes.saturating_add(c.tx.saturating_sub(tx));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(luid: u64, rx: u64, hardware: bool) -> IfCounter {
        IfCounter { luid, rx, tx: rx / 10, hardware, up: true }
    }

    // USB Wi-Fi bez hardwarového bitu + odpojený Ethernet (hardwarový,
    // dole): provoz přes Wi-Fi se musí započítat.
    #[test]
    fn hardwarove_rozhrani_dole_neschova_provoz() {
        let eth_dole = IfCounter { up: false, ..c(1, 1_000, true) };
        let prev = snapshot(&[eth_dole, c(5, 100, false)]);
        let cur = [IfCounter { up: false, ..c(1, 1_000, true) }, c(5, 600, false)];
        assert_eq!(sum_deltas(&prev, &cur).rx_bytes, 500);
    }

    // Wi-Fi vypadne a vrátí se: návrat nesmí přičíst celý čítač.
    #[test]
    fn interface_coming_back_adds_nothing() {
        let wifi = 5_000_000_000;
        let mut prev = snapshot(&[c(1, wifi, true), c(9, 1_000, false)]);
        // Wi-Fi z tabulky zmizela (nebo spadla) — zbyl virtuál.
        let cur = [c(9, 2_000, false)];
        let d = sum_deltas(&prev, &cur);
        assert_eq!(d.rx_bytes, 1_000);
        prev = snapshot(&cur);
        // Wi-Fi je zpět s celým kumulativním čítačem.
        let cur = [c(1, wifi + 500, true), c(9, 3_000, false)];
        assert_eq!(sum_deltas(&prev, &cur).rx_bytes, 0);
        prev = snapshot(&cur);
        let cur = [c(1, wifi + 1_500, true), c(9, 4_000, false)];
        assert_eq!(sum_deltas(&prev, &cur).rx_bytes, 1_000);
    }

    // Rozhraní dole zůstává v sadě — pád druhého adaptéru nesmí
    // zahodit provoz prvního.
    #[test]
    fn down_interface_keeps_others_counted() {
        let prev = snapshot(&[c(1, 1_000, true), c(2, 7_000, true)]);
        let mut eth_down = c(2, 7_000, true);
        eth_down.up = false;
        let d = sum_deltas(&prev, &[c(1, 3_000, true), eth_down]);
        assert_eq!(d.rx_bytes, 2_000);
    }

    // Vynulovaný čítač (reinicializace ovladače) dá 0, ne obří číslo.
    #[test]
    fn reset_counter_gives_zero() {
        let prev = snapshot(&[c(1, 9_000, true)]);
        assert_eq!(sum_deltas(&prev, &[c(1, 100, true)]).rx_bytes, 0);
    }

    // Bez hardwarových rozhraní se počítají virtuální.
    #[test]
    fn falls_back_to_virtual_when_no_hardware() {
        let prev = snapshot(&[c(5, 100, false)]);
        assert_eq!(sum_deltas(&prev, &[c(5, 600, false)]).rx_bytes, 500);
    }
}
