//! Kontrola integrity při startu — Authenticode podpis vlastních
//! binárek (SPEC kap. 2.3).
//!
//! Během vývoje jsou binárky nepodepsané, takže výsledek je jen záznam
//! v logu. Až bude code signing (v11), stane se z Invalid/Unsigned
//! fatální chyba: služba se nespustí a nahlásí to.

use win_sys::trust::{verify_authenticode, SignatureStatus};

/// Ověří podpis vlastní binárky a výsledek zaloguje.
pub fn report_own_binaries() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "kontrola integrity: nelze zjistit cestu vlastní binárky");
            return;
        }
    };

    match verify_authenticode(&exe) {
        Ok(SignatureStatus::Valid) => {
            tracing::info!(exe = %exe.display(), "kontrola integrity: podpis platný");
        }
        Ok(SignatureStatus::Unsigned) => {
            // Očekávaný stav během vývoje — varování, ne fatální chyba.
            tracing::warn!(
                exe = %exe.display(),
                "kontrola integrity: binárka není podepsaná (vývojový build; \
                 od v11 s code signingem bude neshoda fatální)"
            );
        }
        Ok(SignatureStatus::Invalid { code }) => {
            // Podpis existuje, ale nesedí — to už je podezřelé i ve vývoji.
            // v0 ještě nespouští fatální cestu (code signing není), ale
            // loguje se to jako error, ne warning.
            tracing::error!(
                exe = %exe.display(),
                code = format!("{code:#010x}"),
                "kontrola integrity: podpis binárky NEPLATNÝ — od v11 se \
                 služba s tímto stavem odmítne spustit"
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "kontrola integrity: ověření selhalo");
        }
    }
}
