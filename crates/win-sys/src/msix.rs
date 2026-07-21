//! MSIX/AppX inventář (SPEC kap. 5.1): WinRT `PackageManager`.
//! `FindPackages()` (všichni uživatelé) vyžaduje admin/SYSTEM — služba
//! je SYSTEM, konzole běží elevovaně. Frameworky a resource balíčky
//! se přeskakují — uživatele zajímají aplikace.

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
    for pkg in iter {
        let Ok(id) = pkg.Id() else { continue };
        // Frameworky/resource nejsou aplikace.
        if pkg.IsFramework().unwrap_or(false) || pkg.IsResourcePackage().unwrap_or(false) {
            continue;
        }
        let family = id
            .FamilyName()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        if family.is_empty() {
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
        });
    }
    out
}
