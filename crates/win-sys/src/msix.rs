//! MSIX/AppX inventář (SPEC kap. 5.1): WinRT `PackageManager`.
//! `FindPackages()` (všichni uživatelé) vyžaduje admin/SYSTEM — služba
//! je SYSTEM, konzole běží elevovaně. Frameworky a resource balíčky
//! se přeskakují — uživatele zajímají aplikace.
//!
//! Tady žije i JEDNA definice „balíček je součást Windows" pro celou
//! službu (validace kill/toggle, inventář). UI má tentýž seznam
//! prefixů v `crates/ui/src/lib/mandatory.js` — musí zůstat shodné.

use std::collections::HashSet;
use std::sync::Mutex;

/// Jak je balíček podepsaný (`Package.SignatureKind`). `System` znamená
/// součást Windows — to je autoritativní odpověď OS, prefixy jmen jsou
/// jen záloha pro chvíli, kdy ji ještě nemáme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SignatureKind {
    /// Nepodařilo se zjistit.
    #[default]
    Unknown,
    None,
    Developer,
    Enterprise,
    Store,
    System,
}

/// Jeden nainstalovaný MSIX balíček.
#[derive(Debug, Clone, Default)]
pub struct MsixPackage {
    /// PackageFamilyName — párování s identity kaskádou (`msix:{family}`).
    pub family: String,
    pub display_name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    /// Instalační adresář (Exact — z manifestu balíčku).
    pub install_path: Option<String>,
    pub signature_kind: SignatureKind,
}

/// Rodiny balíčků, které patří Windows (malými písmeny, prefix jména
/// rodiny). Shodné se `SYS_FAMILIES` v UI (`mandatory.js`).
///
/// Tečka na konci `microsoft.windows.` je podstatná: Kalkulačka je
/// `Microsoft.WindowsCalculator` a ta se dá odebrat, takže součást
/// Windows NENÍ. Tytéž položky bez prefixu (GUID balíčky, AsyncTextService,
/// CredDialogHost…) Windows podepisuje jako System — prefix je tu pro
/// chvíli, kdy podpis ještě nemáme přečtený.
pub const SYSTEM_FAMILY_PREFIXES: &[&str] = &[
    "microsoft.windows.",
    "microsoftwindows.",
    "windows.",
    "microsoft.vclibs",
    "microsoft.net.",
    "microsoft.ui.xaml",
    "microsoft.windowsappruntime",
    "microsoft.sechealthui",
    "microsoft.aad.",
    "microsoft.accountscontrol",
    "microsoft.lockapp",
    "microsoft.win32webviewhost",
    "microsoft.asynctextservice",
    "microsoft.bioenrollment",
    "microsoft.creddialoghost",
    "microsoft.ecapp",
    "microsoft.microsoftedgedevtoolsclient",
    "microsoft.xboxgamecallableui",
    "ncsiuwpapp",
    "microsoft.windowsstore",
    "microsoft.storepurchaseapp",
    "microsoft.desktopappinstaller",
];

/// Rodiny, které OS při posledním výčtu podepsal jako `System`
/// (malými písmeny). Plní je `packages()` — sken inventáře běží při
/// startu služby a pak řídce, takže dotaz na „je to Windows?" nestojí
/// nic navíc. Prázdná množina (sken ještě neproběhl) jen znamená, že
/// rozhodují prefixy.
static SYSTEMOVE_RODINY: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Je balíček součást Windows? Bere jméno rodiny (`Name_publisherId`)
/// i plné jméno balíčku (`Name_verze_arch_res_publisherId`), jak leží
/// ve složce WindowsApps.
pub fn is_system_family(family: &str) -> bool {
    let f = family.trim().to_ascii_lowercase();
    if f.is_empty() {
        return false;
    }
    if SYSTEM_FAMILY_PREFIXES.iter().any(|p| f.starts_with(p)) || guid_named(&f) {
        return true;
    }
    let rodina = family_of_full_name(&f);
    SYSTEMOVE_RODINY
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.contains(&f) || s.contains(&rodina)))
        .unwrap_or(false)
}

/// Balíčky pojmenované GUIDem (`1527c705-839a-4832-9118-54d4bd6a0c89`)
/// jsou interní součásti Windows bez lidského jména.
fn guid_named(f: &str) -> bool {
    let b = f.as_bytes();
    b.len() >= 14
        && b[..8].iter().all(u8::is_ascii_hexdigit)
        && b[8] == b'-'
        && b[9..13].iter().all(u8::is_ascii_hexdigit)
        && b[13] == b'-'
}

/// Rodina z plného jména balíčku: první a poslední díl oddělený `_`
/// (`Microsoft.WindowsStore_12107.1001.15.0_x64__8wekyb3d8bbwe` →
/// `Microsoft.WindowsStore_8wekyb3d8bbwe`). Jméno rodiny vrátí beze změny.
pub fn family_of_full_name(full: &str) -> String {
    let dily: Vec<&str> = full.split('_').collect();
    if dily.len() <= 2 {
        return full.to_string();
    }
    format!("{}_{}", dily[0], dily[dily.len() - 1])
}

