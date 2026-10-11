//! Stav vestavěné ochrany Windows (v9, SPEC kap. 13.1) — JEN ČTENÍ.
//!
//! Jedna otázka: „jsem chráněný?" Odpovědi se čtou z veřejných zdrojů
//! (SecurityCenter2 WMI, Defender WMI, registr, BitLocker WMI) a kde
//! odpověď není, řekne se to — žádné hádání (SPEC: nikdy nepředstírej
//! záruku, kterou nemáš).

/// Stav jednoho antiviru ze Security Center.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AvProduct {
    pub name: String,
    /// Běží realtime ochrana? (bity productState 0x1000)
    pub enabled: bool,
    /// Jsou definice aktuální? (bit 0x10 = zastaralé)
    pub up_to_date: bool,
    /// Program, který se registroval, na disku NENÍ.
    ///
    /// Security Center registraci po odinstalaci nemusí uklidit, takže
    /// tam roky visí antivirus, který na počítači dávno není — a hlásí
    /// se jako běžící. Bez tohohle příznaku bychom uživateli tvrdili,
    /// že má zapnutou ochranu, kterou nemá, a ještě ho strašili dvěma
    /// antiviry naráz.
    pub leftover: bool,
}

/// Detaily Defenderu z jeho vlastního WMI (jen když je aktivní).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DefenderStatus {
    pub realtime: bool,
    /// Stáří definic ve dnech.
    pub signature_age_days: Option<u32>,
    /// Stáří posledního rychlého skenu ve dnech.
    pub quick_scan_age_days: Option<u32>,
}

/// Firewall per profil (doména, privátní, veřejná).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FirewallState {
    pub domain: Option<bool>,
    pub private: Option<bool>,
    pub public: Option<bool>,
}

/// UAC konfigurace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UacState {
    pub enabled: bool,
    /// 0 = bez výzvy … 2 = souhlas na zabezpečené ploše (default).
    pub admin_prompt: Option<u32>,
    pub secure_desktop: bool,
}

/// Šifrování jednoho svazku (BitLocker).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VolumeEncryption {
    pub letter: String,
    /// 0 = nešifrováno, 1 = chráněno, 2 = neznámé/zamčeno.
    pub protection: u32,
}

/// Celkový stav ochrany.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Protection {
    pub av: Vec<AvProduct>,
    pub defender: Option<DefenderStatus>,
    pub firewall: FirewallState,
    pub uac: UacState,
    /// None = stroj bootuje přes legacy BIOS (Secure Boot neexistuje).
    pub secure_boot: Option<bool>,
    /// (přítomen, verze specifikace) — z WMI MicrosoftTpm.
    pub tpm: Option<(bool, String)>,
    pub volumes: Vec<VolumeEncryption>,
}

/// Přečte celý stav ochrany. COM musí být inicializované na vlákně.
pub fn protection() -> Protection {
    Protection {
        av: av_products(),
        defender: defender_status(),
        firewall: firewall(),
        uac: uac(),
        secure_boot: secure_boot(),
        tpm: tpm(),
        volumes: bitlocker(),
    }
}

/// Antiviry registrované v Security Center (root\SecurityCenter2).
fn av_products() -> Vec<AvProduct> {
    crate::wmi::query(
        r"root\SecurityCenter2",
        "SELECT displayName, productState, pathToSignedProductExe FROM AntiVirusProduct",
        &["displayName", "productState", "pathToSignedProductExe"],
    )
    .into_iter()
    .filter_map(|r| {
        let state: u32 = r.get("productState")?.parse().ok()?;
        Some(AvProduct {
            name: r.get("displayName")?.clone(),
            // Dokumentované bity WSC_SECURITY_PRODUCT_STATE.
            enabled: state & 0x1000 != 0,
            up_to_date: state & 0x10 == 0,
            leftover: product_gone(r.get("pathToSignedProductExe").map(|s| s.as_str())),
        })
    })
    .collect()
}

