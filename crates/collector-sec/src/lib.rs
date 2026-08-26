//! collector-sec — stav ochrany + oprávnění aplikací (v9, SPEC kap. 13).
//!
//! Čtecí crate (SPEC kap. 2): skládá SecurityReport z win-sys čtení.
//! Verdikty nevynáší — stav ochrany jsou fakta, oprávnění nesou
//! poctivý příznak `enforced` (SPEC 13.4: zelená jen tam, kde
//! vynucení opravdu je; falešný pocit ochrany je horší než žádný).

use core_types::proc::{PermissionRow, ProtectionReport, SecurityReport};

/// Stav ochrany. Sahá na WMI — volající cachuje (SPEC 15.1: WMI
/// nikdy v sekundovém cyklu).
pub fn protection() -> ProtectionReport {
    let p = win_sys::security::protection();
    // Security Center vrací tentýž produkt klidně třikrát (registrace
    // per komponenta) — duplicity jsou šum, ne informace.
    let mut av: Vec<(String, bool, bool, bool)> = Vec::new();
    for a in p.av {
        let row = (a.name, a.enabled, a.up_to_date, a.leftover);
        if !av.contains(&row) {
            av.push(row);
        }
    }
    ProtectionReport {
        av,
        defender: p
            .defender
            .map(|d| (d.realtime, d.signature_age_days, d.quick_scan_age_days)),
        fw_domain: p.firewall.domain,
        fw_private: p.firewall.private,
        fw_public: p.firewall.public,
        uac_enabled: p.uac.enabled,
        uac_admin_prompt: p.uac.admin_prompt,
        secure_boot: p.secure_boot,
        tpm: p.tpm,
        encryption: p
            .volumes
            .into_iter()
            .map(|v| (v.letter, v.protection))
            .collect(),
    }
}

/// Co právě běží — podklad pro křížovou kontrolu „používá teď".
///
/// Sám registr na to nestačí. `LastUsedTimeStop == 0` znamená „relace
/// neskončila", ne „aplikace žije": když program spadne nebo ho někdo
/// odinstaluje, konec se nedopíše a záznam tvrdí „používá právě teď"
/// donekonečna. Proto se ptáme i sampleru — a když daný program neběží,
/// mikrofon držet nemůže.
#[derive(Debug, Default, Clone)]
pub struct RunningApps {
    /// Jména běžících .exe, malými písmeny.
    names: std::collections::HashSet<String>,
    /// Rodiny balených aplikací (identity_key `msix:…`), malými písmeny.
    families: std::collections::HashSet<String>,
}

impl RunningApps {
    pub fn from_procs(procs: &[core_types::proc::ProcRow]) -> RunningApps {
        let mut names = std::collections::HashSet::new();
        let mut families = std::collections::HashSet::new();
        for p in procs {
            names.insert(p.name.to_ascii_lowercase());
            if let Some(f) = p.identity_key.strip_prefix("msix:") {
                families.insert(f.to_ascii_lowercase());
            }
        }
        RunningApps { names, families }
    }

    /// Běží program, kterému ten souhlas patří?
    ///
    /// U balených aplikací se porovnává rodina balíčku, u klasických
    /// jméno .exe z cesty v ConsentStore. Prázdný seznam procesů (sampler
    /// ještě nedoběhl) znamená „nevím" — a tam se raději nic neshazuje,
    /// aby živá tečka u kamery neproblikávala.
    fn has(&self, app: &str, packaged: bool) -> bool {
        if self.names.is_empty() && self.families.is_empty() {
            return true;
        }
        if packaged {
            let fam = app.to_ascii_lowercase();
            return self.families.iter().any(|f| *f == fam);
        }
        let exe = app
            .rsplit(char::from(92u8))
            .next()
            .unwrap_or(app)
            .to_ascii_lowercase();
        !exe.is_empty() && self.names.contains(&exe)
    }
}

/// Oprávnění aplikací z ConsentStore. Levné (registr) — jde volat
/// při každém dotazu.
pub fn permissions(running: &RunningApps) -> Vec<PermissionRow> {
    // Registr se čte napříč hivemi VŠECH uživatelů, takže tentýž program
    // dorazí jednou za každý profil, ve kterém má záznam. Do seznamu
    // patří jednou — a to tou verzí, která o něm ví nejvíc: používá se
    // teď, případně byla použita naposledy. Dvojí řádek by nebyl jen
    // šum; UI je klíčuje dvojicí schopnost + cesta a shodný klíč
    // v seznamu shodí vykreslování celé stránky.
    let mut best: std::collections::HashMap<(String, String), PermissionRow> =
        std::collections::HashMap::new();
    for c in win_sys::consent::consents() {
        let app_name = friendly_name(&c.app, c.packaged);
        let group_key = group_key(&c.app, c.packaged);
        let live = c.in_use && running.has(&c.app, c.packaged);
        let row = PermissionRow {
            capability: c.capability,
            app: c.app,
            app_name,
            group_key,
            enforced: c.packaged,
            allow: c.allow,
            // Registr říká jen „relace neskončila". Že aplikace opravdu
            // žije, ví jen sampler — bez toho zůstane po pádu viset
            // „používá právě teď" navždy.
            in_use: live,
            last_used: c.last_used,
            last_start: c.last_start,
        };
        match best.entry((row.capability.clone(), row.app.clone())) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(row);
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                let old = e.get();
                let better = row.in_use && !old.in_use
                    || row.in_use == old.in_use && row.last_used > old.last_used;
                if better {
                    e.insert(row);
                }
            }
        }
    }
    best.into_values().collect()
}

