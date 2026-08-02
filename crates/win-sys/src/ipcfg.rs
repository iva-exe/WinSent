//! Síťové adaptéry a jejich IP konfigurace (v9, sekce Connection).
//!
//! `GetAdaptersAddresses` — tentýž zdroj jako `ipconfig /all`: jméno,
//! MAC, stav, rychlost linky, adresy, brána, DNS, DHCP. Jen čtení;
//! nastavení sítě tenhle nástroj nemění.

use std::net::IpAddr;

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, IP_ADAPTER_ADDRESSES_LH,
    IP_ADAPTER_DHCP_ENABLED,
};
use windows::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6,
};

/// Jeden síťový adaptér, jak ho vidí systém.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Adapter {
    /// Přátelské jméno („Ethernet", „Wi-Fi").
    pub name: String,
    /// Popis hardwaru („Realtek PCIe GBE Family Controller").
    pub description: String,
    /// MAC adresa „AA:BB:…"; virtuální adaptéry ji mít nemusí.
    pub mac: String,
    /// "ethernet" | "wifi" | "loopback" | "virtual" | "other".
    pub kind: &'static str,
    /// Je linka nahoře (OperStatus == Up)?
    pub up: bool,
    /// Rychlost linky v Mb/s (0 = nehlásí).
    pub link_mbps: u64,
    pub ips: Vec<IpAddr>,
    pub gateways: Vec<IpAddr>,
    pub dns: Vec<IpAddr>,
    pub dhcp: bool,
}

/// Přečte všechny adaptéry kromě loopbacku.
pub fn adapters() -> Vec<Adapter> {
    // Dvoufázově dle kontraktu API: velikost → data. Buffer začíná
    // na 16 kB, což obvykle stačí na první pokus.
    let mut size = 16 * 1024u32;
    let mut buf: Vec<u8>;
    // SAFETY: buffer má velikost hlášenou API; linked list se čte jen
    // v mezích tohoto bufferu.
    unsafe {
        loop {
            buf = vec![0u8; size as usize];
            let rc = WIN32_ERROR(GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                GAA_FLAG_INCLUDE_GATEWAYS,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            ));
            if rc == ERROR_SUCCESS {
                break;
            }
            if rc != ERROR_BUFFER_OVERFLOW {
                return Vec::new();
            }
        }
        let mut out = Vec::new();
        let mut node = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !node.is_null() {
            let a = &*node;
            let kind = match a.IfType {
                6 => "ethernet",
                71 => "wifi",
                24 => "loopback",
                // Tunelové/virtuální (Teredo, VPN, Hyper-V…).
                131 | 53 => "virtual",
                _ => "other",
            };
            if kind != "loopback" {
                let mac_len = a.PhysicalAddressLength as usize;
                let mac = a.PhysicalAddress[..mac_len.min(8)]
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(":");
                out.push(Adapter {
                    name: a.FriendlyName.to_string().unwrap_or_default(),
                    description: a.Description.to_string().unwrap_or_default(),
                    mac,
                    kind,
                    // OperStatus 1 = IfOperStatusUp.
                    up: a.OperStatus.0 == 1,
                    link_mbps: if a.TransmitLinkSpeed == u64::MAX {
                        0
                    } else {
                        a.TransmitLinkSpeed / 1_000_000
                    },
                    ips: collect_sockaddrs(a.FirstUnicastAddress as *const u8, |p| {
                        // IP_ADAPTER_UNICAST_ADDRESS_LH: Next @8, Address @16.
                        (p.add(8), p.add(16))
                    }),
                    gateways: collect_sockaddrs(a.FirstGatewayAddress as *const u8, |p| {
                        (p.add(8), p.add(16))
                    }),
                    dns: collect_sockaddrs(a.FirstDnsServerAddress as *const u8, |p| {
                        (p.add(8), p.add(16))
                    }),
                    dhcp: (a.Anonymous2.Flags & IP_ADAPTER_DHCP_ENABLED) != 0,
                });
            }
            node = a.Next;
        }
        out.sort_by(|a, b| {
            // Aktivní fyzické adaptéry první, virtuální dolů.
            let rank = |x: &Adapter| match (x.up, x.kind) {
                (true, "ethernet" | "wifi") => 0,
                (true, _) => 1,
                (false, "ethernet" | "wifi") => 2,
                _ => 3,
            };
            rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
        });
        out
    }
}

/// Projde linked list `IP_ADAPTER_*_ADDRESS` struktur a vytáhne IP
/// adresy. `fields` vrací (ukazatel na Next, ukazatel na SOCKET_ADDRESS)
/// — layout mají tyhle struktury shodný.
///
/// # Safety
/// `head` musí ukazovat do platného bufferu GetAdaptersAddresses.
unsafe fn collect_sockaddrs(
    head: *const u8,
    fields: impl Fn(*const u8) -> (*const u8, *const u8),
) -> Vec<IpAddr> {
    let mut out = Vec::new();
    let mut node = head;
    let mut guard = 0;
    while !node.is_null() && guard < 64 {
        guard += 1;
        let (next_ptr, sockaddr_ptr) = fields(node);
        // SOCKET_ADDRESS: lpSockaddr *mut SOCKADDR, iSockaddrLength i32.
        let sa = *(sockaddr_ptr as *const *const u8);
        if let Some(ip) = sockaddr_to_ip(sa) {
            out.push(ip);
        }
        node = *(next_ptr as *const *const u8);
    }
    out
}

/// sockaddr → IpAddr podle rodiny.
///
/// # Safety
/// `sa` musí ukazovat na platný sockaddr, nebo být null.
unsafe fn sockaddr_to_ip(sa: *const u8) -> Option<IpAddr> {
    if sa.is_null() {
        return None;
    }
    let family = *(sa as *const u16);
    if family == AF_INET.0 {
        let v4 = &*(sa as *const SOCKADDR_IN);
        Some(IpAddr::V4(std::net::Ipv4Addr::from(u32::from_be(
            v4.sin_addr.S_un.S_addr,
        ))))
    } else if family == AF_INET6.0 {
        let v6 = &*(sa as *const SOCKADDR_IN6);
        Some(IpAddr::V6(std::net::Ipv6Addr::from(v6.sin6_addr.u.Byte)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Živý stroj má aspoň jeden adaptér a ten aktivní má IP + bránu.
    #[test]
    fn active_adapter_has_ip_and_gateway() {
        let list = adapters();
        assert!(!list.is_empty(), "žádný adaptér — čtení selhalo");
        let active = list
            .iter()
            .find(|a| a.up && (a.kind == "ethernet" || a.kind == "wifi"));
        if let Some(a) = active {
            assert!(!a.ips.is_empty(), "aktivní adaptér bez IP adresy");
            assert!(!a.dns.is_empty(), "aktivní adaptér bez DNS serverů");
            assert!(!a.mac.is_empty(), "fyzický adaptér bez MAC");
        }
    }
}
