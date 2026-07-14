//! ipc — protokol named pipe mezi službou a UI (SPEC kap. 10).
//!
//! Pipe `\\.\pipe\syswatch`, rámce délkově prefixované (u32 LE),
//! serializace postcard. DACL na pipe omezuje přístup: SYSTEM a
//! Administrators plný, interaktivní uživatelé čtení+zápis (SPEC 2.1).
//! v0 umí jediný request: Ping → Pong.

pub mod client;
pub mod frame;
pub mod server;

/// Název pipe. Jediné místo v kódu, kde je zapsaný.
pub const PIPE_NAME: &str = r"\\.\pipe\syswatch";

/// Maximální velikost rámce. Pojistka proti podvrženému prefixu délky —
/// pipe je útočná plocha (SPEC kap. 21 bod 10). v0 zprávy mají desítky
/// bajtů; limit se zvedne, až protokol poroste.
pub const MAX_FRAME_LEN: u32 = 64 * 1024;

/// Chyby IPC vrstvy.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O chyba na pipe: {0}")]
    Io(#[from] std::io::Error),
    #[error("rámec deklaruje {len} B, limit je {max} B")]
    FrameTooLarge { len: u32, max: u32 },
    #[error("chyba serializace postcard: {0}")]
    Codec(#[from] postcard::Error),
    #[error("Win32 volání `{call}` selhalo: {source}")]
    Win32 {
        call: &'static str,
        source: windows::core::Error,
    },
    #[error("služba neběží nebo pipe neexistuje")]
    NotAvailable,
    #[error(
        "pipe {PIPE_NAME} už existuje — běží jiná instance démona? \
         (nainstalovanou službu zastav: .\\service.ps1 -Stop)"
    )]
    PipeAlreadyExists,
    #[error("služba vrátila chybu: {message}")]
    Remote { message: String },
}
