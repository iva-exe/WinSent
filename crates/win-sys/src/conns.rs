//! Tabulky TCP/UDP spojení s vlastníkem (SPEC kap. 12.1).
//!
//! `GetExtendedTcpTable` / `GetExtendedUdpTable` s `*_OWNER_PID` —
//! každé spojení nese PID procesu, který ho drží. To je celý základ
//! sekce Network: napojit spojení na aplikace umí přesně tohle,
//! žádný driver ani sniffer není potřeba.
//!
//! Čte se snapshot (1×/s je levný, SPEC 12.3); obsah paketů se
//! nečte NIKDY — ukazujeme kam a kolik, ne co (SPEC 12.2).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
    MIB_UDP6TABLE_OWNER_PID, MIB_UDPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

/// Stav TCP spojení (MIB_TCP_STATE). Drží se jako text — UI ho jen
/// zobrazuje a čísla stavů jsou stabilní dokumentovaná konstanta.
fn tcp_state_str(state: u32) -> &'static str {
    match state {
        1 => "closed",
        2 => "listen",
        3 => "syn-sent",
        4 => "syn-received",
        5 => "established",
        6 => "fin-wait-1",
        7 => "fin-wait-2",
        8 => "close-wait",
        9 => "closing",
        10 => "last-ack",
        11 => "time-wait",
        12 => "delete-tcb",
        _ => "?",
    }
}

/// Jedno spojení nebo naslouchající port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conn {
    /// "tcp" | "udp".
    pub proto: &'static str,
    pub local: IpAddr,
    pub local_port: u16,
    /// U UDP a listening TCP chybí.
    pub remote: Option<IpAddr>,
    pub remote_port: u16,
    /// TCP stav; UDP nemá stavy — "udp".
    pub state: &'static str,
    pub pid: u32,
}

impl Conn {
    /// Naslouchá tenhle záznam (otevřená brána dovnitř)?
    pub fn listening(&self) -> bool {
        self.state == "listen" || (self.proto == "udp" && self.remote.is_none())
    }
}

/// Přečte kompletní snapshot: TCP + UDP, IPv4 + IPv6.
pub fn snapshot() -> Vec<Conn> {
    let mut out = Vec::new();
    tcp4(&mut out);
    tcp6(&mut out);
    udp4(&mut out);
    udp6(&mut out);
    out
}

/// Dvoufázové čtení tabulky dle kontraktu GetExtended*Table:
/// nejdřív velikost, pak data. Vrací syrový buffer.
macro_rules! read_table {
    ($fn:ident, $family:expr, $class:expr) => {{
        let mut size = 0u32;
        // SAFETY: dvoufázové volání dle dokumentace; buffer má velikost,
        // kterou API samo ohlásilo.
        unsafe {
            let _ = $fn(None, &mut size, false, $family.0 as u32, $class, 0);
            if size == 0 {
                None
            } else {
                let mut buf = vec![0u8; size as usize];
                let r = $fn(
                    Some(buf.as_mut_ptr() as *mut _),
                    &mut size,
                    false,
                    $family.0 as u32,
                    $class,
                    0,
                );
                (r == 0).then_some(buf)
            }
        }
    }};
}

fn tcp4(out: &mut Vec<Conn>) {
    let Some(buf) = read_table!(GetExtendedTcpTable, AF_INET, TCP_TABLE_OWNER_PID_ALL) else {
        return;
    };
    // SAFETY: buffer začíná MIB_TCPTABLE_OWNER_PID; položky leží hned
    // za polem dwNumEntries (layout dle SDK).
    unsafe {
        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        for r in rows {
            out.push(Conn {
                proto: "tcp",
                local: IpAddr::V4(Ipv4Addr::from(u32::from_be(r.dwLocalAddr))),
                local_port: (u32::from_be(r.dwLocalPort) >> 16) as u16,
                remote: (r.dwState != 2)
                    .then(|| IpAddr::V4(Ipv4Addr::from(u32::from_be(r.dwRemoteAddr)))),
                remote_port: (u32::from_be(r.dwRemotePort) >> 16) as u16,
                state: tcp_state_str(r.dwState),
                pid: r.dwOwningPid,
            });
        }
    }
}

fn tcp6(out: &mut Vec<Conn>) {
    let Some(buf) = read_table!(GetExtendedTcpTable, AF_INET6, TCP_TABLE_OWNER_PID_ALL) else {
        return;
    };
    // SAFETY: layout MIB_TCP6TABLE_OWNER_PID dle SDK.
    unsafe {
        let table = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        for r in rows {
            out.push(Conn {
                proto: "tcp",
                local: IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
                local_port: (u32::from_be(r.dwLocalPort) >> 16) as u16,
                remote: (r.dwState != 2).then(|| IpAddr::V6(Ipv6Addr::from(r.ucRemoteAddr))),
                remote_port: (u32::from_be(r.dwRemotePort) >> 16) as u16,
                state: tcp_state_str(r.dwState),
                pid: r.dwOwningPid,
            });
        }
    }
}

fn udp4(out: &mut Vec<Conn>) {
    let Some(buf) = read_table!(GetExtendedUdpTable, AF_INET, UDP_TABLE_OWNER_PID) else {
        return;
    };
    // SAFETY: layout MIB_UDPTABLE_OWNER_PID dle SDK.
    unsafe {
        let table = &*(buf.as_ptr() as *const MIB_UDPTABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        for r in rows {
            out.push(Conn {
                proto: "udp",
                local: IpAddr::V4(Ipv4Addr::from(u32::from_be(r.dwLocalAddr))),
                local_port: (u32::from_be(r.dwLocalPort) >> 16) as u16,
                remote: None,
                remote_port: 0,
                state: "udp",
                pid: r.dwOwningPid,
            });
        }
    }
}

fn udp6(out: &mut Vec<Conn>) {
    let Some(buf) = read_table!(GetExtendedUdpTable, AF_INET6, UDP_TABLE_OWNER_PID) else {
        return;
    };
    // SAFETY: layout MIB_UDP6TABLE_OWNER_PID dle SDK.
    unsafe {
        let table = &*(buf.as_ptr() as *const MIB_UDP6TABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        for r in rows {
            out.push(Conn {
                proto: "udp",
                local: IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
                local_port: (u32::from_be(r.dwLocalPort) >> 16) as u16,
                remote: None,
                remote_port: 0,
                state: "udp",
                pid: r.dwOwningPid,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Živý systém má vždy spojení: minimálně svchost naslouchá na
    // RPC portu 135 a DNS klient drží UDP porty.
    #[test]
    fn snapshot_sees_live_system() {
        let conns = snapshot();
        assert!(
            conns.len() > 10,
            "jen {} spojení — čtení selhalo",
            conns.len()
        );
        assert!(
            conns.iter().any(|c| c.listening()),
            "žádný naslouchající port — to na Windows nenastává"
        );
        // Každý záznam má vlastníka (pid 0 je jen TIME_WAIT zbytky).
        assert!(conns.iter().filter(|c| c.pid != 0).count() > 5);
    }
}
