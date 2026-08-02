//! WiFi přes WlanAPI (SPEC kap. 12.1) — JEN ČTENÍ.
//!
//! Seznam rozhraní, aktuální připojení (SSID, síla signálu, rychlosti)
//! a viditelné sítě z poslední cache skenování. Připojení/zapomenutí
//! sítě je správa — ta by šla přes validační vrstvu za potvrzením;
//! tenhle modul systém nikdy nemění.
//!
//! Stroj bez WiFi adaptéru vrátí prázdný seznam rozhraní — sekce to
//! řekne narovinu, žádné předstírání.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    wlan_intf_opcode_current_connection, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory,
    WlanGetAvailableNetworkList, WlanOpenHandle, WlanQueryInterface, WLAN_AVAILABLE_NETWORK_LIST,
    WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
};

/// Aktuální připojení jednoho WiFi rozhraní.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiConnection {
    pub ssid: String,
    /// Kvalita signálu 0–100, jak ji hlásí ovladač.
    pub signal_pct: u32,
    /// Sjednané rychlosti v Mb/s.
    pub rx_mbps: u32,
    pub tx_mbps: u32,
    pub secured: bool,
}

/// WiFi rozhraní (fyzická karta).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiInterface {
    pub description: String,
    /// Aktuální připojení; None = karta je, ale nepřipojená.
    pub connection: Option<WifiConnection>,
}

/// Viditelná síť z poslední cache skenování systému.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal_pct: u32,
    pub secured: bool,
    pub connected: bool,
}

/// Přečte WiFi rozhraní a viditelné sítě: (rozhraní, sítě).
/// Prázdná rozhraní = stroj WiFi nemá (běžný desktop).
pub fn snapshot() -> (Vec<WifiInterface>, Vec<WifiNetwork>) {
    let mut ifaces = Vec::new();
    let mut networks = Vec::new();

    // SAFETY: handle se vždy zavírá; každá alokace WlanAPI se uvolňuje
    // přes WlanFreeMemory; čte se jen v mezích hlášených počtů.
    unsafe {
        let mut version = 0u32;
        let mut handle = HANDLE::default();
        if WlanOpenHandle(2, None, &mut version, &mut handle) != 0 {
            return (ifaces, networks);
        }

        let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(handle, None, &mut list_ptr) == 0 && !list_ptr.is_null() {
            let list = &*list_ptr;
            let items = std::slice::from_raw_parts(
                list.InterfaceInfo.as_ptr(),
                list.dwNumberOfItems as usize,
            );
            for info in items {
                let mut iface = WifiInterface {
                    description: String::from_utf16_lossy(
                        &info.strInterfaceDescription[..info
                            .strInterfaceDescription
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(info.strInterfaceDescription.len())],
                    ),
                    connection: None,
                };

                // Aktuální připojení — jen když je karta připojená
                // (jinak query vrací chybu, což je v pořádku).
                let mut size = 0u32;
                let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
                if WlanQueryInterface(
                    handle,
                    &info.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut size,
                    &mut data,
                    None,
                ) == 0
                    && !data.is_null()
                {
                    let attrs = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
                    let assoc = &attrs.wlanAssociationAttributes;
                    let ssid_len = assoc.dot11Ssid.uSSIDLength as usize;
                    iface.connection = Some(WifiConnection {
                        ssid: String::from_utf8_lossy(&assoc.dot11Ssid.ucSSID[..ssid_len.min(32)])
                            .into_owned(),
                        signal_pct: assoc.wlanSignalQuality,
                        rx_mbps: assoc.ulRxRate / 1000,
                        tx_mbps: assoc.ulTxRate / 1000,
                        secured: attrs.wlanSecurityAttributes.bSecurityEnabled.as_bool(),
                    });
                    WlanFreeMemory(data);
                }

                // Viditelné sítě z cache — bez vyvolání nového skenu
                // (flags 0), ať čtení nic nerozpohybuje.
                let mut nets_ptr: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
                if WlanGetAvailableNetworkList(handle, &info.InterfaceGuid, 0, None, &mut nets_ptr)
                    == 0
                    && !nets_ptr.is_null()
                {
                    let nets = &*nets_ptr;
                    let rows = std::slice::from_raw_parts(
                        nets.Network.as_ptr(),
                        nets.dwNumberOfItems as usize,
                    );
                    for n in rows {
                        let ssid_len = n.dot11Ssid.uSSIDLength as usize;
                        let ssid = String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..ssid_len.min(32)])
                            .into_owned();
                        if ssid.is_empty() {
                            continue;
                        }
                        // Táž síť bývá v seznamu vícekrát (profily +
                        // BSS typy) — drží se nejsilnější výskyt.
                        let connected = n.dwFlags & 1 != 0; // WLAN_AVAILABLE_NETWORK_CONNECTED
                        match networks
                            .iter_mut()
                            .find(|w: &&mut WifiNetwork| w.ssid == ssid)
                        {
                            Some(w) => {
                                w.signal_pct = w.signal_pct.max(n.wlanSignalQuality);
                                w.connected |= connected;
                            }
                            None => networks.push(WifiNetwork {
                                ssid,
                                signal_pct: n.wlanSignalQuality,
                                secured: n.bSecurityEnabled.as_bool(),
                                connected,
                            }),
                        }
                    }
                    WlanFreeMemory(nets_ptr as *mut _);
                }
                ifaces.push(iface);
            }
            WlanFreeMemory(list_ptr as *mut _);
        }
        let _ = WlanCloseHandle(handle, None);
    }
    networks.sort_by_key(|n| std::cmp::Reverse(n.signal_pct));
    (ifaces, networks)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Na stroji bez WiFi prázdno (poctivé), s WiFi konzistentní data.
    #[test]
    fn snapshot_is_honest() {
        let (ifaces, networks) = snapshot();
        if ifaces.is_empty() {
            assert!(networks.is_empty(), "sítě bez rozhraní nedávají smysl");
        }
        for n in &networks {
            assert!(n.signal_pct <= 100, "signál přes 100 %: {}", n.signal_pct);
        }
    }
}
