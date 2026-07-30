//! win-sys — tenké safe wrappery nad windows-rs.
//!
//! Sem patří veškerý raw Win32 kontakt pro obecné použití: NtQuery*,
//! SetupAPI, WUA, Task Scheduler (v1+) a capability probing (`caps`).
//! v0 obsahuje jen ověření Authenticode podpisu a prioritu vlákna.

pub mod battery;
pub mod caps;
pub mod cpuinfo;
pub mod devices;
pub mod disk;
pub mod etw;
pub mod gpu;
pub mod gpubasic;
pub mod gpuproc;
pub mod icon;
pub mod msi;
pub mod msix;
pub mod net;
pub mod pdhq;
pub mod proc;
pub mod procinfo;
pub mod recycle;
pub mod registry;
pub mod restore;
pub mod rm;
pub mod services;
pub mod shortcut;
pub mod smart;
pub mod smbios;
pub mod sysinfo;
pub mod tasksched;
pub mod thermal;
pub mod threading;
pub mod trust;
pub mod usn;
pub mod verinfo;
pub mod volumes;
pub mod wic;
pub mod wmi;

/// Chyby win-sys vrstvy. Každé selhání Win32 volání nese kód, nic
/// neselhává mlčky (SPEC kap. 22).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Win32 volání `{call}` selhalo: {code}")]
    Win32 { call: &'static str, code: i32 },
}
