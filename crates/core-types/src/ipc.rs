//! IPC protokol — typy zpráv pro named pipe (SPEC kap. 10).
//!
//! v0 obsahuje jen ping/pong („žiješ?“). Enumy jsou od začátku
//! `#[non_exhaustive]`-friendly tvarem (další varianty přibudou ve v1+),
//! serializace postcard — proto derive serde na všem.

use serde::{Deserialize, Serialize};

/// Verze IPC protokolu. UI a služba si ji vymění při připojení;
/// neshoda znamená „čekám na dokončení aktualizace“ (INFRA kap. 4.3).
pub const PROTOCOL_VERSION: u32 = 4;

/// Požadavek UI → služba.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// „Žiješ?“ — nese verzi protokolu klienta.
    Ping { protocol_version: u32 },
    /// Aktuální snapshot procesů (1 Hz sampler, SPEC kap. 3.1).
    QueryProcs,
    /// Aktuální systémové metriky (CPU, RAM).
    QuerySystem,
    /// Vlastní spotřeba nástroje (SPEC kap. 2.3) — rozpočet musí být
    /// ověřitelný uživatelem, ne slibovaný.
    QuerySelfUsage,
    /// Historie systémových metrik ze system_1s (unix rozsah, včetně).
    QuerySystemHistory { from: i64, to: i64 },
    /// Stav procesů v konkrétním čase (nejbližší vzorek ±2 s).
    QueryProcsAt { ts: i64 },
}

/// Odpověď služba → UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    /// Odpověď na Ping — verze protokolu a uptime služby v sekundách.
    Pong {
        protocol_version: u32,
        uptime_s: u64,
    },
    Procs(Vec<crate::proc::ProcRow>),
    System(crate::proc::SystemSnapshot),
    /// Vlastní spotřeba: CPU %, working set a velikost databáze.
    SelfUsage {
        cpu_pct: f32,
        ws_bytes: u64,
        db_bytes: u64,
    },
    SystemHistory(Vec<crate::proc::SystemPoint>),
    /// Stav procesů z historie; `ts` = skutečný čas nalezeného vzorku.
    ProcsAt {
        ts: i64,
        rows: Vec<crate::proc::HistProcRow>,
    },
    /// Chyba zpracování požadavku. Nic neselhává mlčky (SPEC kap. 22).
    Error {
        message: String,
    },
}
