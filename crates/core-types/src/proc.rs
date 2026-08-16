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
    /// SOUKROMÁ pracovní sada v bajtech — paměť, kterou proces nesdílí
    /// s nikým jiným. Právě tohle číslo ukazuje Správce úloh ve sloupci
    /// „Paměť" a jen tohle jde bezpečně sčítat přes procesy jedné
    /// aplikace; celá pracovní sada by sdílené stránky započítala
    /// u každého procesu znovu.
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
    /// Instalační adresář v registru je, ale na disku NEEXISTUJE —
    /// zbytek po ručně smazané aplikaci (typicky hry). Odinstalátor
    /// takovou položku často nechá v systému viset.
    pub missing_install: bool,
}

/// Proces, který drží soubor (v8, SPEC kap. 18.1 — Restart Manager).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HolderRow {
    pub pid: u32,
    pub name: String,
    /// critical | service | window | console | explorer | unknown
    pub kind: String,
    pub service: Option<String>,
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

/// Základní deska, firmware a stroj (v9, SPEC kap. 15.1).
/// Prázdný řetězec = deska to nehlásí; nic se nedopočítává.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardInfo {
    pub manufacturer: String,
    pub product: String,
    pub version: String,
    pub bios_vendor: String,
    pub bios_version: String,
    pub bios_date: String,
    pub system_manufacturer: String,
    pub system_product: String,
}

/// Stav baterie (v9, SPEC kap. 15.1). `None` u položek, které zařízení
/// nehlásí — u desktopu chybí celá struktura.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub percent: Option<u8>,
    pub ac_online: bool,
    pub charging: bool,
    pub remaining_s: Option<u32>,
    pub design_mwh: Option<u32>,
    pub full_mwh: Option<u32>,
    pub cycles: Option<u32>,
    /// Opotřebení v % (0 = jako nová). None, když chybí kapacity.
    pub wear_pct: Option<f32>,
}

/// Tepelný stav CPU (v9, SPEC kap. 15.2). Teplota je `None`, když ji
/// stroj nehlásí — `temp_source` vždy řekne, čemu uživatel věří.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuThermalInfo {
    pub celsius: Option<f32>,
    /// „HWiNFO" | „LibreHardwareMonitor" | „ACPI" | „nedostupné".
    pub temp_source: String,
    pub clock_mhz: u32,
    pub max_mhz: u32,
    pub throttling: bool,
}

/// Kompletní hardwarový přehled (v9, SPEC kap. 15). Skládá se ze
/// statického inventáře (čte se jednou) a stavu, který se obnovuje.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardwareReport {
    pub board: BoardInfo,
    pub battery: Option<BatteryInfo>,
    pub cpu_thermal: CpuThermalInfo,
    /// Zdraví disků — sdílené s v4 (SPEC 11.1), tady u komponenty.
    pub disks: Vec<DiskHealthRow>,
    /// Svazky pro obsazenost u každého disku.
    pub volumes: Vec<VolumeRow>,
    /// Všechna přítomná zařízení — jméno, model, výrobce, ovladač.
    pub devices: Vec<DeviceRow>,
    /// Kdy byl přehled sestaven (unix).
    pub ts: i64,
}

/// Jedno zařízení ze systémového stromu (v9, SPEC kap. 15.1) — tentýž
/// zdroj, ze kterého čte Správce zařízení.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRow {
    pub name: String,
    pub manufacturer: String,
    /// Technická třída („Display", „Net") a její lidský popis.
    pub class: String,
    pub class_desc: String,
    /// Hardwarové ID — obsahuje VID/PID, tedy skutečný model.
    pub hardware_id: String,
    pub driver_version: String,
    pub driver_date: String,
    /// 0 = běží v pořádku; jinak kód problému (vykřičník ve Správci).
    pub problem_code: u32,
}

/// Připojená obrazovka a její aktuální režim (v9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayRow {
    pub adapter: String,
    pub monitor: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub primary: bool,
}

/// Jedno síťové spojení nebo naslouchající port (v9, SPEC kap. 12).
/// Adresy jako text — UI je jen zobrazuje a postcard je tak přenese
/// bez vlastních typů.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnRow {
    /// "tcp" | "udp".
    pub proto: String,
    pub local: String,
    pub local_port: u16,
    /// Prázdné u naslouchajících portů a UDP.
    pub remote: String,
    pub remote_port: u16,
    /// PTR jméno vzdálené adresy, když už ho resolver zná.
    pub remote_name: Option<String>,
    /// TCP stav ("established", "listen"…), u UDP "udp".
    pub state: String,
    pub pid: u32,
}

