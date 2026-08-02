//! Brána v9C — Connection. `cargo run -p ipc --example v9conncheck`
//!
//! Adaptéry musí odpovídat ipconfig; WiFi se na stroji bez karty
//! nepředstírá (wifi_present = false a žádné sítě).

fn main() {
    let mut fail = 0;

    match ipc::client::ping() {
        Ok(p) if p.protocol_version == core_types::ipc::PROTOCOL_VERSION => {
            println!("OK  protokol v{}", p.protocol_version)
        }
        other => {
            println!("!!  služba: {other:?}");
            std::process::exit(1);
        }
    }

    let r = match ipc::client::query_connection() {
        Ok(r) => r,
        Err(e) => {
            println!("!!  dotaz selhal: {e}");
            std::process::exit(1);
        }
    };

    // 1) Aspoň jeden fyzický adaptér nahoře s kompletní konfigurací.
    let active: Vec<_> = r
        .adapters
        .iter()
        .filter(|a| a.up && (a.kind == "ethernet" || a.kind == "wifi"))
        .collect();
    if active.is_empty() {
        fail += 1;
        println!("!!  žádný aktivní fyzický adaptér — stroj je přitom online");
    }
    for a in &active {
        let v4 = a.ips.iter().any(|i| !i.contains(':'));
        if !v4 || a.gateways.is_empty() || a.dns.is_empty() || a.mac.is_empty() {
            fail += 1;
            println!("!!  {} má díry v konfiguraci: {a:?}", a.name);
        } else {
            println!(
                "OK  {} ({}) — {} · brána {} · DNS {} · {} Mb/s · DHCP {}",
                a.name,
                a.description,
                a.ips.iter().find(|i| !i.contains(':')).expect("v4"),
                a.gateways[0],
                a.dns[0],
                a.link_mbps,
                a.dhcp
            );
        }
    }

    // 2) WiFi poctivost: bez karty žádné sítě ani připojení.
    if r.wifi_present {
        println!(
            "OK  WiFi karta přítomná; {} sítí v dosahu, připojeno: {:?}",
            r.wifi_networks.len(),
            r.wifi_connection.as_ref().map(|c| &c.ssid)
        );
        for n in &r.wifi_networks {
            if n.signal_pct > 100 {
                fail += 1;
                println!("!!  {} hlásí signál {} %", n.ssid, n.signal_pct);
            }
        }
    } else if r.wifi_connection.is_some() || !r.wifi_networks.is_empty() {
        fail += 1;
        println!("!!  WiFi data bez WiFi karty — to je vymyšlené číslo");
    } else {
        println!("--  WiFi karta žádná — sekce se poctivě nepředstírá");
    }

    println!("\n{}", if fail == 0 { "v9C: PASS" } else { "v9C: FAIL" });
}