/// Binárky bezpečnostních produktů, které se registrovaly v Security
/// Center (antivir, antispyware, firewall): `pathToSignedProductExe`
/// a `pathToSignedReportingExe`, rozbalené a malými písmeny.
///
/// Slouží k jedné věci — poznat, co Winsent nesmí ukončit, vypnout po
/// spuštění ani nabídnout k odinstalaci bez varování. Antivir se
/// vypíná ve svém vlastním nastavení; zvenčí to buď zablokuje jeho
/// sebeobrana, nebo se to povede a počítač zůstane bez ochrany.
///
/// Registrace bez binárky na disku (zbytek po odinstalaci) se
/// vynechává; Defender se hlásí URI (`windowsdefender://`), to cesta
/// není. COM se na vlákně inicializuje tady (idempotentní). WMI —
/// volá se na vyžádání, nikdy ve vzorkovací smyčce.
pub fn product_exes() -> Vec<String> {
    crate::wic::init_com_for_thread();
    let mut out: Vec<String> = Vec::new();
    for class in ["AntiVirusProduct", "AntiSpywareProduct", "FirewallProduct"] {
        let rows = crate::wmi::query(
            r"root\SecurityCenter2",
            &format!("SELECT pathToSignedProductExe, pathToSignedReportingExe FROM {class}"),
            &["pathToSignedProductExe", "pathToSignedReportingExe"],
        );
        for r in rows {
            for prop in ["pathToSignedProductExe", "pathToSignedReportingExe"] {
                let Some(raw) = r.get(prop) else { continue };
                let p = expand_env(raw.trim());
                if !looks_like_file_path(&p) || product_gone(Some(&p)) {
                    continue;
                }
                let lc = p.replace('/', "\\").to_ascii_lowercase();
                if !out.contains(&lc) {
                    out.push(lc);
                }
            }
        }
    }
    out
}

/// Adresáře bezpečnostních produktů (malými písmeny, bez koncového
/// lomítka): rodičovské složky z `product_exes`. Co v nich leží, patří
/// antiviru.
///
/// Mělké a sdílené složky se zahazují. Produkt, který by svou binárku
/// nechal přímo v `C:\Program Files` nebo v System32, by jinak udělal
/// „antivir" ze všeho vedle ní. Co leží ve Windows, chrání pravidla
/// pro Windows; tahle množina je jen na cizí produkty.
pub fn product_dirs() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for exe in product_exes() {
        let Some((dir, _)) = exe.rsplit_once(char::from(92u8)) else {
            continue;
        };
        if product_dir_ok(dir) && !out.iter().any(|d| d == dir) {
            out.push(dir.to_string());
        }
    }
    out
}

/// `product_dirs` nejvýš `max_age` stará. Sada antivirů se mění jen
/// instalací a odinstalací, kdežto WMI dotaz stojí desítky milisekund —
/// a ptá se na ni každé ukončení procesu, přepnutí po spuštění
/// (rozpočet T0 je pod 50 ms) i výpis programů.
pub fn product_dirs_cached(max_age: std::time::Duration) -> Vec<String> {
    use std::sync::Mutex;
    use std::time::Instant;
    static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
    if let Ok(g) = CACHE.lock() {
        if let Some((t, dirs)) = g.as_ref() {
            if t.elapsed() <= max_age {
                return dirs.clone();
            }
        }
    }
    // Dotaz běží mimo zámek — souběžný volající ho klidně udělá taky,
    // ale nikdo nečeká na cizí WMI.
    let dirs = product_dirs();
    if let Ok(mut g) = CACHE.lock() {
        *g = Some((Instant::now(), dirs.clone()));
    }
    dirs
}

/// Je adresář dost konkrétní, aby celý patřil jednomu produktu?
fn product_dir_ok(dir_lc: &str) -> bool {
    let sep = char::from(92u8);
    let dir = dir_lc.trim_end_matches(sep);
    // `c:\vendor\produkt` = aspoň dvě úrovně pod kořenem svazku.
    if dir.matches(sep).count() < 2 {
        return false;
    }
    let sysroot = std::env::var("SystemRoot")
        .unwrap_or_else(|_| r"C:\Windows".into())
        .to_ascii_lowercase();
    let sysroot = sysroot.trim_end_matches(sep);
    if dir == sysroot || dir.starts_with(&format!("{sysroot}{sep}")) {
        return false;
    }
    // Sdílené kontejnery dvě úrovně hluboko.
    let jmeno = dir.rsplit(sep).next().unwrap_or_default();
    !matches!(jmeno, "common files" | "microsoft" | "windowsapps")
}

