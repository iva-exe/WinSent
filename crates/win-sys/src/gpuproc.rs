//! GPU přes PDH čítače — VENDOR-NEUTRÁLNÍ zdroj (NVIDIA, AMD, Intel,
//! Qualcomm — cokoliv s WDDM ovladačem). Stejná data čte Správce úloh.
//!
//! `\GPU Engine(*)\Utilization Percentage` (SPEC kap. 3.1): jméno
//! instance nese `pid_<PID>_..._engtype_<typ>`. Per-proces % = součet
//! engine instancí daného PID; celkové GPU % = maximum přes součty
//! jednotlivých engine typů (metodika Správce úloh — 3D vs Copy vs
//! VideoDecode se nesčítají, bere se nejvytíženější).
//!
//! `\GPU Adapter Memory(*)\Dedicated Usage`: obsazená dedikovaná VRAM
//! per adaptér — bere se maximum (největší = diskrétní GPU).
//!
//! Query se drží otevřená; každý tick jeden collect. „Utilization
//! Percentage" je rate counter — první collect vrátí neplatná data,
//! od druhého jsou hodnoty platné (mezi ticky je ~1 s, to stačí).

use std::collections::HashMap;

use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA,
};

/// Jeden vzorek GPU čítačů.
#[derive(Debug, Default)]
pub struct GpuSample {
    /// PID → GPU % (součet přes enginy procesu).
    pub per_pid: HashMap<u32, f32>,
    /// Celkové GPU % (max přes engine typy). None dokud není primed.
    pub total_pct: Option<f32>,
    /// Obsazená dedikovaná VRAM největšího adaptéru v MB.
    pub vram_used_mb: Option<u64>,
}

/// Otevřená PDH query na GPU engine utilization + adapter memory.
pub struct GpuPerProc {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    /// Dedicated Usage; None = counter na systému není.
    mem_counter: Option<PDH_HCOUNTER>,
    /// První collect neposkytuje platná rate data.
    primed: bool,
}

// SAFETY: PDH handly se používají výhradně z jednoho sampler vlákna.
unsafe impl Send for GpuPerProc {}

impl Drop for GpuPerProc {
    fn drop(&mut self) {
        // SAFETY: query jsme otevřeli my.
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}

impl GpuPerProc {
    /// Otevře query a přidá wildcard countery. None = PDH/counter není.
    pub fn init() -> Option<GpuPerProc> {
        // SAFETY: standardní PDH sekvence; při chybě query zavřeme.
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut counter = PDH_HCOUNTER::default();
            let rc = PdhAddEnglishCounterW(
                query,
                w!("\\GPU Engine(*)\\Utilization Percentage"),
                0,
                &mut counter,
            );
            if rc != 0 {
                let _ = PdhCloseQuery(query);
                return None;
            }
            // VRAM je bonus — bez ní query pořád dává využití.
            let mut mem = PDH_HCOUNTER::default();
            let mem_counter = (PdhAddEnglishCounterW(
                query,
                w!("\\GPU Adapter Memory(*)\\Dedicated Usage"),
                0,
                &mut mem,
            ) == 0)
                .then_some(mem);
            // První sběr — naplní baseline pro rate.
            let _ = PdhCollectQueryData(query);
            Some(GpuPerProc {
                query,
                counter,
                mem_counter,
                primed: false,
            })
        }
    }

    /// Sebere aktuální vzorek. Prázdný výsledek při prvním volání nebo
    /// chybě (nikdy nepanikaří).
    pub fn sample(&mut self) -> GpuSample {
        let mut out = GpuSample::default();
        // SAFETY: dvoufázové čtení polí; buffery mají vrácené velikosti.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return out;
            }
            if !self.primed {
                self.primed = true;
                return out; // rate ještě není platný
            }

            // Engine utilization: per-PID součet + per-engtype součty.
            let mut by_engtype: HashMap<String, f64> = HashMap::new();
            for (name, val) in read_counter_array(self.counter) {
                if let Some(pid) = pid_from_instance(&name) {
                    *out.per_pid.entry(pid).or_insert(0.0) += val as f32;
                }
                if let Some(eng) = engtype_from_instance(&name) {
                    *by_engtype.entry(eng.to_string()).or_insert(0.0) += val;
                }
            }
            out.total_pct = by_engtype
                .values()
                .copied()
                .fold(None, |acc: Option<f64>, v| {
                    Some(acc.map_or(v, |a| a.max(v)))
                })
                .map(|v| (v as f32).clamp(0.0, 100.0));

            // VRAM: max přes adaptéry (diskrétní GPU má největší).
            if let Some(mem) = self.mem_counter {
                out.vram_used_mb = read_counter_array(mem)
                    .into_iter()
                    .map(|(_, v)| v as u64)
                    .max()
                    .filter(|&b| b > 0)
                    .map(|b| b / (1024 * 1024));
            }
        }
        out
    }
}

/// Dvoufázové čtení wildcard counteru → (jméno instance, hodnota).
/// SAFETY: counter patří otevřené query volajícího.
unsafe fn read_counter_array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let mut size = 0u32;
    let mut count = 0u32;
    let rc = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
    if rc != PDH_MORE_DATA || size == 0 {
        return out;
    }

    // Buffer musí být zarovnaný na PDH_FMT_COUNTERVALUE_ITEM_W.
    let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    let cap = (size as usize).div_ceil(item);
    let mut buf = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); cap.max(1)];
    let rc = PdhGetFormattedCounterArrayW(
        counter,
        PDH_FMT_DOUBLE,
        &mut size,
        &mut count,
        Some(buf.as_mut_ptr()),
    );
    if rc != 0 {
        return out;
    }

    for it in buf.iter().take(count as usize) {
        if it.FmtValue.CStatus != 0 {
            continue; // neplatná hodnota pro tuto instanci
        }
        let name = it.szName.to_string().unwrap_or_default();
        out.push((name, it.FmtValue.Anonymous.doubleValue));
    }
    out
}

/// Vyparsuje PID z názvu instance „pid_1234_luid_..._engtype_3D".
fn pid_from_instance(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("pid_")?;
    let end = rest.find('_').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Vyparsuje engine typ („3D", „Copy", …) z názvu instance.
fn engtype_from_instance(name: &str) -> Option<&str> {
    name.rsplit_once("engtype_").map(|(_, t)| t)
}
