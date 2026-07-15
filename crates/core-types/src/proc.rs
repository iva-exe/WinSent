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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    /// Celkové CPU v % (0–100).
    pub cpu_pct: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub proc_count: u32,
}

impl Default for SystemSnapshot {
    fn default() -> Self {
        Self {
            cpu_pct: 0.0,
            mem_used_mb: 0,
            mem_total_mb: 0,
            proc_count: 0,
        }
    }
}
