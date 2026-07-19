//! Per-proces využití GPU přes PDH counter `\GPU Engine(*)\Utilization
//! Percentage` (SPEC kap. 3.1). Přesně to, co dělá Správce úloh: jméno
//! instance nese `pid_<PID>_..._engtype_<typ>`, hodnoty všech engine
//! instancí daného PID se sečtou. Userspace, žádný kernel driver.
//!
//! Query se drží otevřená; každý tick jeden collect. „Utilization
//! Percentage“ je rate counter — první collect vrátí neplatná data,
//! od druhého jsou hodnoty platné (mezi ticky je ~1 s, to stačí).

use std::collections::HashMap;

use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA,
};

/// Otevřená PDH query na GPU engine utilization.
pub struct GpuPerProc {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
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
    /// Otevře query a přidá wildcard counter. None = PDH/counter není.
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
            // První sběr — naplní baseline pro rate.
            let _ = PdhCollectQueryData(query);
            Some(GpuPerProc {
                query,
                counter,
                primed: false,
            })
        }
    }

    /// Sebere aktuální vzorek → mapa PID → GPU % (součet přes engine).
    /// Prázdná mapa při prvním volání nebo chybě (nikdy nepanikaří).
    pub fn sample(&mut self) -> HashMap<u32, f32> {
        let mut out = HashMap::new();
        // SAFETY: dvoufázové čtení pole; buffer má vrácenou velikost.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return out;
            }
            if !self.primed {
                self.primed = true;
                return out; // rate ještě není platný
            }

            let mut size = 0u32;
            let mut count = 0u32;
            let rc = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                None,
            );
            if rc != PDH_MORE_DATA || size == 0 {
                return out;
            }

            // Buffer musí být zarovnaný na PDH_FMT_COUNTERVALUE_ITEM_W.
            let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let cap = (size as usize).div_ceil(item);
            let mut buf = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); cap.max(1)];
            let rc = PdhGetFormattedCounterArrayW(
                self.counter,
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
                if let Some(pid) = pid_from_instance(&name) {
                    let val = it.FmtValue.Anonymous.doubleValue as f32;
                    *out.entry(pid).or_insert(0.0) += val;
                }
            }
        }
        out
    }
}

/// Vyparsuje PID z názvu instance „pid_1234_luid_..._engtype_3D".
fn pid_from_instance(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("pid_")?;
    let end = rest.find('_').unwrap_or(rest.len());
    rest[..end].parse().ok()
}
