//! IPC protokol — typy zpráv pro named pipe (SPEC kap. 10).
//!
//! v0 obsahuje jen ping/pong („žiješ?“). Enumy jsou od začátku
//! `#[non_exhaustive]`-friendly tvarem (další varianty přibudou ve v1+),
//! serializace postcard — proto derive serde na všem.

use serde::{Deserialize, Serialize};

/// Verze IPC protokolu. UI a služba si ji vymění při připojení;
/// neshoda znamená „čekám na dokončení aktualizace“ (INFRA kap. 4.3).
pub const PROTOCOL_VERSION: u32 = 14;

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
    /// Statické informace o komponentách (CPU/RAM/GPU/disky).
    QuerySysInfo,
    /// Detaily proměnných v čase (jádra, disky, GPU) — pro zámek grafu.
    QueryDetailAt { ts: i64 },
    /// Historie disků pro per-disk grafy.
    QueryDiskHistory { from: i64, to: i64 },
    /// Historie jader CPU (mini grafy při zámku času).
    QueryCoreHistory { from: i64, to: i64 },
    /// Ikona aplikace podle identity_key (extrahovaná z .exe, cache).
    QueryIcon { identity_key: String },
    /// Události (záseky, pády) v rozsahu — markery na časové ose (v3).
    QueryEvents { from: i64, to: i64 },
    /// Poslední incidenty (nejnovější první), max `limit`.
    QueryIncidents { limit: u32 },
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
    SysInfo(crate::proc::StaticInfo),
    /// Detaily v čase: jádra, disky, GPU (co historie má).
    DetailAt {
        ts: i64,
        cores: Vec<f32>,
        disks: Vec<crate::proc::DiskRate>,
        gpu: Option<crate::proc::GpuInfo>,
    },
    /// Body historie disků: (ts, index, r_bps, w_bps).
    DiskHistory(Vec<(i64, u32, u64, u64)>),
    /// Body historie jader: (ts, jádro, pct).
    CoreHistory(Vec<(i64, u32, f32)>),
    /// Ikona aplikace (None = zkoušeno, ikona není).
    Icon(Option<crate::proc::IconData>),
    /// Události v rozsahu (v3).
    Events(Vec<crate::proc::EventRow>),
    /// Incidenty (v3).
    Incidents(Vec<crate::proc::IncidentRow>),
    /// Chyba zpracování požadavku. Nic neselhává mlčky (SPEC kap. 22).
    Error {
        message: String,
    },
}