/// Vyjmenuje MSIX balíčky (bez frameworků a resource balíčků).
pub fn packages() -> Vec<MsixPackage> {
    let mut out = Vec::new();
    let Ok(pm) = windows::Management::Deployment::PackageManager::new() else {
        return out;
    };
    let Ok(list) = pm.FindPackages() else {
        return out;
    };
    let Ok(iter) = list.First() else { return out };
    let mut systemove: HashSet<String> = HashSet::new();
    for pkg in iter {
        let Ok(id) = pkg.Id() else { continue };
        let family = id
            .FamilyName()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        if family.is_empty() {
            continue;
        }
        let signature_kind = signature_kind(&pkg);
        // Podpis se zapisuje i u frameworků: proces z frameworku
        // (VCLibs, WindowsAppRuntime) se o něj při validaci kill opře.
        if signature_kind == SignatureKind::System {
            systemove.insert(family.to_ascii_lowercase());
        }
        // Frameworky/resource nejsou aplikace.
        if pkg.IsFramework().unwrap_or(false) || pkg.IsResourcePackage().unwrap_or(false) {
            continue;
        }
        // DisplayName umí u neprovisionovaných balíčků selhat — fallback
        // na jméno z identity.
        let display_name = pkg
            .DisplayName()
            .map(|s| s.to_string_lossy())
            .ok()
            .filter(|s| !s.is_empty() && !s.starts_with("ms-resource:"))
            .or_else(|| id.Name().map(|s| s.to_string_lossy()).ok())
            .unwrap_or_else(|| family.clone());
        let version = id
            .Version()
            .map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision))
            .ok();
        let publisher = pkg
            .PublisherDisplayName()
            .map(|s| s.to_string_lossy())
            .ok()
            .filter(|s| !s.is_empty() && !s.starts_with("ms-resource:"));
        let install_path = pkg
            .InstalledPath()
            .map(|s| s.to_string_lossy())
            .ok()
            .filter(|s| !s.is_empty());
        out.push(MsixPackage {
            family,
            display_name,
            publisher,
            version,
            install_path,
            signature_kind,
        });
    }
    // Prázdný výsledek (selhání výčtu) starou znalost nepřepisuje —
    // jinak by po jednom neúspěšném skenu zmizely všechny podpisy.
    if !systemove.is_empty() {
        if let Ok(mut g) = SYSTEMOVE_RODINY.lock() {
            *g = Some(systemove);
        }
    }
    out
}

/// Podpis balíčku; chyba (starší Windows bez `IPackage4`) = Unknown.
fn signature_kind(pkg: &windows::ApplicationModel::Package) -> SignatureKind {
    use windows::ApplicationModel::PackageSignatureKind as K;
    match pkg.SignatureKind() {
        Ok(K::System) => SignatureKind::System,
        Ok(K::Store) => SignatureKind::Store,
        Ok(K::Enterprise) => SignatureKind::Enterprise,
        Ok(K::Developer) => SignatureKind::Developer,
        Ok(K::None) => SignatureKind::None,
        _ => SignatureKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemove_rodiny_podle_prefixu() {
        for f in [
            "Microsoft.Windows.ShellExperienceHost_cw5n1h2txyewy",
            "MicrosoftWindows.Client.CBS_cw5n1h2txyewy",
            "windows.immersivecontrolpanel_cw5n1h2txyewy",
            "Microsoft.SecHealthUI_8wekyb3d8bbwe",
            "Microsoft.AsyncTextService_8wekyb3d8bbwe",
            "NcsiUwpApp_8wekyb3d8bbwe",
            "Microsoft.WindowsStore_8wekyb3d8bbwe",
            "Microsoft.DesktopAppInstaller_8wekyb3d8bbwe",
            "1527c705-839a-4832-9118-54d4Bd6a0c89_cw5n1h2txyewy",
            "E2A4F912-2574-4A75-9BB0-0D023378592B_cw5n1h2txyewy",
            // Plné jméno, jak leží ve WindowsApps.
            "Microsoft.WindowsStore_12107.1001.15.0_x64__8wekyb3d8bbwe",
        ] {
            assert!(is_system_family(f), "{f} je součást Windows");
        }
        // Odebíratelné aplikace dodávané s Windows součástí NEJSOU.
        for f in [
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
            "microsoft.windowscommunicationsapps_8wekyb3d8bbwe",
            "NVIDIACorp.NVIDIAControlPanel_56jybvy8sckqj",
            "SpotifyAB.SpotifyMusic_zpdnekdrzrea0",
            "",
        ] {
            assert!(!is_system_family(f), "{f} součást Windows není");
        }
    }

    #[test]
    fn rodina_z_plneho_jmena() {
        assert_eq!(
            family_of_full_name("Microsoft.WindowsStore_12107.1001.15.0_x64__8wekyb3d8bbwe"),
            "Microsoft.WindowsStore_8wekyb3d8bbwe"
        );
        assert_eq!(
            family_of_full_name("Microsoft.WindowsStore_8wekyb3d8bbwe"),
            "Microsoft.WindowsStore_8wekyb3d8bbwe"
        );
    }
}
