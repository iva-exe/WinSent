//! Oprávnění aplikací z CapabilityAccessManager ConsentStore
//! (v9, SPEC kap. 13.4) — kdo má přístup ke kameře, mikrofonu,
//! poloze… a kdo je používá PRÁVĚ TEĎ.
//!
//! Zdroj: registr `...\CapabilityAccessManager\ConsentStore\<schopnost>`
//! — balené aplikace jako PackageFamilyName podklíče, klasické pod
//! `NonPackaged\<cesta s # místo \>`. `LastUsedTimeStop == 0` při
//! nenulovém startu = používá právě teď (totéž, co pohání tečku
//! u kamery ve Windows).
//!
//! Služba běží jako SYSTEM, takže HKCU je hive SYSTEMU — souhlasy
//! reálných uživatelů se čtou z HKU\<SID> (stejný důvod jako
//! u odinstalace, viz validate::uninstall_command).
//!
//! ⚠ Vynucení (SPEC 13.4): u balených aplikací Windows Deny tvrdě
//! vynutí brokerem; u Win32 je to jen deklarace, kterou jde obejít.
//! `enforced` to nese dál — UI NIKDY nesmí ukázat tvrdý zámek tam,
//! kde není.

use crate::registry::{enum_subkeys, read_string, read_u64, HKEY_USERS};

/// Jeden záznam oprávnění: aplikace × schopnost.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Consent {
    /// webcam | microphone | location | …
    pub capability: String,
    /// PackageFamilyName, nebo cesta k .exe u klasických aplikací.
    pub app: String,
    /// Balená (MSIX/UWP) aplikace? Jen u těch Windows Deny VYNUTÍ.
    pub packaged: bool,
    /// "allow" | "deny" (jiné hodnoty se přeskakují).
    pub allow: bool,
    /// Používá právě teď (stop == 0 a start != 0).
    pub in_use: bool,
    /// Konec posledního použití (unix), když je znám.
    pub last_used: Option<i64>,
    /// Začátek posledního použití (unix). Spolu s koncem dává dobu,
    /// po kterou aplikace kameru nebo mikrofon opravdu držela —
    /// „naposledy včera, 3 h 12 min" je informace úplně jiné váhy
    /// než holé „naposledy včera".
    pub last_start: Option<i64>,
}

/// Schopnosti, které mají pro uživatele význam. Ostatní podklíče
/// ConsentStore (userAccountInformation…) jsou nízkoúrovňový šum.
const CAPABILITIES: &[&str] = &[
    "webcam",
    "microphone",
    "location",
    "contacts",
    "appointments",
    "email",
    "phoneCall",
    "documentsLibrary",
    "picturesLibrary",
    "videosLibrary",
    "musicLibrary",
    "downloadsFolder",
    "broadFileSystemAccess",
    "screenCapture",
];

const STORE: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

/// FILETIME (100ns od 1601) → unix sekundy.
fn filetime_to_unix(ft: u64) -> Option<i64> {
    if ft == 0 {
        return None;
    }
    Some((ft / 10_000_000) as i64 - 11_644_473_600)
}

/// Přečte souhlasy všech reálných uživatelů (HKU\S-1-5-21-*).
pub fn consents() -> Vec<Consent> {
    let mut out = Vec::new();
    for sid in enum_subkeys(HKEY_USERS, "") {
        if !sid.starts_with("S-1-5-21") || sid.ends_with("_Classes") {
            continue;
        }
        let store = format!(r"{sid}\{STORE}");
        for cap in CAPABILITIES {
            let cap_key = format!(r"{store}\{cap}");
            for entry in enum_subkeys(HKEY_USERS, &cap_key) {
                if entry == "NonPackaged" {
                    // Klasické aplikace: per-exe podklíče nesou JEN časy
                    // použití — povolení platí hromadně pro všechny
                    // desktopové aplikace v `NonPackaged\Value` (přesně
                    // tak to má přepínač „aplikacím klasické plochy"
                    // v Nastavení). Cesta má '#' místo '\'.
                    let np = format!(r"{cap_key}\NonPackaged");
                    let np_allow = read_string(HKEY_USERS, &np, "Value")
                        .map(|v| v == "Allow")
                        .unwrap_or(true);
                    for exe in enum_subkeys(HKEY_USERS, &np) {
                        if let Some(c) = read_entry(
                            &format!(r"{np}\{exe}"),
                            cap,
                            &exe.replace('#', r"\"),
                            false,
                            Some(np_allow),
                        ) {
                            out.push(c);
                        }
                    }
                } else if let Some(c) =
                    read_entry(&format!(r"{cap_key}\{entry}"), cap, &entry, true, None)
                {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Přečte jeden záznam. `inherited_allow` je hromadné povolení pro
/// NonPackaged — per-exe klíče vlastní Value obvykle nemají a bez
/// zděděné hodnoty by se poctivé záznamy zahazovaly.
fn read_entry(
    key: &str,
    capability: &str,
    app: &str,
    packaged: bool,
    inherited_allow: Option<bool>,
) -> Option<Consent> {
    let allow = match read_string(HKEY_USERS, key, "Value").as_deref() {
        Some("Allow") => true,
        Some("Deny") => false,
        _ => inherited_allow?,
    };
    let start = read_u64(HKEY_USERS, key, "LastUsedTimeStart").unwrap_or(0);
    let stop = read_u64(HKEY_USERS, key, "LastUsedTimeStop").unwrap_or(0);
    Some(Consent {
        capability: capability.to_string(),
        app: app.to_string(),
        packaged,
        allow,
        in_use: start != 0 && stop == 0,
        last_used: filetime_to_unix(stop.max(start)),
        last_start: filetime_to_unix(start),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Živý stroj má v ConsentStore desítky záznamů; in_use vyžaduje
    // nenulový start a stop == 0 — nikdy naopak.
    #[test]
    fn consents_are_consistent() {
        let list = consents();
        assert!(
            !list.is_empty(),
            "ConsentStore prázdný — na živém stroji nesedí (HKU čtení selhalo?)"
        );
        for c in &list {
            if c.in_use {
                assert!(c.last_used.is_some(), "in_use bez času startu: {c:?}");
            }
        }
        // Aspoň jedna kamera/mikrofon položka na desktopu s aplikacemi.
        assert!(
            list.iter()
                .any(|c| c.capability == "webcam" || c.capability == "microphone"),
            "žádný webcam/microphone záznam"
        );
    }
}

/// SIDy skutečných uživatelů, jejichž souhlasy se čtou.
///
/// Služba běží jako SYSTEM, takže její `HKEY_CURRENT_USER` je hive
/// SYSTEMU a o souhlasech lidí neví nic. Sledování změn potřebuje
/// vědět, do kterých hive se dívat.
pub fn user_hives() -> Vec<String> {
    enum_subkeys(HKEY_USERS, "")
        .into_iter()
        .filter(|sid| sid.starts_with("S-1-5-21") && !sid.ends_with("_Classes"))
        .collect()
}
