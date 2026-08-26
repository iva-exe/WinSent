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

/// Co ten NTSTATUS znamená, česky. `None` = kód, který nepoznáváme —
/// pak stačí hexa zápis a nic si nevymýšlíme.
///
/// Detail incidentu nesl exit kód jen desítkově, tedy `-1073741819`.
/// To je pro čtenáře záznamu nepoužitelné číslo; přitom právě tenhle
/// kód je nejčastější pád vůbec a jmenuje se ACCESS_VIOLATION.
pub fn ntstatus_meaning(code: u32) -> Option<&'static str> {
    Some(match code {
        0xC000_0005 => "sáhnutí do paměti, která procesu nepatří (ACCESS_VIOLATION)",
        0xC000_0006 => "stránka souboru nešla načíst (IN_PAGE_ERROR) — často vadný disk",
        0xC000_0017 => "došla paměť (NO_MEMORY)",
        0xC000_001D => "procesor dostal neplatnou instrukci (ILLEGAL_INSTRUCTION)",
        0xC000_0025 => "výjimku nešlo předat dál (NONCONTINUABLE_EXCEPTION)",
        0xC000_0026 => "chybný stav výjimky (INVALID_DISPOSITION)",
        0xC000_008C => "sáhnutí za konec pole (ARRAY_BOUNDS_EXCEEDED)",
        0xC000_008E => "dělení nulou v plovoucí čárce (FLOAT_DIVIDE_BY_ZERO)",
        0xC000_0094 => "dělení nulou (INTEGER_DIVIDE_BY_ZERO)",
        0xC000_0095 => "přetečení celého čísla (INTEGER_OVERFLOW)",
        0xC000_00FD => "přetečení zásobníku (STACK_OVERFLOW)",
        0xC000_0135 => "chybí knihovna DLL (DLL_NOT_FOUND)",
        0xC000_0142 => "knihovnu se nepodařilo inicializovat (DLL_INIT_FAILED)",
        0xC000_0374 => "poškozená halda (HEAP_CORRUPTION)",
        0xC000_0409 => "přepsaný zásobník, zásah ochrany (STACK_BUFFER_OVERRUN)",
        0xC000_041D => "výjimka uvnitř obsluhy zpětného volání",
        0xC000_0602 => "program se ukončil sám kvůli poškozenému stavu (FAIL_FAST)",
        0xC000_0022 => "přístup odepřen (ACCESS_DENIED)",
        _ => return None,
    })
}

/// Detail pádu jako JSON: kód desítkově i hexa, jméno procesu a
/// aplikace zachycené v okamžiku detekce, a pokud kód známe, i český
/// popis. Vzniklo kvůli tomu, že v záznamu stálo jen holé záporné číslo.
pub fn crash_detail(exit_code: u32, name: &str, app: &str) -> String {
    let mut s = format!(
        "{{\"exit_code\":{exit_code},\"exit_hex\":\"0x{exit_code:08X}\",\"name\":\"{}\",\"app\":\"{}\"",
        json_str(name),
        json_str(app)
    );
    if let Some(m) = ntstatus_meaning(exit_code) {
        s.push_str(&format!(",\"meaning\":\"{}\"", json_str(m)));
    }
    s.push('}');
    s
}

/// Minimální JSON escape pro řetězce do detail polí.
pub fn json_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
