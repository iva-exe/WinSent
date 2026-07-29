//! Typy pro živé procesy a systémové metriky (v1, SPEC kap. 3.1).

use serde::{Deserialize, Serialize};

/// Jeden proces v aktuálním snapshotu sampleru.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcRow {
    pub pid: u32,
    pub parent_pid: u32,
    /// Čas vzniku procesu — s PID tvoří stabilní identitu instance
    /// (PID se recykluje; každá mutace se validuje proti téhle dvojici).
    pub create_time: i64,
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
    /// Využití GPU procesem v % (PDH GPU Engine, součet přes enginy).
    /// Jako Správce úloh; 0 když PDH counter není.
    pub gpu_pct: f32,
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
    /// Signály pro klasifikaci záseku (SPEC kap. 3.3, v3):
    /// hard faulty/s (PDH Page Reads/sec), max hloubka fronty disků,
    /// průměrná latence disku na operaci (ms), příznak thermal throttle.
    pub hard_flt_rate: f32,
    pub disk_qlen: f32,
    pub disk_lat_ms: f32,
    pub thermal_throttle: bool,
}

/// Událost na časové ose (zásek, pád procesu…) — markery v grafu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub pid: Option<u32>,
    /// JSON s doplňky (exit code, lag_ms, klasifikace…).
    pub detail: Option<String>,
}

/// Aplikace z inventáře (v4, SPEC kap. 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppRow {
    /// Klíč shodný s identitou procesů (`app:…` / `msix:…`) — spojuje
    /// inventář s během i ikonami.
    pub identity_key: String,
    /// desktop | msix | os
    pub kind: String,
    pub display_name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_ts: Option<i64>,
    /// Počet cest v mapě souborů (pro seznam bez načítání map).
    pub path_count: u32,
}

/// Jedna cesta z mapy souborů aplikace (v4, SPEC 5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppPathRow {
    /// Souborová cesta, nebo registry větev (role == "registry").
    pub path: String,
    /// install | config | data | cache | logs | registry
    pub role: String,
    /// msi | msix | registry | heuristic
    pub source: String,
    /// exact | high | guess — guess se v UI kreslí tečkovaně.
    pub confidence: String,
    /// Velikost (lazy, on-demand) + kdy byla spočtená.
    pub size_bytes: Option<u64>,
    pub size_ts: Option<i64>,
}

/// Logický svazek (v4, SPEC kap. 11.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolumeRow {
    pub letter: char,
    pub label: String,
    pub fs: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub fixed: bool,
    /// Fyzický disk pod svazkem (spojení se SMART kartou).
    pub disk_index: Option<u32>,
}

/// Zdraví fyzického disku (NVMe health log; SATA zatím None).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskHealthRow {
    pub index: u32,
    pub model: String,
    pub temp_c: Option<i32>,
    /// Opotřebení (0 = nový, 100+ = za návrhovou životností).
    pub used_pct: Option<u8>,
    pub spare_pct: Option<u8>,
    pub power_on_hours: Option<u64>,
    /// NVMe critical warning bity (0 = OK).
    pub critical: Option<u8>,
}

/// Nález hledání v MFT indexu (v4, SPEC kap. 11.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileHit {
    pub path: String,
    pub name: String,
    /// FILE_ATTRIBUTE_* bity (adresář/skrytý/systémový → barvy v UI).
    pub attrs: u32,
    pub size_bytes: Option<u64>,
}

/// Výsledek úklidové analýzy disků (v4E).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupReport {
    /// (velikost, cesty) — potvrzené duplicity (jméno+velikost+hash).
    pub dups: Vec<(u64, Vec<String>)>,
    pub zero_byte: Vec<String>,
    /// (cesta, velikost) — temp adresáře k úklidu.
    pub junk: Vec<(String, u64)>,
    pub finished_ts: i64,
    /// Největší soubory a složky po svazcích: (písmeno, cesta, velikost).
    pub big_files: Vec<(char, String, u64)>,
    pub big_dirs: Vec<(char, String, u64)>,
}

/// Startup položka (v6, SPEC kap. 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartupRow {
    /// `{source}|{name}` — klíč pro přepnutí.
    pub id: String,
    pub name: String,
    /// run_user | run_machine | folder_user | folder_common | task |
    /// service | msix | shell
    pub source: String,
    pub command: String,
    pub enabled: bool,
    /// Lze přepínat? (Winlogon hooky ne — jen varování.)
    pub toggleable: bool,
    /// Identita aplikace, která položku vlastní (ikona + seskupení).
    pub identity_key: Option<String>,
    pub app_name: Option<String>,
    pub publisher: Option<String>,
}

/// Incident (zásek s viníkem, pád aplikace, BSOD) — SPEC kap. 16.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IncidentRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub identity_key: Option<String>,
    pub culprit: Option<String>,
    pub detail: Option<String>,
    pub window_from: Option<i64>,
    pub window_to: Option<i64>,
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
    /// Identita aplikace (v2) — náhled minulosti seskupuje a ikonuje
    /// stejně jako živý list. Prázdné u vzorků před migrací 5.
    pub identity_key: Option<String>,
    pub app_name: Option<String>,
    pub publisher: Option<String>,
}