/// Vypadá řetězec jako cesta k souboru (`X:\…`)? URI schéma ne.
fn looks_like_file_path(p: &str) -> bool {
    p.len() > 3
        && p.as_bytes().get(1) == Some(&b':')
        && p.as_bytes()[0].is_ascii_alphabetic()
        && (p.contains(char::from(92u8)) || p.contains('/'))
}

/// `%ProgramFiles%\…` → skutečná cesta. Neznámou proměnnou nechá být
/// (pak cesta neexistuje a registrace se vynechá).
fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let key = &after[..j];
                match std::env::var(key) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(key);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Je registrace osiřelá — binárka, kterou uvádí, na disku není?
///
/// Defender se registruje jako `windowsdefender://`, což cesta k souboru
/// není; tam se nic netvrdí. Když cesta chybí nebo je nečitelná, taky
/// mlčíme — prohlásit ochranu za neexistující kvůli neznalosti by bylo
/// horší než ji nechat být.
fn product_gone(path: Option<&str>) -> bool {
    let Some(p) = path.map(str::trim).filter(|p| !p.is_empty()) else {
        return false;
    };
    // Cesta k souboru = svazek a lomítko. Cokoliv jiného (URI schéma)
    // neumíme ověřit.
    let looks_like_path = p.len() > 3
        && p.as_bytes().get(1) == Some(&b':')
        && (p.contains(char::from(92u8)) || p.contains('/'));
    if !looks_like_path {
        return false;
    }
    matches!(std::path::Path::new(p).try_exists(), Ok(false))
}

/// Detaily Defenderu (root\Microsoft\Windows\Defender) — potřebuje
/// admin/SYSTEM, což služba je. Na stroji s cizím AV bývá prázdné.
fn defender_status() -> Option<DefenderStatus> {
    let rows = crate::wmi::query(
        r"root\Microsoft\Windows\Defender",
        "SELECT RealTimeProtectionEnabled, AntivirusSignatureAge, QuickScanAge \
         FROM MSFT_MpComputerStatus",
        &[
            "RealTimeProtectionEnabled",
            "AntivirusSignatureAge",
            "QuickScanAge",
        ],
    );
    let r = rows.first()?;
    Some(DefenderStatus {
        realtime: crate::wmi::flag(r, "RealTimeProtectionEnabled")?,
        signature_age_days: r.get("AntivirusSignatureAge").and_then(|v| v.parse().ok()),
        // QuickScanAge 4294967295 = sken nikdy neproběhl.
        quick_scan_age_days: r
            .get("QuickScanAge")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&v| v < 36500)
            .map(|v| v as u32),
    })
}

/// Firewall per profil — přímo z registru (levné, bez COM).
fn firewall() -> FirewallState {
    use crate::registry::{read_u64, HKEY_LOCAL_MACHINE};
    let read = |profile: &str| -> Option<bool> {
        read_u64(
            HKEY_LOCAL_MACHINE,
            &format!(
                r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\{profile}"
            ),
            "EnableFirewall",
        )
        .map(|v| v != 0)
    };
    FirewallState {
        domain: read("DomainProfile"),
        private: read("StandardProfile"),
        public: read("PublicProfile"),
    }
}

/// UAC z Policies\System.
fn uac() -> UacState {
    use crate::registry::{read_u64, HKEY_LOCAL_MACHINE};
    let base = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
    UacState {
        enabled: read_u64(HKEY_LOCAL_MACHINE, base, "EnableLUA").unwrap_or(0) != 0,
        admin_prompt: read_u64(HKEY_LOCAL_MACHINE, base, "ConsentPromptBehaviorAdmin")
            .map(|v| v as u32),
        secure_desktop: read_u64(HKEY_LOCAL_MACHINE, base, "PromptOnSecureDesktop").unwrap_or(1)
            != 0,
    }
}

/// Secure Boot: klíč existuje jen na UEFI stroji. None = legacy BIOS,
/// kde Secure Boot neexistuje — to je fakt, ne chyba.
fn secure_boot() -> Option<bool> {
    use crate::registry::{read_u64, HKEY_LOCAL_MACHINE};
    read_u64(
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Control\SecureBoot\State",
        "UEFISecureBootEnabled",
    )
    .map(|v| v != 0)
}