/// Klíč, pod kterým patří záznamy téže aplikace k sobě.
///
/// ConsentStore klíčuje oprávnění CESTOU k .exe. Aplikace, která se
/// instaluje do složky s číslem verze, tak po každé aktualizaci založí
/// nový záznam a ten starý zůstane ležet — naměřeno 141 řádků pro
/// mikrofon Discordu (`…\Discord\app-1.0.9184\Discord.exe`), nejstarší
/// z roku 2020, a 46 pro Overwolf.
///
/// Sdružuje se proto podle cesty, ve které je číslo verze nahrazené
/// hvězdičkou. Sdružovat podle jména .exe by nešlo: dvě různé Javy
/// (`jdk-17…\javaw.exe` vs `zulu21…\javaw.exe`) ani dvě různá OBS
/// (obs-studio vs Streamlabs) nejsou tytéž aplikace, přestože se
/// jejich soubory jmenují stejně.
fn group_key(app: &str, packaged: bool) -> String {
    // Balené aplikace mají v klíči PackageFamilyName — ten je napříč
    // verzemi stejný, není co srovnávat.
    if packaged {
        return app.to_ascii_lowercase();
    }
    app.split('\\')
        .map(|seg| if is_version_segment(seg) { "*" } else { seg })
        .collect::<Vec<_>>()
        .join("\\")
        .to_ascii_lowercase()
}

/// Vypadá segment cesty jako samotné číslo verze?
///
/// Projít smí jen to, co kromě verze nenese žádný jiný význam:
/// „app-1.0.9184" a „0.169.0.24" ano, „jdk-17.0.8.7-hotspot" ne —
/// tím se totiž odlišují dvě různé Javy a sloučit se nesmějí.
fn is_version_segment(seg: &str) -> bool {
    let rest = seg.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let rest = rest.strip_prefix(['-', '_']).unwrap_or(rest);
    !rest.is_empty()
        && rest.contains('.')
        && rest.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Čitelné jméno: u cesty poslední komponenta bez .exe, u PFN část
/// před podtržítkem (hash vydavatele je šum).
fn friendly_name(app: &str, packaged: bool) -> String {
    if packaged {
        let base = app.split('_').next().unwrap_or(app);
        // „Microsoft.WindowsCamera" → „WindowsCamera".
        base.rsplit('.').next().unwrap_or(base).to_string()
    } else {
        std::path::Path::new(app)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| app.to_string())
    }
}

/// Celý report najednou.
pub fn report(running: &RunningApps) -> SecurityReport {
    SecurityReport {
        protection: protection(),
        permissions: permissions(running),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Jména: cesta → jméno souboru, PFN → čitelná část.
    #[test]
    fn friendly_names() {
        assert_eq!(
            friendly_name(r"C:\Program Files\OBS\obs64.exe", false),
            "obs64"
        );
        assert_eq!(
            friendly_name("Microsoft.WindowsCamera_8wekyb3d8bbwe", true),
            "WindowsCamera"
        );
    }

    // Verze téže aplikace patří k sobě, různé aplikace se stejně
    // pojmenovaným .exe ne. Data z reálného ConsentStore.
    #[test]
    fn versions_group_but_different_apps_do_not() {
        let d1 = group_key(r"C:\Users\IVA\AppData\Local\Discord\app-0.0.307\Discord.exe", false);
        let d2 = group_key(r"C:\Users\IVA\AppData\Local\Discord\app-1.0.9184\Discord.exe", false);
        assert_eq!(d1, d2, "verze Discordu se musí sloučit");

        let o1 = group_key(r"C:\Program Files (x86)\Overwolf\0.166.1.16\obs\bin\64bit\ow-obs.exe", false);
        let o2 = group_key(r"C:\Program Files (x86)\Overwolf\0.169.0.24\obs\bin\64bit\ow-obs.exe", false);
        assert_eq!(o1, o2, "verze Overwolfu se musí sloučit");

        // Dvě různé Javy — shodné jméno souboru, jiná aplikace.
        let j1 = group_key(r"C:\Program Files\Eclipse Adoptium\jdk-17.0.8.7-hotspot\bin\javaw.exe", false);
        let j2 = group_key(
            r"C:\Users\IVA\AppData\Roaming\ModrinthApp\meta\java_versions\zulu21.36.17-ca-jre21.0.4-win_x64\bin\javaw.exe",
            false,
        );
        assert_ne!(j1, j2, "dvě různé Javy se sloučit nesmějí");

        // OBS Studio vs Streamlabs OBS.
        let b1 = group_key(r"C:\Program Files\obs-studio\bin\64bit\obs64.exe", false);
        let b2 = group_key(
            r"C:\Program Files\Streamlabs OBS\resources\app.asar.unpacked\node_modules\obs-studio-node\obs64.exe",
            false,
        );
        assert_ne!(b1, b2, "OBS Studio a Streamlabs nejsou táž aplikace");

        // „64bit" ani „app.asar.unpacked" nejsou čísla verze.
        assert!(!is_version_segment("64bit"));
        assert!(!is_version_segment("app.asar.unpacked"));
        assert!(!is_version_segment("Apex Legends"));
        assert!(is_version_segment("app-1.0.9184"));
        assert!(is_version_segment("0.169.0.24"));
        assert!(is_version_segment("v1.2"));
    }

    // Report jde sestavit a enforced nese jen balené aplikace.
    #[test]
    fn report_builds_and_enforcement_is_honest() {
        win_sys::wic::init_com_for_thread();
        let r = report();
        for p in &r.permissions {
            if p.enforced {
                assert!(
                    !p.app.contains('\\'),
                    "cesta k .exe označená jako vynucená: {:?}",
                    p.app
                );
            }
        }
    }
}
