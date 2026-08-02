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
    let mut av: Vec<(String, bool, bool)> = Vec::new();
    for a in p.av {
        let row = (a.name, a.enabled, a.up_to_date);
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

/// Oprávnění aplikací z ConsentStore. Levné (registr) — jde volat
/// při každém dotazu.
pub fn permissions() -> Vec<PermissionRow> {
    win_sys::consent::consents()
        .into_iter()
        .map(|c| {
            let app_name = friendly_name(&c.app, c.packaged);
            PermissionRow {
                capability: c.capability,
                app: c.app,
                app_name,
                enforced: c.packaged,
                allow: c.allow,
                in_use: c.in_use,
                last_used: c.last_used,
            }
        })
        .collect()
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
pub fn report() -> SecurityReport {
    SecurityReport {
        protection: protection(),
        permissions: permissions(),
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
