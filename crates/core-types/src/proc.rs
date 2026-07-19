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
    /// Disk I/O v bajtech za sekundu (čtení + zápis zvlášť), z delty
    /// kumulativních čítačů mezi vzorky.
    pub disk_r_bps: u64,
    pub disk_w_bps: u64,
    /// Identita aplikace, pod kterou proces patří (v2, SPEC kap. 4).
    pub identity_key: String,
    /// Zobrazované jméno aplikace (např. „Google Chrome“, „Windows“).
    pub app_name: String,
    /// Vydavatel z podpisu/VERSIONINFO, když je znám.
    pub publisher: Option<String>,
    /// Ochranná třída procesu (SPEC kap. 4.3).
    pub protection: Protection,
    /// Jak jistá je identita (SPEC kap. 4.4) — `guess`/`path` se v UI
    /// odliší tečkovaným podtrhem.
    pub confidence: Confidence,
}

/// Ochranná třída procesu (SPEC kap. 4.3) — zatím jen pro zobrazení.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Protection {
    /// Kill = BSOD. Šedě + zámek, bez tlačítka.
    Critical,
    /// PPL — kill nemožný.
    Protected,
    /// SYSTEM/SERVICE — kill za potvrzením.
    System,
    #[default]
    /// Běžný uživatelský proces.
    User,
}

/// Ikona aplikace: syrové RGBA pixely (top-down), UI je vykreslí na
/// canvas → data URL. Přenáší se jen jednou na identity_key (cache v UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconData {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Jistota identity (SPEC kap. 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Override, MSIX, OS, uninstall záznam nebo platný podpis.
    #[default]
    Exact,
    /// Jen podle adresáře binárky — nespolehlivé, vizuálně odlišené.
    Guess,
}

/// Doplňkové údaje GPU (NVML) pro detail sekci — jen živý pohled,
/// do historie se neukládá (senzory naostro přijdou ve v3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct GpuInfo {
    pub temp_c: Option<f32>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub power_w: Option<f32>,
    pub clock_mhz: Option<u32>,
}

/// Systémové metriky z téhož vzorku.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
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
    /// GPU využití v % (NVML). None = nedostupné — nikdy nepředstírat
    /// číslo, které nemáme (SPEC kap. 15.2).
    pub gpu_pct: Option<f32>,
    /// Zátěž jednotlivých logických jader v % (0–100).
    pub cores: Vec<f32>,
    /// Doplňkové GPU údaje (teplota, VRAM…), když je NVML.
    pub gpu: Option<GpuInfo>,
    /// Rychlosti fyzických disků (čtení/zápis B/s).
    pub disks: Vec<DiskRate>,
    /// Takt CPU: aktuální průměr a maximum (MHz).
    pub cpu_clock_mhz: u32,
    pub cpu_clock_max_mhz: u32,
    /// Uptime systému v sekundách.
    pub uptime_s: u64,
    /// Součty přes všechny procesy (parita se Správcem úloh).
    pub threads_total: u32,
    pub handles_total: u32,
}

/// Rychlost jednoho fyzického disku v aktuálním vzorku.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DiskRate {
    pub index: u32,
    pub r_bps: u64,
    pub w_bps: u64,
}

/// Jeden RAM modul (SMBIOS Type 17).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RamModuleInfo {
    pub size_mb: u64,
    pub speed_mts: u32,
    pub configured_mts: u32,
    pub slot: String,
    pub manufacturer: String,
    pub part_number: String,
}

/// Popis fyzického disku.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskDesc {
    pub index: u32,
    pub model: String,
}

/// Statické informace o komponentách — zjišťují se jednou při startu
/// služby (SPEC kap. 15.1), UI si je vyžádá přes QuerySysInfo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StaticInfo {
    pub cpu_name: String,
    pub cpu_base_mhz: u32,
    pub physical_cores: u32,
    pub logical_cores: u32,
    pub l1_kb: u32,
    pub l2_kb: u32,
    pub l3_kb: u32,
    pub ram_modules: Vec<RamModuleInfo>,
    pub ram_slots: u32,
    pub gpu_name: Option<String>,
    pub disks: Vec<DiskDesc>,
}

/// Jeden bod historie systémových metrik (z tabulky system_1s).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SystemPoint {
    pub ts: i64,
    pub cpu_pct: f32,
    pub mem_used_mb: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    pub gpu_pct: Option<f32>,
}

/// Řádek procesu z historie (tabulky sample_1s + proc_names).
/// Užší než ProcRow — historie nedrží vlákna ani session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistProcRow {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub ws_bytes: u64,
    pub disk_r_bps: u64,
    pub disk_w_bps: u64,
}
