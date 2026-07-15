//! win-sys — tenké safe wrappery nad windows-rs.
//!
//! Sem patří veškerý raw Win32 kontakt pro obecné použití: NtQuery*,
//! SetupAPI, WUA, Task Scheduler (v1+) a capability probing (`caps`).
//! v0 obsahuje jen ověření Authenticode podpisu a prioritu vlákna.

pub mod caps;
pub mod net;
pub mod proc;
pub mod sysinfo;
pub mod threading;
pub mod trust;

/// Chyby win-sys vrstvy. Každé selhání Win32 volání nese kód, nic
/// neselhává mlčky (SPEC kap. 22).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Win32 volání `{call}` selhalo: {code}")]
    Win32 { call: &'static str, code: i32 },
}
