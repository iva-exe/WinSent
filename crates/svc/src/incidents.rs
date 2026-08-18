//! Logika incidentů v démonu (SPEC kap. 3.3, 16): klasifikace záseku
//! z metrik sampleru, filtr pádů procesů z ETW exit kódů a startovní
//! sken BSOD (minidumpy + nečisté vypnutí).

use core_types::proc::{ProcRow, SystemSnapshot};

/// Zpráva pro zapisovací vlákno store — vzorky i události jdou jedním
/// kanálem, aby DB měla jediného zapisovatele.

/// Jedno sezení použití oprávnění pro zápis do databáze (v9D).
#[derive(Debug, Clone)]
pub struct PermUseEntry {
    pub app: String,
    pub capability: String,
    pub start_ts: i64,
    /// `None` = aplikace ji drží právě teď.
    pub stop_ts: Option<i64>,
}

pub enum StoreMsg {
    Tick(i64, Vec<ProcRow>, SystemSnapshot),
    /// Výsledek skenu inventáře (v4) — nahradí obsah app/app_path.
    Inventory(Vec<store::apps::ScanApp>),
    /// Spočtená velikost cesty aplikace (lazy cache).
    PathSize {
        identity_key: String,
        path: String,
        size_bytes: u64,
        ts: i64,
    },
    /// Smazání záznamu incidentu (vlastní DB, žádná mutace OS).
    DeleteIncident(i64),
    /// Sezení, ve kterých aplikace držely kameru, mikrofon nebo polohu
    /// (v9D) — celá dávka najednou.
    ///
    /// Schválně dávka, ne zpráva na sezení: ConsentStore jich má na
    /// běžném stroji přes dvě stovky a kanál má šestnáct míst. Posílat
    /// je po jedné znamenalo, že se drtivá většina tiše zahodila a
    /// historie nevznikla vůbec.
    PermUse(Vec<PermUseEntry>),
    Event {
        ts: i64,
        kind: &'static str,
        pid: Option<u32>,
        detail: String,
    },
    Incident {
        ts: i64,
        kind: &'static str,
        identity_key: Option<String>,
        culprit: Option<String>,
        detail: String,
        /// Odkaz na .etl černé skříňky (forenzní okno, SPEC 16.3).
        etl_path: Option<String>,
        window_from: i64,
        window_to: i64,
    },
}

/// Výsledek klasifikace záseku (SPEC 3.3 — pořadí dle četnosti příčin;
/// CPU je poslední, protože bývá nejméně častým viníkem).
pub struct StallVerdict {
    pub cause: &'static str,
    /// Viník: (pid, jméno, identity_key) — top proces dle metriky příčiny.
    pub culprit: Option<(u32, String, String)>,
    /// Top 3 procesy dle příslušné metriky, pro detail incidentu.
    pub top: Vec<(u32, String, f64)>,
}

/// Klasifikuje zásek z aktuálního vzorku.
pub fn classify_stall(sys: &SystemSnapshot, procs: &[ProcRow]) -> StallVerdict {
    // 1. paging — hard faulty skočily
    if sys.hard_flt_rate > 500.0 {
        let top = top_by(procs, |p| p.ws_bytes as f64);
        return StallVerdict {
            cause: "paging",
            culprit: top.first().map(to_culprit(procs)),
            top,
        };
    }
    // 2. I/O saturace — fronta nebo latence disku
    if sys.disk_qlen > 8.0 || sys.disk_lat_ms > 200.0 {
        let top = top_by(procs, |p| (p.disk_r_bps + p.disk_w_bps) as f64);
        return StallVerdict {
            cause: "io",
            culprit: top.first().map(to_culprit(procs)),
            top,
        };
    }
    // 3. thermal throttle
    if sys.thermal_throttle {
        let top = top_by(procs, |p| p.cpu_pct as f64);
        return StallVerdict {
            cause: "thermal",
            culprit: top.first().map(to_culprit(procs)),
            top,
        };
    }
    // 4. CPU saturace
    if sys.cpu_pct > 95.0 {
        let top = top_by(procs, |p| p.cpu_pct as f64);
        return StallVerdict {
            cause: "cpu",
            culprit: top.first().map(to_culprit(procs)),
            top,
        };
    }
    // 5. neznámé (typicky driver/DPC) — zaznamenat, netvrdit příčinu.
    StallVerdict {
        cause: "unknown",
        culprit: None,
        top: top_by(procs, |p| p.cpu_pct as f64),
    }
}

/// Top 3 procesy podle metriky (jen nenulové).
fn top_by(procs: &[ProcRow], metric: impl Fn(&ProcRow) -> f64) -> Vec<(u32, String, f64)> {
    let mut rows: Vec<(u32, String, f64)> = procs
        .iter()
        .map(|p| (p.pid, p.app_name.clone(), metric(p)))
        .filter(|(_, _, v)| *v > 0.0)
        .collect();
    rows.sort_by(|a, b| b.2.total_cmp(&a.2));
    rows.truncate(3);
    rows
}

/// Doplní k top záznamu identity_key z řádků procesů.
fn to_culprit(procs: &[ProcRow]) -> impl Fn(&(u32, String, f64)) -> (u32, String, String) + '_ {
    move |(pid, name, _)| {
        let key = procs
            .iter()
            .find(|p| p.pid == *pid)
            .map(|p| p.identity_key.clone())
            .unwrap_or_default();
        (*pid, name.clone(), key)
    }
}

/// Je exit kód pádem? Jen NTSTATUS severity error (0xC…) — 0xE… jsou
/// „user defined" kódy, které běžně vracejí kompilátory a skripty
/// (E2E ukázal záplavu z rustc workerů). Výjimka 0xC000013A = ukončení
/// Ctrl+C/zavřením konzole, to je běžné a chtěné.
pub fn is_crash_exit(code: u32) -> bool {
    (code >> 28) == 0xC && code != 0xC000_013A
}

/// Minimální JSON escape pro řetězce do detail polí.
pub fn json_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
