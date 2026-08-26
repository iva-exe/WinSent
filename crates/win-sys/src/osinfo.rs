//! Verze Windows a stav aktualizací (SPEC kap. 13.1 — „jsem chráněný?").
//!
//! Všechno jen čtení, žádné COM a žádná síť: `RtlGetVersion` pro pravé
//! číslo sestavení (`GetVersionEx` lže kvůli manifestům) a registr pro
//! edici, marketingové označení a časy poslední úspěšné kontroly.
//!
//! Windows Update se schválně NEČTE přes `IUpdateSearcher` — ten se ptá
//! po síti, trvá desítky sekund a na stroji s odloženými aktualizacemi
//! se umí zaseknout. Časy z `Auto Update\Results` říkají to podstatné:
//! kdy systém naposledy opravdu hledal a instaloval.

use crate::registry::{enum_subkeys, enum_values, read_string, read_u64, HKEY_LOCAL_MACHINE};

const CURVER: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const WU_RESULTS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\Results";

/// Verze systému a stav aktualizací.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OsInfo {
    /// „Windows 11 Pro" — marketingové jméno včetně edice.
    pub product: String,
    /// „24H2" — označení funkční aktualizace.
    pub display_version: Option<String>,
    /// Sestavení a revize: 26100.4652.
    pub build: u32,
    pub ubr: u32,
    /// Kdy byl systém nainstalován (unix).
    pub install_ts: Option<i64>,
    /// Architektura: „x64", „ARM64", „x86".
    pub arch: String,
    /// Kdy naposledy úspěšně proběhlo hledání aktualizací (unix).
    pub update_last_search: Option<i64>,
    /// Kdy naposledy úspěšně proběhla instalace aktualizace (unix).
    pub update_last_install: Option<i64>,
    /// Typ spuštění služby Windows Update (2 = automaticky,
    /// 3 = ručně, 4 = zakázáno). `None` = klíč se nepodařilo přečíst.
    pub update_service_start: Option<u32>,
    /// Je automatické aktualizování zakázané zásadou?
    pub update_disabled_by_policy: bool,
}

/// Přečte verzi systému a stav aktualizací. Levné — samý registr.
pub fn os_info() -> OsInfo {
    let build = read_string(HKEY_LOCAL_MACHINE, CURVER, "CurrentBuildNumber")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    OsInfo {
        // ProductName na Windows 11 pořád hlásí „Windows 10 Pro" —
        // Microsoft ho po vydání jedenáctky nepřepsal. Číslo sestavení
        // je jediný spolehlivý rozlišovač (22000 a výš = 11).
        product: product_name(build),
        display_version: read_string(HKEY_LOCAL_MACHINE, CURVER, "DisplayVersion")
            .or_else(|| read_string(HKEY_LOCAL_MACHINE, CURVER, "ReleaseId")),
        build,
        ubr: read_u64(HKEY_LOCAL_MACHINE, CURVER, "UBR").unwrap_or(0) as u32,
        install_ts: read_u64(HKEY_LOCAL_MACHINE, CURVER, "InstallDate").map(|v| v as i64),
        arch: arch(),
        update_last_search: wu_time("Detect").or_else(last_online_scan),
        update_last_install: wu_time("Install"),
        update_service_start: read_u64(
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Services\wuauserv",
            "Start",
        )
        .map(|v| v as u32),
        update_disabled_by_policy: read_u64(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU",
            "NoAutoUpdate",
        )
        .is_some_and(|v| v != 0),
    }
}

/// Jméno systému. Registr se u jedenáctky nezměnil, tak se opraví podle
/// čísla sestavení a edice se doplní z `EditionID`.
fn product_name(build: u32) -> String {
    let raw = read_string(HKEY_LOCAL_MACHINE, CURVER, "ProductName")
        .unwrap_or_else(|| "Windows".to_string());
    if build >= 22000 && raw.contains("Windows 10") {
        return raw.replace("Windows 10", "Windows 11");
    }
    raw
}

/// Architektura procesu systému z proměnné prostředí — služba běží
/// nativně, takže tohle je architektura stroje.
fn arch() -> String {
    match std::env::var("PROCESSOR_ARCHITECTURE").as_deref() {
        Ok("AMD64") => "x64".into(),
        Ok("ARM64") => "ARM64".into(),
        Ok("x86") => "x86".into(),
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// `LastSuccessTime` z výsledků Windows Update. Formát je
/// „YYYY-MM-DD HH:MM:SS" v UTC.
fn wu_time(what: &str) -> Option<i64> {
    let s = read_string(HKEY_LOCAL_MACHINE, &format!(r"{WU_RESULTS}\{what}"), "LastSuccessTime")?;
    parse_utc(&s)
}

/// Náhradní zdroj času posledního hledání.
///
/// Klíč `Auto Update\Results` na novějších sestaveních chybí — správu
/// aktualizací převzal USO a tenhle záznam už neplní. `Auto Update\
/// LastOnlineScanTimeForAppCategory` ale zůstal: pod ním leží kategorie
/// a v každé sada GUID → čas posledního online skenu. Nejnovější z nich
/// je to, co uživatel zná jako „naposledy zkontrolováno".
fn last_online_scan() -> Option<i64> {
    const BASE: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\LastOnlineScanTimeForAppCategory";
    enum_subkeys(HKEY_LOCAL_MACHINE, BASE)
        .iter()
        .flat_map(|sub| enum_values(HKEY_LOCAL_MACHINE, &format!(r"{BASE}\{sub}")))
        .filter_map(|(_, v)| parse_utc(&v))
        .max()
}

/// „YYYY-MM-DD HH:MM:SS" (UTC) → unix. Vlastní převod, protože kvůli
/// jednomu formátu z registru se nevyplatí tahat do služby kalendář.
fn parse_utc(s: &str) -> Option<i64> {
    let (date, time) = s.trim().split_once(' ')?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let mo: i64 = d.next()?.parse().ok()?;
    let da: i64 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let se: i64 = t.next().unwrap_or("0").parse().ok()?;
    if !(1970..=3000).contains(&y) || !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        return None;
    }
    Some(days_from_civil(y, mo, da) * 86400 + h * 3600 + mi * 60 + se)
}

/// Počet dní od 1970-01-01 (Howard Hinnantův algoritmus, veřejná doména).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_parsing_matches_known_epoch() {
        assert_eq!(parse_utc("1970-01-01 00:00:00"), Some(0));
        assert_eq!(parse_utc("2000-01-01 00:00:00"), Some(946_684_800));
        assert_eq!(parse_utc("2026-08-26 21:28:02"), Some(1_787_779_682));
        assert_eq!(parse_utc("nesmysl"), None);
    }

    // Na každém Windows musí vyjít aspoň jméno a sestavení.
    #[test]
    fn os_info_has_build() {
        let i = os_info();
        assert!(i.build > 0, "sestavení nepřečteno");
        assert!(!i.product.is_empty(), "jméno systému nepřečteno");
    }
}
