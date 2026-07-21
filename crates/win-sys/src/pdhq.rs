//! Obecná držená PDH query na skalární čítače (bez wildcard instancí —
//! na ty je gpuproc). v3: `\Memory\Page Reads/sec` = hard faulty
//! obsluhované diskem, klíčový signál klasifikace záseku (SPEC 3.3).
//!
//! Rate countery potřebují dva sběry — první collect jen plní baseline.

use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};

/// Otevřená query s čítačem hard faultů.
pub struct MemFaults {
    query: PDH_HQUERY,
    page_reads: PDH_HCOUNTER,
    primed: bool,
}

// SAFETY: PDH handly se používají výhradně z jednoho vlákna.
unsafe impl Send for MemFaults {}

impl Drop for MemFaults {
    fn drop(&mut self) {
        // SAFETY: query jsme otevřeli my.
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}

impl MemFaults {
    /// Otevře query. None = čítač na systému není.
    pub fn init() -> Option<MemFaults> {
        // SAFETY: standardní PDH sekvence; při chybě query zavřeme.
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut page_reads = PDH_HCOUNTER::default();
            if PdhAddEnglishCounterW(query, w!("\\Memory\\Page Reads/sec"), 0, &mut page_reads) != 0
            {
                let _ = PdhCloseQuery(query);
                return None;
            }
            let _ = PdhCollectQueryData(query);
            Some(MemFaults {
                query,
                page_reads,
                primed: false,
            })
        }
    }

    /// Hard faulty za sekundu. None při prvním volání / chybě.
    pub fn sample(&mut self) -> Option<f64> {
        // SAFETY: handly patří této query; výstup je lokální.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            if !self.primed {
                self.primed = true;
                return None;
            }
            let mut val = PDH_FMT_COUNTERVALUE::default();
            if PdhGetFormattedCounterValue(self.page_reads, PDH_FMT_DOUBLE, None, &mut val) != 0
                || val.CStatus != 0
            {
                return None;
            }
            Some(val.Anonymous.doubleValue)
        }
    }
}
