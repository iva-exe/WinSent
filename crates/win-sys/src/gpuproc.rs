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
//! Detail hlavního adaptéru (stejná query, žádný sběr navíc):
//! rozpad enginů podle typu (3D, Copy, Video…) z týchž součtů jako
//! celkové %, `\GPU Adapter Memory(*)\Shared Usage` a VRAM per proces
//! z `\GPU Process Memory(*)\Local Usage`.
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

const MB: u64 = 1024 * 1024;

/// Vytížení enginů hlavního adaptéru podle typu v % — totéž, co
/// Správce úloh kreslí v grafech enginů. None = adaptér engine toho
/// typu nemá (a UI dlaždici nekreslí); Some(0) = má, ale stojí.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct EngineUse {
    pub d3: Option<f32>,
    pub copy: Option<f32>,
    pub video_dec: Option<f32>,
    pub video_enc: Option<f32>,
    /// Compute i Cuda.
    pub compute: Option<f32>,
}

/// Jeden vzorek GPU čítačů.
#[derive(Debug, Default)]
pub struct GpuSample {
    /// PID → GPU % (maximum přes enginy, které proces používá).
    pub per_pid: HashMap<u32, f32>,
    /// Celkové GPU % (max přes enginy). None dokud není primed.
    pub total_pct: Option<f32>,
    /// Obsazená dedikovaná VRAM největšího adaptéru v MB.
    pub vram_used_mb: Option<u64>,
    /// Enginy hlavního adaptéru podle typu.
    pub engines: EngineUse,
    /// Sdílená paměť, kterou hlavní adaptér právě drží, v MB.
    pub shared_used_mb: Option<u64>,
    /// (PID, MB) rezidentní VRAM procesů hlavního adaptéru, sestupně.
    /// Všechny procesy — strop a počet řeší volající, který zná
    /// celkovou VRAM.
    pub mem_by_pid: Vec<(u32, u64)>,
}

/// Otevřená PDH query na GPU engine utilization + adapter memory.
pub struct GpuPerProc {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    /// Dedicated Usage; None = counter na systému není.
    mem_counter: Option<PDH_HCOUNTER>,
    /// Shared Usage adaptérů; None = counter na systému není.
    shared_counter: Option<PDH_HCOUNTER>,
    /// Local Usage procesů; None = counter na systému není.
    proc_mem_counter: Option<PDH_HCOUNTER>,
    /// LUID hlavního adaptéru („luid_0x…_0x…") od D3DKMT; None = ten
    /// s největší obsazenou VRAM.
    main_luid: Option<String>,
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
            let mem_counter = pridej(query, w!("\\GPU Adapter Memory(*)\\Dedicated Usage"));
            // Detail hlavního adaptéru — taky bonus. Per proces se čte
            // „Local Usage" (rezidentní v lokálních segmentech = VRAM),
            // ne „Dedicated Usage": ten je na Win10 rozbitý (KB4490156)
            // a prohlížeč tu na 8GB kartě hlásil 74 GB.
            let shared_counter = pridej(query, w!("\\GPU Adapter Memory(*)\\Shared Usage"));
            let proc_mem_counter = pridej(query, w!("\\GPU Process Memory(*)\\Local Usage"));
            // První sběr — naplní baseline pro rate.
            let _ = PdhCollectQueryData(query);
            Some(GpuPerProc {
                query,
                counter,
                mem_counter,
                shared_counter,
                proc_mem_counter,
                main_luid: None,
                primed: false,
            })
        }
    }

    /// Hlavní adaptér pro rozpad enginů, sdílenou paměť a VRAM per
    /// proces — ten, ze kterého čte senzory D3DKMT, ať všechny údaje
    /// v detailu patří téže kartě. None = největší obsazená VRAM.
    pub fn set_main_luid(&mut self, luid: Option<String>) {
        self.main_luid = luid;
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

            let mem = self
                .mem_counter
                .map(|c| read_counter_array(c))
                .unwrap_or_default();
            // Hlavní adaptér: od D3DKMT, jinak ten s největší obsazenou
            // VRAM (stejné pravidlo jako u vram_used_mb).
            let main = match self.main_luid.as_deref() {
                Some(l) => Some(l),
                None => mem
                    .iter()
                    .filter(|(_, v)| *v > 0.0)
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .and_then(|(n, _)| luid_of(n)),
            };

            let (per_pid, total_pct, engines) = aggregate(&read_counter_array(self.counter), main);
            out.per_pid = per_pid;
            out.total_pct = total_pct;
            out.engines = engines;

            // VRAM: max přes adaptéry (diskrétní GPU má největší).
            out.vram_used_mb = mem
                .iter()
                .map(|(_, v)| *v as u64)
                .max()
                .filter(|&b| b > 0)
                .map(|b| b / MB);

            if let Some(main) = main {
                if let Some(c) = self.shared_counter {
                    out.shared_used_mb = read_counter_array(c)
                        .into_iter()
                        .filter(|(n, _)| je_adapter(n, main))
                        .map(|(_, v)| v as u64)
                        .max()
                        .map(|b| b / MB);
                }
                if let Some(c) = self.proc_mem_counter {
                    out.mem_by_pid = mem_by_pid(&read_counter_array(c), main);
                }
            }
        }
        out
    }
}

