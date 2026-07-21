//! IPC protokol — typy zpráv pro named pipe (SPEC kap. 10).
//!
//! v0 obsahuje jen ping/pong („žiješ?“). Enumy jsou od začátku
//! `#[non_exhaustive]`-friendly tvarem (další varianty přibudou ve v1+),
//! serializace postcard — proto derive serde na všem.

use serde::{Deserialize, Serialize};

/// Verze IPC protokolu. UI a služba si ji vymění při připojení;
/// neshoda znamená „čekám na dokončení aktualizace“ (INFRA kap. 4.3).
pub const PROTOCOL_VERSION: u32 = 18;

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
    /// Inventář aplikací (v4, SPEC kap. 5).
    QueryApps,
    /// Mapa souborů aplikace.
    QueryAppMap { identity_key: String },
    /// Spočítá velikosti cest aplikace (pomalé — on-demand) a vrátí
    /// čerstvou mapu; výsledky se cachují do DB.
    ComputeAppSizes { identity_key: String },
    /// Vyžádá nový sken inventáře na pozadí.
    RescanApps,
    /// Svazky + zdraví fyzických disků (v4, SPEC kap. 11.1).
    QueryVolumes,
    /// Postaví MFT index svazku (sekundy; index drží služba v paměti
    /// a po 5 min nečinnosti ho uvolní).
    BuildFileIndex { letter: char },
    /// Hledání v MFT indexu svazku.
    SearchFiles {
        letter: char,
        query: String,
        limit: u32,
    },
    /// Duplicity pod kořenem — dvoufázová čtecí analýza (SPEC 11.3).
    FindDuplicates { root: String, min_size: u64 },
    /// Stav auto-indexace + úklidové analýzy (v4E).
    QueryCleanup,
    /// Smaže záznam incidentu (jen náš DB záznam, žádná mutace OS).
    DeleteIncident { id: i64 },
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
    /// Inventář aplikací (v4).
    Apps(Vec<crate::proc::AppRow>),
    /// Mapa souborů aplikace (v4).
    AppMap(Vec<crate::proc::AppPathRow>),
    /// Potvrzení požadavku bez dat (RescanApps).
    Ack,
    /// Svazky + zdraví disků (v4).
    Volumes {
        volumes: Vec<crate::proc::VolumeRow>,
        health: Vec<crate::proc::DiskHealthRow>,
    },
    /// Index svazku postaven: (písmeno, počet záznamů).
    IndexInfo {
        letter: char,
        entries: u64,
    },
    /// Nálezy hledání (v4).
    Files(Vec<crate::proc::FileHit>),
    /// Skupiny duplicit: (velikost, cesty).
    Duplicates(Vec<(u64, Vec<String>)>),
    /// Stav úklidu: indexace svazků (písmeno, záznamů, hotovo),
    /// běží-li analýza a případný výsledek.
    Cleanup {
        indexing: Vec<(char, u64, bool)>,
        running: bool,
        report: Option<crate::proc::CleanupReport>,
    },
    /// Chyba zpracování požadavku. Nic neselhává mlčky (SPEC kap. 22).
    Error {
        message: String,
    },
}
