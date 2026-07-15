//! Celkový síťový provoz: kumulativní bajty přes GetIfTable2.
//! Levné dokumentované API — deltu na bps počítá kolektor.

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

/// Sečte InOctets/OutOctets všech aktivních nesmyčkových rozhraní.
pub fn net_totals() -> Result<NetTotals, Error> {
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

        let mut totals = NetTotals::default();
        let count = (*table).NumEntries as usize;
        // Řádky leží inline za hlavičkou tabulky.
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), count);
        for row in rows {
            if row.Type == IF_TYPE_SOFTWARE_LOOPBACK || row.OperStatus.0 != IF_OPER_STATUS_UP {
                continue;
            }
            totals.rx_bytes = totals.rx_bytes.saturating_add(row.InOctets);
            totals.tx_bytes = totals.tx_bytes.saturating_add(row.OutOctets);
        }
        FreeMibTable(table as *const _);
        Ok(totals)
    }
}
