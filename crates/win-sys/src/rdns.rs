//! Reverzní DNS — „kam to spojení vlastně vede" (SPEC kap. 12.1).
//!
//! `GetNameInfoW` s NI_NAMEREQD: buď skutečné PTR jméno, nebo nic.
//! Bez jména se ukáže IP — nikdy se nic nedomýšlí.
//!
//! Volání umí blokovat i sekundy, proto ho NIKDY nevolá obslužné
//! vlákno IPC — resolver běží na pozadí a výsledky se cachují
//! (SPEC 12.3: reverzní DNS cachuj jako podpisy).

use std::net::{IpAddr, SocketAddr};

use windows::Win32::Networking::WinSock::{
    GetNameInfoW, WSAStartup, NI_MAXHOST, NI_NAMEREQD, SOCKADDR_STORAGE, WSADATA,
};

/// Winsock se inicializuje jednou na proces — bez WSAStartup vrací
/// GetNameInfoW WSANOTINITIALISED a nikdy nic nepřeloží.
fn init_winsock() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut data = WSADATA::default();
        // SAFETY: standardní inicializace, verze 2.2.
        let _ = unsafe { WSAStartup(0x0202, &mut data) };
    });
}

/// Přeloží IP na PTR jméno. `None` = záznam neexistuje (běžné) nebo
/// selhal lookup — obojí znamená „ukaž IP".
pub fn resolve(ip: IpAddr) -> Option<String> {
    // Loopback a nespecifikované adresy nemají smysl překládat.
    if ip.is_loopback() || ip.is_unspecified() {
        return None;
    }
    init_winsock();
    let sa = SocketAddr::new(ip, 0);
    // SOCKADDR_STORAGE je největší sockaddr — vejde se v4 i v6.
    let mut storage = SOCKADDR_STORAGE::default();
    let len = sockaddr_from(&sa, &mut storage);

    let mut host = [0u16; NI_MAXHOST as usize];
    // SAFETY: storage je vyplněná na `len` bajtů, buffer má pevnou
    // velikost NI_MAXHOST dle kontraktu API.
    let r = unsafe {
        GetNameInfoW(
            &storage as *const _ as *const _,
            windows::Win32::Networking::WinSock::socklen_t(len),
            Some(&mut host),
            None,
            NI_NAMEREQD as i32,
        )
    };
    if r != 0 {
        return None;
    }
    let end = host.iter().position(|&c| c == 0).unwrap_or(host.len());
    let name = String::from_utf16_lossy(&host[..end]).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Vyplní SOCKADDR_STORAGE z Rust adresy; vrací délku struktury.
fn sockaddr_from(sa: &SocketAddr, storage: &mut SOCKADDR_STORAGE) -> i32 {
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6};
    // SAFETY: zápis do unie přes ukazatele na správně velké struktury.
    unsafe {
        match sa {
            SocketAddr::V4(v4) => {
                let dst = storage as *mut _ as *mut SOCKADDR_IN;
                (*dst).sin_family = AF_INET;
                (*dst).sin_port = v4.port().to_be();
                (*dst).sin_addr.S_un.S_addr = u32::from(*v4.ip()).to_be();
                std::mem::size_of::<SOCKADDR_IN>() as i32
            }
            SocketAddr::V6(v6) => {
                let dst = storage as *mut _ as *mut SOCKADDR_IN6;
                (*dst).sin6_family = AF_INET6;
                (*dst).sin6_port = v6.port().to_be();
                (*dst).sin6_addr.u.Byte = v6.ip().octets();
                std::mem::size_of::<SOCKADDR_IN6>() as i32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Loopback se nepřekládá — rovnou None, žádné volání DNS.
    #[test]
    fn loopback_is_not_resolved() {
        assert!(resolve("127.0.0.1".parse().unwrap()).is_none());
        assert!(resolve("::1".parse().unwrap()).is_none());
    }

    // Veřejný resolver Cloudflare má stabilní PTR záznam. Test síť
    // potřebuje; bez ní projde jako None (žádné jméno ≠ chyba).
    #[test]
    fn known_public_ip_resolves_or_none() {
        if let Some(name) = resolve("1.1.1.1".parse().unwrap()) {
            assert!(name.contains('.'), "podivné PTR jméno: {name}");
        }
    }
}
