//! Capability probing — feature-flag matice (INFRA kap. 1.4).
//!
//! Zjišťuje se jednou při startu služby, sdílí se read-only. Moduly se
//! ptají `Caps`, nikdy nedělají vlastní detekci verze. Naplní se ve v1
//! (RtlGetVersion, probe NtSuspendProcess, verze ETW schémat…).

/// Schopnosti aktuálního systému. v0: prázdný nosič, pole přibudou ve v1.
#[derive(Debug, Clone, Default)]
pub struct Caps {}

/// Sestaví `Caps` probingem běžícího systému. v0 nemá co zjišťovat.
pub fn probe() -> Caps {
    Caps::default()
}