/// Spojení jedné aplikace (v9) — seskupené podle identity (kap. 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppNetRow {
    pub identity_key: String,
    pub app_name: String,
    pub publisher: Option<String>,
    /// Kolik procesů aplikace má aspoň jedno spojení.
    pub proc_count: u32,
    /// Aktivní spojení (established) / naslouchající porty.
    pub established: u32,
    pub listening: u32,
    /// Trafik aplikace teď: bajty za sekundu (ETW Kernel-Network).
    pub rx_bps: u64,
    pub tx_bps: u64,
    pub conns: Vec<ConnRow>,
}

/// Síťový adaptér s IP konfigurací (v9, sekce Connection).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetAdapterRow {
    pub name: String,
    pub description: String,
    pub mac: String,
    /// "ethernet" | "wifi" | "virtual" | "other".
    pub kind: String,
    pub up: bool,
    /// Rychlost linky v Mb/s (0 = nehlásí).
    pub link_mbps: u64,
    pub ips: Vec<String>,
    pub gateways: Vec<String>,
    pub dns: Vec<String>,
    pub dhcp: bool,
}

/// WiFi síť viditelná z poslední cache skenování (v9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiNetworkRow {
    pub ssid: String,
    pub signal_pct: u32,
    pub secured: bool,
    pub connected: bool,
}

/// Stav připojení (v9, sekce Connection): adaptéry + WiFi.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionReport {
    pub adapters: Vec<NetAdapterRow>,
    /// Má stroj vůbec WiFi kartu? Bez ní se sekce WiFi nepředstírá.
    pub wifi_present: bool,
    /// Aktuální připojení: (popis karty, SSID, signál %, rx/tx Mb/s).
    pub wifi_connection: Option<WifiNetworkRow>,
    pub wifi_networks: Vec<WifiNetworkRow>,
}

/// Stav ochrany Windows (v9, SPEC kap. 13.1). `None` = nejde zjistit
/// nebo neexistuje (legacy BIOS nemá Secure Boot) — nikdy se nehádá.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProtectionReport {
    /// Antiviry ze Security Center: (jméno, běží, aktuální definice).
    pub av: Vec<(String, bool, bool)>,
    /// Defender detaily, když je aktivní: (realtime, stáří definic
    /// ve dnech, stáří rychlého skenu ve dnech).
    pub defender: Option<(bool, Option<u32>, Option<u32>)>,
    /// Firewall per profil: doména, privátní, veřejná.
    pub fw_domain: Option<bool>,
    pub fw_private: Option<bool>,
    pub fw_public: Option<bool>,
    pub uac_enabled: bool,
    /// 0 = bez výzvy … 5; 2 je výchozí souhlas na zabezpečené ploše.
    pub uac_admin_prompt: Option<u32>,
    pub secure_boot: Option<bool>,
    /// (zapnutý, verze specifikace).
    pub tpm: Option<(bool, String)>,
    /// BitLocker per svazek: (písmeno, 0 nešifrováno / 1 chráněno / 2 jiné).
    pub encryption: Vec<(String, u32)>,
}

/// Oprávnění jedné aplikace k jedné schopnosti (v9, SPEC kap. 13.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRow {
    /// webcam | microphone | location | …
    pub capability: String,
    /// PackageFamilyName, nebo cesta k .exe u klasických aplikací.
    pub app: String,
    /// Čitelné jméno aplikace (z cesty nebo z PFN).
    pub app_name: String,
    /// Klíč, pod který patří všechny verze téže aplikace. ConsentStore
    /// klíčuje podle cesty, takže aplikace instalovaná do složky s číslem
    /// verze má vlastní záznam za každou verzi, kterou kdy měla —
    /// pod tímhle klíčem se v UI slijí do jednoho řádku.
    pub group_key: String,
    /// Balená aplikace — jen u té Windows Deny tvrdě VYNUTÍ.
    /// UI podle toho barví: zelená jen kde vynucení opravdu je.
    pub enforced: bool,
    pub allow: bool,
    /// Používá schopnost právě teď (živá tečka).
    pub in_use: bool,
    /// Konec posledního použití (unix).
    pub last_used: Option<i64>,
}

/// Security sekce (v9): ochrana + oprávnění.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SecurityReport {
    pub protection: ProtectionReport,
    pub permissions: Vec<PermissionRow>,
}
