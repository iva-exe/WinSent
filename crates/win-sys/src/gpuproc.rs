//! GPU přes PDH čítače — VENDOR-NEUTRÁLNÍ zdroj (NVIDIA, AMD, Intel,
//! Qualcomm — cokoliv s WDDM ovladačem). Stejná data čte Správce úloh.
//!
//! `\GPU Engine(*)\Utilization Percentage` (SPEC kap. 3.1): jméno
//! instance nese `pid_<PID>_..._engtype_<typ>`.
//!
//! Per-proces i celkové % se počítají STEJNĚ: hodnoty se sečtou uvnitř
//! jednoho fyzického enginu (luid adaptéru + eng_N) a přes enginy
//! i adaptéry se bere MAXIMUM. Enginy běží souběžně — 3D, Copy
//! a VideoDecode jsou oddělené jednotky, adaptér jich téhož typu mívá
//! několik a hybridní notebook má dvě GPU; jejich součet neodpovídá
//! tomu, „kolik GPU zabírá".
//!
//! Změřeno na Discordu: součet přes enginy 17,2 %, maximum 9,0 %,
//! Správce úloh u téhož procesu ve stejnou chvíli 6,8 % (Video
//! Decode). Součet nafukoval čísla zhruba dvojnásobně.
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
    /// PID → GPU % (maximum přes enginy, které proces používá).
    pub per_pid: HashMap<u32, f32>,
    /// Celkové GPU % (max přes enginy). None dokud není primed.
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

            let (per_pid, total_pct) = aggregate(read_counter_array(self.counter));
            out.per_pid = per_pid;
            out.total_pct = total_pct;

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

/// Identita fyzického enginu z názvu instance: úsek od `luid_` po
/// `_engtype_`, např. „luid_0x00000000_0x0000CECA_phys_0_eng_13".
///
/// Dřív se klíčovalo jen typem enginu. Jeden adaptér ale má několik
/// nezávislých enginů téhož typu (NVIDIA tu má 7× Copy) a druhý adaptér
/// (iGPU hybridního notebooku) vlastní 3D — sčítalo se tak přes různé
/// jednotky i různé GPU a hra 90 % + DWM na iGPU 25 % dalo clamp 100 %.
/// Neznámý formát spadne na jméno bez `pid_N_`, aby se nesouvisející
/// instance nesloučily.
fn engine_key_from_instance(name: &str) -> &str {
    if let (Some(start), Some(end)) = (name.find("luid_"), name.rfind("_engtype_")) {
        if start < end {
            return &name[start..end];
        }
    }
    match name.strip_prefix("pid_") {
        Some(rest) => rest.split_once('_').map_or(rest, |(_, r)| r),
        None => name,
    }
}

/// Instance → (PID → %, celkové %). Uvnitř jednoho fyzického enginu
/// (luid + eng_N) se sčítá — procesy se o engine dělí, proces může mít
/// i víc instancí téhož enginu — a přes enginy i adaptéry se bere
/// maximum. Stejně počítá Správce úloh.
fn aggregate(
    items: impl IntoIterator<Item = (String, f64)>,
) -> (HashMap<u32, f32>, Option<f32>) {
    let mut by_engine: HashMap<String, f64> = HashMap::new();
    let mut per_pid_engine: HashMap<(u32, String), f64> = HashMap::new();
    for (name, val) in items {
        let eng = engine_key_from_instance(&name).to_string();
        if let Some(pid) = pid_from_instance(&name) {
            *per_pid_engine.entry((pid, eng.clone())).or_insert(0.0) += val;
        }
        *by_engine.entry(eng).or_insert(0.0) += val;
    }
    let mut per_pid: HashMap<u32, f32> = HashMap::new();
    for ((pid, _), v) in per_pid_engine {
        let e = per_pid.entry(pid).or_insert(0.0);
        *e = e.max(v as f32);
    }
    let total = by_engine
        .values()
        .copied()
        .fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.max(v))))
        .map(|v| (v as f32).clamp(0.0, 100.0));
    (per_pid, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(pid: u32, luid: &str, eng: u32, ty: &str, v: f64) -> (String, f64) {
        (
            format!("pid_{pid}_luid_0x00000000_0x0000{luid}_phys_0_eng_{eng}_engtype_{ty}"),
            v,
        )
    }

    // Dva různé Copy enginy téhož adaptéru jsou nezávislé jednotky.
    #[test]
    fn engines_of_same_type_do_not_add_up() {
        let (per_pid, total) = aggregate([
            inst(10, "CECA", 4, "Copy", 30.0),
            inst(10, "CECA", 5, "Copy", 30.0),
        ]);
        assert_eq!(total, Some(30.0));
        assert_eq!(per_pid[&10], 30.0);
    }

    // 3D na dvou adaptérech (iGPU + dGPU) se nesčítá.
    #[test]
    fn adapters_do_not_add_up() {
        let (per_pid, total) = aggregate([
            inst(1, "DCE1", 0, "3D", 25.0),
            inst(2, "CECA", 0, "3D", 90.0),
        ]);
        assert_eq!(total, Some(90.0));
        assert_eq!(per_pid[&1], 25.0);
        assert_eq!(per_pid[&2], 90.0);
    }

    // Procesy na stejném enginu se o něj dělí — tam se sčítá.
    #[test]
    fn processes_on_one_engine_add_up() {
        let (per_pid, total) = aggregate([
            inst(1, "CECA", 0, "3D", 20.0),
            inst(2, "CECA", 0, "3D", 30.0),
        ]);
        assert_eq!(total, Some(50.0));
        assert_eq!(per_pid[&1], 20.0);
        assert_eq!(per_pid[&2], 30.0);
    }

    #[test]
    fn engine_key_parsing() {
        assert_eq!(
            engine_key_from_instance("pid_7_luid_0x0_0x1_phys_0_eng_13_engtype_Copy"),
            "luid_0x0_0x1_phys_0_eng_13"
        );
        assert_eq!(engine_key_from_instance("pid_7_neco_jineho"), "neco_jineho");
        assert_eq!(pid_from_instance("pid_1234_luid_x"), Some(1234));
    }
}
