//! collector-net — spojení per aplikace (v9, SPEC kap. 12).
//!
//! Snapshot TCP/UDP tabulek (win-sys::conns) spojený s identitou
//! aplikací: každé spojení nese PID, PID nese identity_key (kap. 4),
//! a UI pak umí říct „Chrome má 40 spojení, kam vedou" — což Task
//! Manager per aplikace neumí.
//!
//! Čtecí crate (SPEC kap. 2): nic nemění, obsah paketů se nečte
//! NIKDY — kam a kolik, ne co (SPEC 12.2). Reverzní DNS doplňuje
//! volající z vlastní cache; tady se jen řekne, které adresy stojí
//! za překlad.

use std::collections::HashMap;
use std::net::IpAddr;

use core_types::proc::{AppNetRow, ConnRow};

/// Identita vlastníka spojení, jak ji zná sampler. Vlastněné řetězce
/// — closure je skládá z map, které drží zámek jen po dobu volání.
pub struct Owner {
    pub identity_key: String,
    pub app_name: String,
    pub publisher: Option<String>,
}

/// Sestaví per-aplikační pohled ze snapshotu spojení.
///
/// `owner_of` mapuje PID na identitu (ze sampleru — živý stav);
/// `name_of` vrací PTR jméno z cache resolveru (None = ještě neznáme).
/// Spojení procesů, které sampler nezná (krátce žijící), se seskupí
/// pod jménem procesu z tabulky — poctivě bez identity.
pub fn per_app(
    conns: &[win_sys::conns::Conn],
    owner_of: impl Fn(u32) -> Option<Owner>,
    name_of: impl Fn(IpAddr) -> Option<String>,
) -> Vec<AppNetRow> {
    // Seskupení podle identity; klíč "pid:N" pro neznámé vlastníky.
    let mut groups: HashMap<String, AppNetRow> = HashMap::new();
    let mut pids_per_group: HashMap<String, std::collections::HashSet<u32>> = HashMap::new();

    for c in conns {
        // Systémový idle proces a TIME_WAIT zbytky bez vlastníka.
        if c.pid == 0 {
            continue;
        }
        let (key, app_name, publisher) = match owner_of(c.pid) {
            Some(o) => (o.identity_key, o.app_name, o.publisher),
            None => (format!("pid:{}", c.pid), format!("PID {}", c.pid), None),
        };
        let row = groups.entry(key.clone()).or_insert_with(|| AppNetRow {
            identity_key: key.clone(),
            app_name,
            publisher,
            proc_count: 0,
            established: 0,
            listening: 0,
            conns: Vec::new(),
        });
        pids_per_group.entry(key).or_default().insert(c.pid);

        if c.listening() {
            row.listening += 1;
        }
        if c.state == "established" {
            row.established += 1;
        }
        row.conns.push(ConnRow {
            proto: c.proto.to_string(),
            local: c.local.to_string(),
            local_port: c.local_port,
            remote: c.remote.map(|r| r.to_string()).unwrap_or_default(),
            remote_port: c.remote_port,
            remote_name: c.remote.and_then(&name_of),
            state: c.state.to_string(),
            pid: c.pid,
        });
    }

    let mut out: Vec<AppNetRow> = groups
        .into_iter()
        .map(|(key, mut row)| {
            row.proc_count = pids_per_group.get(&key).map_or(0, |s| s.len()) as u32;
            // Stabilní pořadí spojení: aktivní první, pak porty.
            row.conns.sort_by(|a, b| {
                let rank = |c: &ConnRow| match c.state.as_str() {
                    "established" => 0,
                    "listen" => 2,
                    "udp" => 3,
                    _ => 1,
                };
                rank(a)
                    .cmp(&rank(b))
                    .then_with(|| a.remote.cmp(&b.remote))
                    .then_with(|| a.local_port.cmp(&b.local_port))
            });
            row
        })
        .collect();
    // Nejaktivnější aplikace první.
    out.sort_by(|a, b| {
        b.established
            .cmp(&a.established)
            .then_with(|| b.conns.len().cmp(&a.conns.len()))
            .then_with(|| a.app_name.to_lowercase().cmp(&b.app_name.to_lowercase()))
    });
    out
}

/// Vzdálené adresy ze snapshotu, které stojí za reverzní překlad —
/// unikátní, bez loopbacku a privátních rozsahů (ty PTR nemají).
pub fn addrs_to_resolve(conns: &[win_sys::conns::Conn]) -> Vec<IpAddr> {
    let mut seen = std::collections::HashSet::new();
    conns
        .iter()
        .filter_map(|c| c.remote)
        .filter(|ip| {
            if ip.is_loopback() || ip.is_unspecified() {
                return false;
            }
            match ip {
                IpAddr::V4(v4) => !v4.is_private() && !v4.is_link_local(),
                IpAddr::V6(v6) => {
                    // fe80::/10 link-local a fc00::/7 unique-local.
                    let seg = v6.segments()[0];
                    !(0xfe80..=0xfebf).contains(&seg) && !(0xfc00..=0xfdff).contains(&seg)
                }
            }
        })
        .filter(|ip| seen.insert(*ip))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use win_sys::conns::Conn;

    fn conn(proto: &'static str, pid: u32, state: &'static str, remote: Option<&str>) -> Conn {
        Conn {
            proto,
            local: "0.0.0.0".parse().unwrap(),
            local_port: 1000,
            remote: remote.map(|r| r.parse().unwrap()),
            remote_port: 443,
            state,
            pid,
        }
    }

    // Seskupení: dva procesy jedné aplikace = jedna skupina se dvěma
    // spojeními; neznámý PID dostane poctivou pid: identitu.
    #[test]
    fn groups_by_identity() {
        let conns = vec![
            conn("tcp", 10, "established", Some("1.2.3.4")),
            conn("tcp", 11, "established", Some("5.6.7.8")),
            conn("tcp", 99, "listen", None),
        ];
        let rows = per_app(
            &conns,
            |pid| {
                (pid == 10 || pid == 11).then(|| Owner {
                    identity_key: "app:chrome".into(),
                    app_name: "Chrome".into(),
                    publisher: Some("Google".into()),
                })
            },
            |_| None,
        );
        assert_eq!(rows.len(), 2);
        let chrome = rows
            .iter()
            .find(|r| r.identity_key == "app:chrome")
            .unwrap();
        assert_eq!(chrome.proc_count, 2);
        assert_eq!(chrome.established, 2);
        let anon = rows.iter().find(|r| r.identity_key == "pid:99").unwrap();
        assert_eq!(anon.listening, 1);
    }

    // K překladu jdou jen veřejné adresy — privátní PTR nemají
    // a loopback je šum.
    #[test]
    fn resolve_list_skips_private() {
        let conns = vec![
            conn("tcp", 1, "established", Some("192.168.1.1")),
            conn("tcp", 1, "established", Some("10.0.0.1")),
            conn("tcp", 1, "established", Some("142.250.180.14")),
            conn("tcp", 1, "established", Some("142.250.180.14")),
            conn("tcp", 1, "established", Some("127.0.0.1")),
        ];
        let list = addrs_to_resolve(&conns);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].to_string(), "142.250.180.14");
    }
}
