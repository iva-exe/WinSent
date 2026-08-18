//! Čtení protokolu událostí Windows (v3+, SPEC kap. 16).
//!
//! Windows si o každém pádu aplikace i o každém BSODu vedou záznam samy
//! a vědí u něj věci, které se odjinud zjistit nedají — hlavně **modul,
//! ve kterém to spadlo**. To bývá jiná knihovna než aplikace sama a je
//! to ta nejcennější informace celého hlášení: „Photoshop spadl" je
//! k ničemu, „Photoshop spadl v ovladači grafiky" je odpověď.
//!
//! Čte se přes `EvtQuery` s XPath filtrem, ne procházením celého
//! protokolu — ten má na běžném stroji desítky tisíc záznamů.
//!
//! Pozor na past, na kterou se dá naletět: **ID události samo o sobě
//! neurčuje, o co jde.** ID 1000 v kanálu Application hlásí i WMI
//! (`WmiApRpl`), a kdo filtruje jen podle čísla, dostane úplně jiné
//! události — ověřeno na vývojovém stroji, kde jsou přesně takové tři.
//! Filtruje se proto vždy i podle poskytovatele.

use windows::core::PCWSTR;
use windows::Win32::System::EventLog::{
    EvtClose, EvtNext, EvtQuery, EvtQueryReverseDirection, EvtRender, EvtRenderEventXml, EVT_HANDLE,
};

use crate::Error;

/// Jedna přečtená událost — syrová, bez interpretace.
#[derive(Debug, Clone, Default)]
pub struct Event {
    /// Kdy nastala (unix). 0 = čas se nepodařilo přečíst.
    pub ts: i64,
    /// Hodnoty z `<EventData>` v pořadí, v jakém je systém zapsal.
    /// Význam pozic je daný poskytovatelem; popsaný je u volajícího,
    /// který ví, na co se ptal.
    pub data: Vec<String>,
}

/// RAII pro handle událostí.
struct Evt(EVT_HANDLE);

impl Drop for Evt {
    fn drop(&mut self) {
        if self.0.0 != 0 {
            // SAFETY: handle pochází z Evt* volání a zavírá se jednou.
            unsafe {
                let _ = EvtClose(self.0);
            }
        }
    }
}