/// Přidá volitelný counter do query. None = na systému není.
/// SAFETY: `query` je otevřená query volajícího.
unsafe fn pridej(query: PDH_HQUERY, cesta: PCWSTR) -> Option<PDH_HCOUNTER> {
    let mut c = PDH_HCOUNTER::default();
    (PdhAddEnglishCounterW(query, cesta, 0, &mut c) == 0).then_some(c)
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
        // Platná jsou DVĚ stavová čísla: VALID_DATA (0) a NEW_DATA (1,
        // „platná a jiná než minule"). Dřív se bral jen nulový stav,
        // takže by vypadla právě hodnota, která se změnila.
        if !platny_stav(it.FmtValue.CStatus) {
            continue; // neplatná hodnota pro tuto instanci
        }
        let name = it.szName.to_string().unwrap_or_default();
        out.push((name, it.FmtValue.Anonymous.doubleValue));
    }
    out
}

/// Je hodnota PDH čítače platná? (PDH_CSTATUS_VALID_DATA, PDH_CSTATUS_NEW_DATA)
pub(crate) fn platny_stav(cstatus: u32) -> bool {
    use windows::Win32::System::Performance::{PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA};
    cstatus == PDH_CSTATUS_VALID_DATA || cstatus == PDH_CSTATUS_NEW_DATA
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

/// Typ enginu z názvu instance („…_engtype_VideoDecode" → „VideoDecode").
fn engtype_from_instance(name: &str) -> &str {
    name.rfind("_engtype_")
        .map_or("", |i| &name[i + "_engtype_".len()..])
}

/// LUID adaptéru z názvu instance nebo klíče enginu, bez `_phys_N`
/// a dalšího: „luid_0x00000000_0x0000C958". None = instance LUID nemá.
fn luid_of(name: &str) -> Option<&str> {
    let rest = &name[name.find("luid_")?..];
    Some(
        rest.match_indices('_')
            .nth(2)
            .map_or(rest, |(i, _)| &rest[..i]),
    )
}

/// Patří instance hlavnímu adaptéru? Celý LUID, ne předpona
/// (`…_0x1` není `…_0x10`); velikost písmen se nebere v potaz.
fn je_adapter(name: &str, main: &str) -> bool {
    luid_of(name).is_some_and(|l| l.eq_ignore_ascii_case(main))
}

/// Skupina enginu v rozpadu podle typu.
#[derive(Debug, PartialEq)]
enum Skupina {
    D3,
    Copy,
    VideoDec,
    VideoEnc,
    /// AMD má jediný „Video Codec" na dekódování i kódování.
    VideoCodec,
    Compute,
    /// LegacyOverlay, Security, VR, Graphics_1, VideoProcessing… —
    /// Správce úloh je nabízí jen ve výběru, do dlaždic nejdou.
    Jine,
}

/// Typ enginu → skupina. Názvy se liší podle vendora („VideoDecode"
/// u NVIDIA, „Video Decode" u Intelu, „Compute_0" i „High Priority
/// Compute"), proto bez mezer a podtržítek a podle podřetězce.
fn skupina(engtype: &str) -> Skupina {
    // Normalizace do bufferu na zásobníku: volá se pro každý engine
    // hlavního adaptéru každý tick a String by byl alokace navíc.
    // Typy enginů mají pár desítek znaků; delší konec se ořízne.
    let mut buf = [0u8; 64];
    let mut n = 0;
    for b in engtype.bytes().filter(|&b| b != b' ' && b != b'_') {
        if n == buf.len() {
            break;
        }
        buf[n] = b.to_ascii_lowercase();
        n += 1;
    }
    let t = &buf[..n];
    let has = |s: &[u8]| t.windows(s.len()).any(|w| w == s);
    if has(b"videocodec") {
        Skupina::VideoCodec
    } else if has(b"videodecode") {
        Skupina::VideoDec
    } else if has(b"videoencode") {
        Skupina::VideoEnc
    } else if has(b"3d") {
        Skupina::D3
    } else if has(b"copy") {
        Skupina::Copy
    } else if has(b"compute") || has(b"cuda") {
        Skupina::Compute
    } else {
        Skupina::Jine
    }
}

/// Instance → (PID → %, celkové %, enginy hlavního adaptéru podle
/// typu). Uvnitř jednoho fyzického enginu (luid + eng_N) se sčítá —
/// procesy se o engine dělí, proces může mít i víc instancí téhož
/// enginu — a přes enginy i adaptéry se bere maximum. Stejně počítá
/// Správce úloh.
///
/// Typ enginu se skládá stejně: maximum přes enginy toho typu (7× Copy
/// u NVIDIA jsou nezávislé jednotky), jen z hlavního adaptéru —
/// 3D integrované grafiky do dlaždice diskrétní karty nepatří.
fn aggregate(
    items: &[(String, f64)],
    main: Option<&str>,
) -> (HashMap<u32, f32>, Option<f32>, EngineUse) {
    // Klíče si půjčují jména instancí — žádná alokace na instanci.
    // engine → (součet přes procesy, typ enginu)
    let mut by_engine: HashMap<&str, (f64, &str)> = HashMap::new();
    let mut per_pid_engine: HashMap<(u32, &str), f64> = HashMap::new();
    for (name, val) in items {
        let eng = engine_key_from_instance(name);
        if let Some(pid) = pid_from_instance(name) {
            *per_pid_engine.entry((pid, eng)).or_insert(0.0) += val;
        }
        by_engine
            .entry(eng)
            .or_insert((0.0, engtype_from_instance(name)))
            .0 += val;
    }
    let mut per_pid: HashMap<u32, f32> = HashMap::new();
    for ((pid, _), v) in per_pid_engine {
        let e = per_pid.entry(pid).or_insert(0.0);
        *e = e.max(v as f32);
    }
    let total = by_engine
        .values()
        .map(|&(v, _)| v)
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        })
        .map(|v| (v as f32).clamp(0.0, 100.0));

    let mut engines = EngineUse::default();
    if let Some(main) = main {
        let max = |slot: &mut Option<f32>, v: f32| *slot = Some(slot.map_or(v, |a| a.max(v)));
        for (&eng, &(sum, ty)) in &by_engine {
            if !je_adapter(eng, main) {
                continue;
            }
            let v = (sum as f32).clamp(0.0, 100.0);
            match skupina(ty) {
                Skupina::D3 => max(&mut engines.d3, v),
                Skupina::Copy => max(&mut engines.copy, v),
                Skupina::VideoDec => max(&mut engines.video_dec, v),
                Skupina::VideoEnc => max(&mut engines.video_enc, v),
                Skupina::VideoCodec => {
                    max(&mut engines.video_dec, v);
                    max(&mut engines.video_enc, v);
                }
                Skupina::Compute => max(&mut engines.compute, v),
                Skupina::Jine => {}
            }
        }
    }
    (per_pid, total, engines)
}

