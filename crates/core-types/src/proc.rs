//! Typy pro živé procesy a systémové metriky (v1, SPEC kap. 3.1).

use serde::{Deserialize, Serialize};

/// Jeden proces v aktuálním snapshotu sampleru.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcRow {
    pub pid: u32,
    pub parent_pid: u32,
    /// Jméno image (bez cesty). Prázdné jméno u pid 0 = System Idle.
    pub name: String,
    /// CPU v % celkové kapacity všech jader (0–100), z delty vzorků.
    pub cpu_pct: f32,
    /// Working set v bajtech.
    pub ws_bytes: u64,
    /// Private bytes (commit) v bajtech.
    pub priv_bytes: u64,
    pub threads: u32,
    pub session_id: u32,
}

/// Systémové metriky z téhož vzorku.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct SystemSnapshot {
    /// Celkové CPU v % (0–100).
    pub cpu_pct: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub proc_count: u32,
    /// Síť: celkový download/upload v bajtech za sekundu (všechna
    /// fyzická rozhraní kromě loopbacku).
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
}

/// Jeden bod historie systémových metrik (z tabulky system_1s).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SystemPoint {
    pub ts: i64,
    pub cpu_pct: f32,
    pub mem_used_mb: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
}

/// Řádek procesu z historie (tabulky sample_1s + proc_names).
/// Užší než ProcRow — historie nedrží vlákna ani session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistProcRow {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub ws_bytes: u64,
}