/// Přečte poslední události kanálu podle XPath dotazu.
///
/// `limit` je strop — protokol může mít desítky tisíc záznamů a nikoho
/// nezajímají všechny. Čte se od nejnovější.
pub fn query(channel: &str, xpath: &str, limit: usize) -> Result<Vec<Event>, Error> {
    let wchannel: Vec<u16> = channel.encode_utf16().chain(std::iter::once(0)).collect();
    let wquery: Vec<u16> = xpath.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: oba řetězce jsou nulou ukončené a žijí po celé volání.
    let h = unsafe {
        EvtQuery(
            None,
            PCWSTR(wchannel.as_ptr()),
            PCWSTR(wquery.as_ptr()),
            EvtQueryReverseDirection.0,
        )
    }
    .map_err(|e| Error::Win32 {
        call: "EvtQuery",
        code: e.code().0,
    })?;
    let _q = Evt(h);

    let mut out = Vec::new();
    // EvtNext bere pole syrových handlů (isize), ne typovaných.
    let mut batch: [isize; 16] = [0; 16];
    while out.len() < limit {
        let mut returned = 0u32;
        // SAFETY: pole má hlášenou velikost; vrácené handly zavíráme níž.
        let ok = unsafe { EvtNext(h, &mut batch, 2000, 0, &mut returned) }.is_ok();
        if !ok || returned == 0 {
            break;
        }
        for ev in batch.iter().take(returned as usize) {
            let guard = Evt(EVT_HANDLE(*ev));
            if let Some(xml) = render(guard.0) {
                out.push(parse(&xml));
            }
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// Vyrenderuje událost do XML. Dvoufázově — nejdřív se zjistí velikost.
fn render(ev: EVT_HANDLE) -> Option<String> {
    let mut used = 0u32;
    let mut props = 0u32;
    // SAFETY: první volání jen zjišťuje potřebnou velikost bufferu.
    unsafe {
        let _ = EvtRender(None, ev, EvtRenderEventXml.0, 0, None, &mut used, &mut props);
    }
    if used == 0 {
        return None;
    }
    let mut buf = vec![0u8; used as usize];
    // SAFETY: buffer má právě tu velikost, o kterou si systém řekl.
    let ok = unsafe {
        EvtRender(
            None,
            ev,
            EvtRenderEventXml.0,
            used,
            Some(buf.as_mut_ptr() as *mut _),
            &mut used,
            &mut props,
        )
    }
    .is_ok();
    if !ok {
        return None;
    }
    // Výstup je UTF-16 s ukončovací nulou.
    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&c| c != 0)
        .collect();
    Some(String::from_utf16_lossy(&u16s))
}

/// Vytáhne z XML čas a hodnoty `Data`.
///
/// Vědomě bez XML knihovny: hledají se dvě konkrétní věci ve tvaru, který
/// generuje jediný producent — systém sám. Plnohodnotný parser by sem
/// přitáhl závislost kvůli dvěma řetězcům.
fn parse(xml: &str) -> Event {
    let mut ev = Event::default();
    if let Some(p) = xml.find("SystemTime='") {
        let rest = &xml[p + 12..];
        if let Some(end) = rest.find('\'') {
            ev.ts = iso_to_unix(&rest[..end]);
        }
    }
    let mut rest = xml;
    while let Some(p) = rest.find("<Data") {
        rest = &rest[p..];
        let Some(gt) = rest.find('>') else { break };
        let body = &rest[gt + 1..];
        let Some(end) = body.find("</Data>") else { break };
        ev.data.push(unescape(&body[..end]));
        rest = &body[end..];
    }
    ev
}

/// `2026-08-03T20:26:39.1234567Z` na unix. Bez knihovny na čas: formát je
/// pevný a zajímají nás jen celé sekundy.
fn iso_to_unix(s: &str) -> i64 {
    let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(sec)) = (
        num(0, 4),
        num(5, 7),
        num(8, 10),
        num(11, 13),
        num(14, 16),
        num(17, 19),
    ) else {
        return 0;
    };
    // Dny od epochy podle civilního kalendáře (Howard Hinnant).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + h * 3600 + mi * 60 + sec
}

/// Základní XML entity — víc jich systém do EventData nedává.
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Převod času musí sedět na známé hodnotě.
    #[test]
    fn iso_time_converts() {
        assert_eq!(iso_to_unix("2026-08-03T20:26:39.1234567Z"), 1_785_788_799);
        assert_eq!(iso_to_unix("1970-01-01T00:00:00.0000000Z"), 0);
    }

    // Z XML se musí vytáhnout hodnoty ve správném pořadí — na pozicích
    // stojí celý překlad pádu.
    #[test]
    fn data_values_keep_their_order() {
        let xml = "<Event><System><TimeCreated SystemTime='2026-08-03T20:26:39.0000000Z'/>\
                   </System><EventData><Data>EADesktop.exe</Data><Data>13.743.0.6256</Data>\
                   <Data>c0000005</Data></EventData></Event>";
        let ev = parse(xml);
        assert_eq!(ev.data, ["EADesktop.exe", "13.743.0.6256", "c0000005"]);
        assert_eq!(ev.ts, 1_785_788_799);
    }

    // Protokol Application jde číst a filtr podle poskytovatele projde.
    #[test]
    fn application_channel_is_readable() {
        let r = query(
            "Application",
            "*[System[Provider[@Name='Application Error'] and (EventID=1000)]]",
            5,
        );
        assert!(r.is_ok(), "dotaz selhal: {r:?}");
    }
}