/// Instance `\GPU Process Memory(*)\Local Usage` → (PID, MB) hlavního
/// adaptéru, sestupně. Proces může mít instanci na každé fyzické části
/// adaptéru — sčítá se.
fn mem_by_pid(items: &[(String, f64)], main: &str) -> Vec<(u32, u64)> {
    let mut by_pid: HashMap<u32, f64> = HashMap::new();
    for (name, val) in items {
        if !je_adapter(name, main) {
            continue;
        }
        if let Some(pid) = pid_from_instance(name) {
            *by_pid.entry(pid).or_insert(0.0) += val;
        }
    }
    let mut out: Vec<(u32, u64)> = by_pid
        .into_iter()
        .map(|(pid, b)| (pid, b as u64 / MB))
        .filter(|&(_, mb)| mb > 0)
        .collect();
    // Při shodě podle PID, ať se pořadí mezi ticky nepřehazuje náhodně.
    out.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
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

    const CECA: &str = "luid_0x00000000_0x0000CECA";

    // Dva různé Copy enginy téhož adaptéru jsou nezávislé jednotky.
    #[test]
    fn engines_of_same_type_do_not_add_up() {
        let (per_pid, total, _) = aggregate(
            &[
                inst(10, "CECA", 4, "Copy", 30.0),
                inst(10, "CECA", 5, "Copy", 30.0),
            ],
            None,
        );
        assert_eq!(total, Some(30.0));
        assert_eq!(per_pid[&10], 30.0);
    }

    // 3D na dvou adaptérech (iGPU + dGPU) se nesčítá.
    #[test]
    fn adapters_do_not_add_up() {
        let (per_pid, total, _) = aggregate(
            &[
                inst(1, "DCE1", 0, "3D", 25.0),
                inst(2, "CECA", 0, "3D", 90.0),
            ],
            None,
        );
        assert_eq!(total, Some(90.0));
        assert_eq!(per_pid[&1], 25.0);
        assert_eq!(per_pid[&2], 90.0);
    }

    // Procesy na stejném enginu se o něj dělí — tam se sčítá.
    #[test]
    fn processes_on_one_engine_add_up() {
        let (per_pid, total, _) = aggregate(
            &[
                inst(1, "CECA", 0, "3D", 20.0),
                inst(2, "CECA", 0, "3D", 30.0),
            ],
            None,
        );
        assert_eq!(total, Some(50.0));
        assert_eq!(per_pid[&1], 20.0);
        assert_eq!(per_pid[&2], 30.0);
    }

    // Typ enginu = nejvytíženější engine toho typu, ne součet přes
    // sedm Copy enginů. Uvnitř jednoho enginu se procesy sčítají.
    #[test]
    fn engine_type_is_max_not_sum() {
        let (_, _, eng) = aggregate(
            &[
                inst(1, "CECA", 4, "Copy", 20.0),
                inst(2, "CECA", 4, "Copy", 15.0),
                inst(1, "CECA", 5, "Copy", 30.0),
                inst(1, "CECA", 6, "Copy", 0.0),
            ],
            Some(CECA),
        );
        assert_eq!(eng.copy, Some(35.0));
        // Typy, které adaptér nemá, zůstávají None (dlaždice se nekreslí).
        assert_eq!(eng.d3, None);
        assert_eq!(eng.video_dec, None);
    }

    // Enginy jiného adaptéru (iGPU) do rozpadu hlavního nepatří; LUID
    // se porovnává celý a bez ohledu na velikost písmen.
    #[test]
    fn other_adapter_is_ignored() {
        let (_, total, eng) = aggregate(
            &[
                inst(1, "DCE1", 0, "3D", 90.0),
                inst(2, "CECA", 0, "3D", 10.0),
                inst(2, "CECA1", 0, "3D", 70.0),
            ],
            Some("LUID_0X00000000_0X0000ceca"),
        );
        assert_eq!(eng.d3, Some(10.0));
        // Celkové % se nemění — pořád max přes všechno.
        assert_eq!(total, Some(90.0));
        // Bez hlavního adaptéru rozpad není.
        let (_, _, eng) = aggregate(&[inst(2, "CECA", 0, "3D", 10.0)], None);
        assert_eq!(eng, EngineUse::default());
    }

    // AMD „Video Codec" plní dekódování i kódování; Cuda je výpočet;
    // neznámé typy se ignorují.
    #[test]
    fn engine_type_classification() {
        let (_, _, eng) = aggregate(
            &[
                inst(1, "CECA", 3, "Video Codec", 12.0),
                inst(1, "CECA", 10, "Cuda", 40.0),
                inst(1, "CECA", 1, "Compute_0", 10.0),
                inst(1, "CECA", 7, "Security", 99.0),
                inst(1, "CECA", 0, "High Priority 3D", 5.0),
            ],
            Some(CECA),
        );
        assert_eq!(eng.video_dec, Some(12.0));
        assert_eq!(eng.video_enc, Some(12.0));
        assert_eq!(eng.compute, Some(40.0));
        assert_eq!(eng.d3, Some(5.0));
        assert_eq!(eng.copy, None);
        assert_eq!(skupina("VideoDecode"), Skupina::VideoDec);
        assert_eq!(skupina("Video Encode"), Skupina::VideoEnc);
        assert_eq!(skupina("LegacyOverlay"), Skupina::Jine);
        assert_eq!(skupina("VideoProcessing"), Skupina::Jine);
        assert_eq!(skupina("High Priority Compute"), Skupina::Compute);
        assert_eq!(skupina("Copy_1"), Skupina::Copy);
        assert_eq!(skupina(""), Skupina::Jine);
    }

    // VRAM per proces: jen hlavní adaptér, části adaptéru se sčítají,
    // sestupně, nuly pryč.
    #[test]
    fn mem_by_pid_main_adapter_only() {
        let mb = |v: f64| v * 1024.0 * 1024.0;
        let items = [
            (format!("pid_10_{CECA}_phys_0"), mb(100.0)),
            (format!("pid_10_{CECA}_phys_1"), mb(50.0)),
            (format!("pid_20_{CECA}_phys_0"), mb(400.0)),
            (format!("pid_30_{CECA}_phys_0"), 0.0),
            (
                "pid_40_luid_0x00000000_0x0000DCE1_phys_0".to_string(),
                mb(900.0),
            ),
        ];
        assert_eq!(mem_by_pid(&items, CECA), vec![(20, 400), (10, 150)]);
    }

    #[test]
    fn engine_key_parsing() {
        assert_eq!(
            engine_key_from_instance("pid_7_luid_0x0_0x1_phys_0_eng_13_engtype_Copy"),
            "luid_0x0_0x1_phys_0_eng_13"
        );
        assert_eq!(engine_key_from_instance("pid_7_neco_jineho"), "neco_jineho");
        assert_eq!(pid_from_instance("pid_1234_luid_x"), Some(1234));
        assert_eq!(luid_of("luid_0x0_0x1_phys_0_eng_13"), Some("luid_0x0_0x1"));
        assert_eq!(luid_of("pid_7_luid_0x0_0x1"), Some("luid_0x0_0x1"));
        assert_eq!(luid_of("pid_7_neco"), None);
        assert_eq!(
            engtype_from_instance("x_eng_3_engtype_Video Codec"),
            "Video Codec"
        );
    }
}