/// TPM přes WMI (root\CIMV2\Security\MicrosoftTpm).
///
/// Prázdný výsledek NEZNAMENÁ „čip tam není". Dotaz vrátí prázdno taky
/// tehdy, když na něj nemáme právo nebo když WMI třídu neregistrovala
/// žádná verze — a Winsent to hlásil jako „TPM: nenalezen", tedy jako
/// fakt o hardwaru. Proto se hned vedle hledá zařízení ve stromu
/// systému: když tam Trusted Platform Module je, čip existuje a my jen
/// nevíme, v jakém je stavu.
fn tpm() -> Option<(bool, String)> {
    let rows = crate::wmi::query(
        r"root\CIMV2\Security\MicrosoftTpm",
        "SELECT IsEnabled_InitialValue, SpecVersion FROM Win32_Tpm",
        &["IsEnabled_InitialValue", "SpecVersion"],
    );
    let Some(r) = rows.first() else {
        // Čip ve stromu zařízení je, stav neznáme — prázdná verze
        // specifikace je pro volajícího značka „nezjištěno".
        return tpm_device_present().then(|| (false, String::new()));
    };
    Some((
        crate::wmi::flag(r, "IsEnabled_InitialValue").unwrap_or(false),
        r.get("SpecVersion")
            .map(|v| v.split(',').next().unwrap_or(v).trim().to_string())
            .unwrap_or_default(),
    ))
}

/// Je ve stromu zařízení čip TPM? Levné čtení registru — třída
/// `SecurityDevices` existuje jen tam, kde nějaký čip je.
fn tpm_device_present() -> bool {
    use crate::registry::{enum_subkeys, HKEY_LOCAL_MACHINE};
    enum_subkeys(
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Enum\ACPI\MSFT0101",
    )
    .iter()
    .any(|s| !s.is_empty())
}

/// BitLocker per svazek (root\CIMV2\Security\MicrosoftVolumeEncryption)
/// — čitelné jen pro admin/SYSTEM.
fn bitlocker() -> Vec<VolumeEncryption> {
    crate::wmi::query(
        r"root\CIMV2\Security\MicrosoftVolumeEncryption",
        "SELECT DriveLetter, ProtectionStatus FROM Win32_EncryptableVolume",
        &["DriveLetter", "ProtectionStatus"],
    )
    .into_iter()
    .filter_map(|r| {
        Some(VolumeEncryption {
            letter: r.get("DriveLetter")?.clone(),
            protection: r.get("ProtectionStatus")?.parse().ok()?,
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Stav ochrany musí jít přečíst a firewall profily existují na
    // každých Windows. AV vyžaduje SecurityCenter — na serverech chybí,
    // na desktopu je vždy aspoň Defender.
    #[test]
    fn protection_reads_something() {
        crate::wic::init_com_for_thread();
        let p = protection();
        assert!(
            p.firewall.private.is_some() || p.firewall.public.is_some(),
            "firewall profily nejdou přečíst"
        );
        // UAC klíč existuje vždy.
        // (enabled může být false — to je zjištění, ne chyba čtení)
        let _ = p.uac;
    }

    #[test]
    fn adresar_produktu_jen_konkretni() {
        assert!(product_dir_ok(r"c:\program files\eset\eset security"));
        assert!(product_dir_ok(r"c:\program files\avast software\avast"));
        assert!(product_dir_ok(r"d:\vendor\produkt"));
        for d in [
            r"c:\program files",
            r"c:\eset",
            r"c:\",
            r"c:\program files\common files",
            r"c:\programdata\microsoft",
        ] {
            assert!(!product_dir_ok(d), "{d} je moc obecný");
        }
        let sysroot = std::env::var("SystemRoot")
            .unwrap_or_else(|_| r"C:\Windows".into())
            .to_ascii_lowercase();
        assert!(!product_dir_ok(&format!(r"{sysroot}\system32")));
    }

    // Čtení Security Center nesmí spadnout a vrací jen existující cesty.
    #[test]
    fn produkty_jsou_existujici_cesty() {
        for exe in product_exes() {
            assert!(std::path::Path::new(&exe).exists(), "{exe} neexistuje");
        }
        for dir in product_dirs() {
            assert!(product_dir_ok(&dir), "{dir} neměl projít");
        }
    }
}
