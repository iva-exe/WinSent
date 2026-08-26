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
fn tpm() -> Option<(bool, String)> {
    let rows = crate::wmi::query(
        r"root\CIMV2\Security\MicrosoftTpm",
        "SELECT IsEnabled_InitialValue, SpecVersion FROM Win32_Tpm",
        &["IsEnabled_InitialValue", "SpecVersion"],
    );
    let r = rows.first()?;
    Some((
        crate::wmi::flag(r, "IsEnabled_InitialValue").unwrap_or(false),
        r.get("SpecVersion")
            .map(|v| v.split(',').next().unwrap_or(v).trim().to_string())
            .unwrap_or_default(),
    ))
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
}
