//! collector-proc — sampler procesů 1 Hz přes NtQuerySystemInformation
//! (SPEC kap. 3.1). CPU % vzniká z delty kumulativních časů mezi dvěma
//! ticky; identita vzorku je (pid, create_time), takže recyklovaný PID
//! nezdědí cizí deltu. Buffer se alokuje jednou — v horké cestě nic.
//!
//! v1 mezikrok: tick vrací hotové řádky pro IPC. Ring buffer + flusher
//! do SQLite se na tenhle tvar napojí v dalším kroku v1.

use std::collections::HashMap;
use std::time::Instant;

use core_types::config::Config;
use core_types::proc::{DiskDesc, DiskRate, ProcRow, StaticInfo, SystemSnapshot};

pub mod stall;

/// Chyby sampleru.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
}


/// Co ze sběru na tomhle stroji nefunguje.
///
/// Existuje kvůli jedinému příznaku, který se hrozně blbě hledá na
/// dálku: aplikace hlásí „služba běží", ale sekce Tasks je prázdná.
/// Dřív to znamenalo, že tick padal celý a nikdo se nedozvěděl proč —
/// teď doplňky degradují a tady je záznam, který se dá poslat dál.
#[derive(Default)]
pub struct Degraded {
    /// Co selhalo → jak to systém popsal. Jedna položka na volání.
    failed: std::collections::BTreeMap<&'static str, String>,
}

impl Degraded {
    /// Zaznamená selhání a zaloguje ho POPRVÉ. Opakovat to každou
    /// sekundu by log utopilo a stejně by nic nepřidalo.
    pub fn warn_once(&mut self, what: &'static str, err: &impl std::fmt::Display) {
        let msg = err.to_string();
        if self.failed.insert(what, msg.clone()).is_none() {
            tracing::warn!(
                zdroj = what,
                error = %msg,
                "část sběru nefunguje — zbytek jede dál"
            );
        }
    }

    /// Seznam nefunkčních zdrojů pro UI („co tomuhle stroji nejde").
    pub fn list(&self) -> Vec<(String, String)> {
        self.failed
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }
}

/// Kumulativní čítače procesu z minulého ticku (pro delty).
#[derive(Clone, Copy)]
struct PrevProc {
    cpu_100ns: u64,
    io_read: u64,
    io_write: u64,
}

/// Stav sampleru mezi ticky.
pub struct State {
    /// Znovupoužívaný buffer pro NtQuerySystemInformation.
    buf: Vec<u8>,
    /// Čítače z minulého ticku: klíč (pid, create_time).
    prev_cpu: HashMap<(u32, i64), PrevProc>,
    prev_tick: Instant,
    prev_sys: win_sys::sysinfo::SystemTimes,
    prev_net: win_sys::net::NetMeter,
    /// Kdy se síťové čítače naposledy povedlo přečíst. Když čtení pár
    /// ticků selhává, NetMeter si drží starý stav a příští delta nese
    /// bajty za celou tu dobu — dělit ji jedním tickem by dalo špičku.
    prev_net_at: Instant,
    prev_cores: Vec<win_sys::sysinfo::CoreTimes>,
    /// NVML kontext; None = GPU metrika nedostupná (bez NVIDIA).
    gpu: Option<win_sys::gpu::Nvml>,
    /// Otevřené fyzické disky + minulé čítače (pro delty).
    disks: Vec<win_sys::disk::Disk>,
    prev_disks: Vec<win_sys::disk::DiskCounters>,
    /// Per-proces GPU přes PDH (SPEC kap. 3.1); None = counter není.
    /// Vendor-neutrální — slouží i jako fallback celkového GPU % a VRAM
    /// na strojích bez NVML (AMD, Intel).
    gpu_proc: Option<win_sys::gpuproc::GpuPerProc>,
    /// Celková VRAM z registru (fallback pro detail bez NVML).
    gpu_vram_total_mb: Option<u64>,
    /// Hard faulty/s přes PDH (signál paging, SPEC 3.3); None = není.
    mem_faults: Option<win_sys::pdhq::MemFaults>,
    /// Statické info komponent — zjištěno jednou při init.
    statics: StaticInfo,
    /// Engine identity aplikací (v2, SPEC kap. 4) — cache + background.
    identity: identity::Engine,
    /// Co na tomhle stroji ze sběru nefunguje (viz Degraded).
    degraded: Degraded,
    /// Počet logických jader — normalizace na % celkové kapacity.
    n_cpus: f64,
}

/// Statické informace o komponentách (pro QuerySysInfo).
/// Sdílená cache ikon aplikací (pro IPC handler ve službě).
pub fn icon_store(state: &State) -> identity::IconStore {
    state.identity.icons()
}

pub fn static_info(state: &State) -> StaticInfo {
    state.statics.clone()
}

/// Inicializace sampleru.
pub fn init(_cfg: &Config) -> Result<State, Error> {
    // Všechna logická jádra přes všechny procesorové skupiny.
    // available_parallelism() vrací jen jednu skupinu (max 64), takže na
    // strojích s víc vlákny vycházelo CPU % procesů dvojnásobné.
    let n_cpus = win_sys::cpuinfo::logical_count().max(1) as f64;
    let gpu = win_sys::gpu::Nvml::init();
    if gpu.is_none() {
        tracing::info!("NVML nedostupné — GPU pojede z PDH/registry fallbacku (vendor-neutrální)");
    }
    // Registry fallback: název + celková VRAM pro jakéhokoliv vendora.
    let gpu_basic = win_sys::gpubasic::detect();

    // Disky: otevřít jednou, čítače se pak čtou 1×/s.
    let disks = win_sys::disk::open_disks();
    let prev_disks = disks
        .iter()
        .map(|d| win_sys::disk::counters(d).unwrap_or_default())
        .collect();

    // Statické info komponent — jednou při startu (SPEC kap. 15.1).
    let cpu = win_sys::cpuinfo::cpu_static();
    let (ram_modules, ram_slots) = win_sys::smbios::ram_modules();
    let statics = StaticInfo {
        cpu_name: cpu.name,
        cpu_base_mhz: cpu.base_mhz,
        physical_cores: cpu.physical_cores,
        logical_cores: cpu.logical_cores,
        l1_kb: cpu.l1_kb,
        l2_kb: cpu.l2_kb,
        l3_kb: cpu.l3_kb,
        ram_modules: ram_modules
            .into_iter()
            .map(|m| core_types::proc::RamModuleInfo {
                size_mb: m.size_mb,
                speed_mts: m.speed_mts,
                configured_mts: m.configured_mts,
                slot: m.slot,
                manufacturer: m.manufacturer,
                part_number: m.part_number,
                mem_type: m.mem_type,
                clock_mhz: m.clock_mhz,
                configured_was_clock: m.configured_was_clock,
            })
            .collect(),
        ram_slots,
        gpu_name: gpu
            .as_ref()
            .and_then(|g| g.name())
            .or_else(|| gpu_basic.name.clone()),
        disks: disks
            .iter()
            .map(|d| DiskDesc {
                index: d.index,
                model: d.model.clone(),
            })
            .collect(),
    };

    Ok(State {
        buf: Vec::new(),
        prev_cpu: HashMap::new(),
        prev_tick: Instant::now(),
        prev_sys: win_sys::sysinfo::system_times()?,
        prev_net: win_sys::net::NetMeter::new()?,
        prev_net_at: Instant::now(),
        prev_cores: win_sys::sysinfo::core_times(n_cpus as usize)?,
        gpu,
        disks,
        prev_disks,
        gpu_proc: win_sys::gpuproc::GpuPerProc::init(),
        gpu_vram_total_mb: gpu_basic.vram_total_mb,
        mem_faults: win_sys::pdhq::MemFaults::init(),
        statics,
        identity: identity::Engine::new(identity::load_tables()),
        degraded: Degraded::default(),
        n_cpus,
    })
}

/// Jeden vzorek: všechny procesy + systémové metriky.
pub fn tick(state: &mut State) -> Result<(Vec<ProcRow>, SystemSnapshot), Error> {
    let raw = win_sys::proc::snapshot_processes(&mut state.buf)?;
    let now = Instant::now();
    // Delta stěny v jednotkách 100 ns; klamp proti dělení nulou.
    let wall_100ns = (now.duration_since(state.prev_tick).as_nanos() / 100).max(1) as f64;
    state.prev_tick = now;

    // GPU přes PDH (vendor-neutrální): per-PID % + celkové % + VRAM.
    let gpu_pdh = state
        .gpu_proc
        .as_mut()
        .map(|g| g.sample())
        .unwrap_or_default();
    let gpu_by_pid = &gpu_pdh.per_pid;

    // Delty per proces (CPU %, disk B/s); nový proces (nebo recyklovaný
    // PID) začíná od nuly — bez minulého vzorku deltu nemá.
    let wall_s = wall_100ns / 1e7;
    let mut next_cpu = HashMap::with_capacity(raw.len());
    let mut rows = Vec::with_capacity(raw.len());
    for p in &raw {
        let key = (p.pid, p.create_time);
        let (cpu_pct, disk_r_bps, disk_w_bps) = match state.prev_cpu.get(&key) {
            Some(prev) if p.cpu_time_100ns >= prev.cpu_100ns => (
                ((p.cpu_time_100ns - prev.cpu_100ns) as f64 / (wall_100ns * state.n_cpus) * 100.0)
                    as f32,
                (p.io_read_bytes.saturating_sub(prev.io_read) as f64 / wall_s) as u64,
                (p.io_write_bytes.saturating_sub(prev.io_write) as f64 / wall_s) as u64,
            ),
            _ => (0.0, 0, 0),
        };
        next_cpu.insert(
            key,
            PrevProc {
                cpu_100ns: p.cpu_time_100ns,
                io_read: p.io_read_bytes,
                io_write: p.io_write_bytes,
            },
        );

        // pid 0 (Idle) do tabulky nepatří — je to účetnictví nečinnosti,
        // ne proces; systémové CPU % ho už zohledňuje.
        if p.pid == 0 {
            continue;
        }
        // Identita (v2): jen lookup v cache; nováček dostane provisional
        // a dořeší se na pozadí (SPEC kap. 4.2 — nic drahého v cyklu).
        let (id, prot) = state.identity.identify(p.pid, &p.name, p.create_time);
        rows.push(ProcRow {
            pid: p.pid,
            parent_pid: p.parent_pid,
            create_time: p.create_time,
            name: p.name.clone(),
            cpu_pct: cpu_pct.clamp(0.0, 100.0),
            // Soukromá pracovní sada, ne celá.
            //
            // Celá pracovní sada obsahuje i stránky sdílené mezi
            // procesy (systémové DLL, sdílená paměť). U aplikace
            // z jednoho procesu je rozdíl malý, ale prohlížeč nebo hra
            // jich mají deset a součet pak počítá tytéž stránky
            // pořád dokola — aplikace se ukázala klidně dvakrát
            // větší, než ji hlásí Správce úloh. Ten sčítá právě tohle
            // pole (sloupec „Paměť" = soukromá pracovní sada).
            ws_bytes: p.ws_priv_bytes,
            priv_bytes: p.priv_bytes,
            threads: p.threads,
            session_id: p.session_id,
            disk_r_bps,
            disk_w_bps,
            gpu_pct: gpu_by_pid.get(&p.pid).copied().unwrap_or(0.0).min(100.0),
            identity_key: id.identity_key,
            app_name: id.app_name,
            publisher: id.publisher,
            protection: prot,
            confidence: id.confidence,
        });
    }
    // Obecné hostitelské procesy patří aplikaci, která je spustila.
    //
    // WebView2 běží jako `msedgewebview2.exe` mimo instalaci hostitele,
    // takže mu kaskáda identity dá vlastní klíč a v Tasks z toho vznikne
    // řádek „Microsoft Edge WebView2" — zatímco veškerá práce (a hlavně
    // GPU, protože vykresluje) patří aplikaci nad ním. Správce úloh ho
    // ukazuje vnořený pod hostitelem a my jsme u téhož procesu měli GPU
    // u cizího řádku: naše vlastní aplikace hlásila 0 %, zatímco Správce
    // úloh u ní ukazoval 8,6 %.
    //
    // Seznam je schválně krátký. `svchost`, `dllhost` ani `rundll32` sem
    // NEPATŘÍ — ty hostí služby a COM objekty a Správce úloh je taky
    // ukazuje samostatně.
    reparent_hosts(&mut rows);

    state.prev_cpu = next_cpu;

    // Úklid identity cache od zaniklých procesů (levné, jednou za tick).
    let live: std::collections::HashSet<u32> = raw.iter().map(|p| p.pid).collect();
    state.identity.retain_pids(&live);

    // Systém: busy = (kernel - idle) + user z delty GetSystemTimes.
    //
    // Od téhle chvíle se NIC nesmí propsat do `?`.
    //
    // Seznam procesů je v tuhle chvíli hotový a je to to jediné, na čem
    // sekci Tasks záleží. Když selže některý z doplňků — síťové součty,
    // zátěž jednotlivých jader — nesmí to smazat celý vzorek. Přesně to
    // se ale dělo: jediné selhání `net_totals()` na cizím stroji
    // znamenalo, že `tick` vrátil Err, sampler nepřepsal poslední vzorek
    // a Tasks zůstaly navždy prázdné. Ukazatel „služba běží" přitom
    // svítil zeleně, protože na ping odpovídá jiné vlákno.
    //
    // Každý doplněk proto degraduje sám za sebe a jednou to řekne do
    // logu (opakovat to každou sekundu by log utopilo).
    let sys = win_sys::sysinfo::system_times().unwrap_or(state.prev_sys);
    let idle_d = sys.idle.saturating_sub(state.prev_sys.idle) as f64;
    let kernel_d = sys.kernel.saturating_sub(state.prev_sys.kernel) as f64;
    let user_d = sys.user.saturating_sub(state.prev_sys.user) as f64;
    let total = kernel_d + user_d;
    let cpu_pct = if total > 0.0 {
        (((kernel_d - idle_d) + user_d) / total * 100.0) as f32
    } else {
        0.0
    };
    state.prev_sys = sys;

    let (mem_used_mb, mem_total_mb) = match win_sys::sysinfo::memory_status_mb() {
        Ok(v) => v,
        Err(e) => {
            state.degraded.warn_once("memory_status_mb", &e);
            (0, 0)
        }
    };

    // Síť: delta bajtů (po rozhraních, viz NetMeter) / delta stěny → B/s.
    //      Dělí se dobou od posledního ÚSPĚŠNÉHO čtení, ne délkou ticku.
    let (net_rx_bps, net_tx_bps) = match state.prev_net.delta() {
        Ok(d) => {
            let net_s = now.duration_since(state.prev_net_at).as_secs_f64().max(wall_s);
            state.prev_net_at = now;
            (
                (d.rx_bytes as f64 / net_s) as u64,
                (d.tx_bytes as f64 / net_s) as u64,
            )
        }
        Err(e) => {
            state.degraded.warn_once("net_totals", &e);
            (0, 0)
        }
    };

    // Per-core zátěž: busy = 1 − idle_d / (kernel_d + user_d).
    let cores_now = match win_sys::sysinfo::core_times(state.n_cpus as usize) {
        Ok(c) => c,
        Err(e) => {
            state.degraded.warn_once("core_times", &e);
            Vec::new()
        }
    };
    let cores = cores_now
        .iter()
        .zip(state.prev_cores.iter())
        .map(|(now, prev)| {
            let idle_d = now.idle.saturating_sub(prev.idle) as f64;
            let total_d = (now.kernel.saturating_sub(prev.kernel)
                + now.user.saturating_sub(prev.user)) as f64;
            if total_d > 0.0 {
                (((1.0 - idle_d / total_d) * 100.0) as f32).clamp(0.0, 100.0)
            } else {
                0.0
            }
        })
        .collect();
    state.prev_cores = cores_now;

    // Disky: delty kumulativních bajtů → B/s per disk. Zároveň signály
    // pro klasifikaci záseku (SPEC 3.3): max hloubka fronty a průměrná
    // latence na operaci = Δ(busy čas) / Δ(počet operací).
    let mut disk_rates = Vec::with_capacity(state.disks.len());
    let mut disk_qlen = 0u32;
    let mut lat_time_100ns = 0u64;
    let mut lat_ops = 0u64;
    for (i, d) in state.disks.iter().enumerate() {
        match win_sys::disk::counters(d) {
            Ok(now) => {
                let prev = state.prev_disks.get(i).copied().unwrap_or_default();
                disk_rates.push(DiskRate {
                    index: d.index,
                    r_bps: (now.read_bytes.saturating_sub(prev.read_bytes) as f64 / wall_s) as u64,
                    w_bps: (now.write_bytes.saturating_sub(prev.write_bytes) as f64 / wall_s)
                        as u64,
                });
                disk_qlen = disk_qlen.max(now.queue_depth);
                lat_time_100ns += now
                    .read_time_100ns
                    .saturating_sub(prev.read_time_100ns)
                    .saturating_add(now.write_time_100ns.saturating_sub(prev.write_time_100ns));
                lat_ops += (now.read_count.saturating_sub(prev.read_count)
                    + now.write_count.saturating_sub(prev.write_count))
                    as u64;
                state.prev_disks[i] = now;
            }
            Err(e) => tracing::debug!(disk = d.index, error = %e, "čtení čítačů disku selhalo"),
        }
    }
    let disk_lat_ms = if lat_ops > 0 {
        (lat_time_100ns as f64 / lat_ops as f64 / 10_000.0) as f32
    } else {
        0.0
    };

    // Hard faulty/s (paging signál). 0 dokud PDH není primed.
    let hard_flt_rate = state
        .mem_faults
        .as_mut()
        .and_then(|m| m.sample())
        .unwrap_or(0.0) as f32;

    // Takty CPU (stupeň 3 kaskády, SPEC 15.2) + uptime + součty.
    let (cpu_clock_mhz, cpu_clock_max_mhz) =
        win_sys::sysinfo::cpu_clocks(state.n_cpus as usize).unwrap_or((0, 0));
    let threads_total: u32 = raw.iter().map(|p| p.threads).sum();
    let handles_total: u32 = raw.iter().map(|p| p.handles).sum();

    // GPU detail: NVML dává vše (teplota, spotřeba, takt); bez NVML
    // (AMD/Intel) poctivý PDH+registry fallback — VRAM ano, senzory „—“
    // (ADLX/IGCL přijdou dle SPEC 15.2 později).
    let gpu_detail = match state.gpu.as_ref() {
        Some(g) => {
            let d = g.details();
            Some(core_types::proc::GpuInfo {
                temp_c: d.temp_c,
                vram_used_mb: d.vram_used_mb,
                vram_total_mb: d.vram_total_mb,
                power_w: d.power_w,
                clock_mhz: d.clock_mhz,
            })
        }
        None if gpu_pdh.total_pct.is_some() || state.gpu_vram_total_mb.is_some() => {
            Some(core_types::proc::GpuInfo {
                temp_c: None,
                vram_used_mb: gpu_pdh.vram_used_mb,
                vram_total_mb: state.gpu_vram_total_mb,
                power_w: None,
                clock_mhz: None,
            })
        }
        None => None,
    };

    let snapshot = SystemSnapshot {
        cpu_pct: cpu_pct.clamp(0.0, 100.0),
        mem_used_mb,
        mem_total_mb,
        proc_count: rows.len() as u32,
        net_rx_bps,
        net_tx_bps,
        // Celkové GPU %: NVML, jinak PDH (metodika Správce úloh) —
        // funguje na NVIDIA, AMD i Intel.
        gpu_pct: state
            .gpu
            .as_ref()
            .and_then(|g| g.utilization_pct())
            .or(gpu_pdh.total_pct),
        cores,
        gpu: gpu_detail,
        disks: disk_rates,
        cpu_clock_mhz,
        cpu_clock_max_mhz,
        uptime_s: win_sys::sysinfo::system_uptime_s(),
        threads_total,
        handles_total,
        hard_flt_rate,
        disk_qlen: disk_qlen as f32,
        disk_lat_ms,
        // Heuristika throttlingu: takt výrazně pod maximem PŘI zátěži.
        // Bez zátěže je nízký takt normální power management.
        thermal_throttle: cpu_clock_max_mhz > 0
            && cpu_clock_mhz < cpu_clock_max_mhz * 7 / 10
            && cpu_pct > 50.0,
    };
    Ok((rows, snapshot))
}

/// Korektní ukončení sampleru.
pub fn shutdown(_state: State) {}

/// Co ze sběru na tomhle stroji nefunguje — pro diagnostiku v UI.
pub fn degraded(state: &State) -> Vec<(String, String)> {
    state.degraded.list()
}

/// Image, které jsou jen běhovým prostředím jiné aplikace.
const GENERIC_HOSTS: &[&str] = &["msedgewebview2.exe"];

fn is_generic_host(name: &str) -> bool {
    GENERIC_HOSTS.iter().any(|h| name.eq_ignore_ascii_case(h))
}

/// Přepíše identitu hostitelských procesů na identitu předka, který
/// hostitel není.
///
/// Chodí se po rodičích, protože WebView2 si spouští vlastní potomky
/// (renderer, GPU proces) — jejich rodičem je zase `msedgewebview2.exe`.
/// Hloubka je omezená a navštívené PIDy se hlídají: kdyby PID recykloval
/// tak nešťastně, že vznikne cyklus, nesmí to zaseknout celý sampler.
/// Když se předek nenajde (skončil), zůstane hostiteli vlastní identita —
/// vymýšlet si vlastníka by bylo horší než ho přiznat jako neznámého.
fn reparent_hosts(rows: &mut [ProcRow]) {
    let hosts: Vec<u32> = rows
        .iter()
        .filter(|r| is_generic_host(&r.name))
        .map(|r| r.pid)
        .collect();
    if hosts.is_empty() {
        return;
    }
    let by_pid: HashMap<u32, usize> = rows.iter().enumerate().map(|(i, r)| (r.pid, i)).collect();

    let mut fixes: Vec<(usize, String, String, Option<String>)> = Vec::new();
    for pid in hosts {
        let Some(&idx) = by_pid.get(&pid) else { continue };
        let mut cur = rows[idx].parent_pid;
        // Čas vzniku článku, ze kterého se právě jde k rodiči.
        let mut child_ct = rows[idx].create_time;
        let mut seen = 0u8;
        let mut visited: Vec<u32> = vec![pid];
        while seen < 8 {
            seen += 1;
            if visited.contains(&cur) {
                break;
            }
            visited.push(cur);
            let Some(&pidx) = by_pid.get(&cur) else { break };
            // Rodič musí být starší než potomek. Windows PID recykluje
            // a `parent_pid` se po zániku rodiče neaktualizuje — číslo
            // pak ukazuje na proces, který vznikl až potom a s WebView2
            // nemá nic společného. Bez téhle kontroly by si hostitel
            // vypůjčil identitu náhodné aplikace.
            //
            // Porovnává se s POTOMKEM v řetězci, ne s původním
            // hostitelem: recyklovaný rodič mezilehlého WebView2, který
            // vznikl po něm, ale před původním hostitelem, jinak prošel
            // a renderer dostal identitu cizí hry.
            if rows[pidx].create_time > child_ct {
                break;
            }
            if is_generic_host(&rows[pidx].name) {
                child_ct = rows[pidx].create_time;
                cur = rows[pidx].parent_pid;
                continue;
            }
            let owner = &rows[pidx];
            fixes.push((
                idx,
                owner.identity_key.clone(),
                owner.app_name.clone(),
                owner.publisher.clone(),
            ));
            break;
        }
    }
    for (idx, key, app, publisher) in fixes {
        rows[idx].identity_key = key;
        rows[idx].app_name = app;
        rows[idx].publisher = publisher;
    }
}

#[cfg(test)]
mod reparent_tests {
    use super::*;

    fn row(pid: u32, parent: u32, ct: i64, name: &str, key: &str) -> ProcRow {
        ProcRow {
            pid,
            parent_pid: parent,
            create_time: ct,
            name: name.into(),
            identity_key: key.into(),
            app_name: key.into(),
            ..Default::default()
        }
    }

    // W1 (t=50) má rodiče PID 1234, který skončil a dostal ho pozdější
    // hra X (t=70). Renderer W2 (t=100) pod W1 nesmí přes W1 dojít
    // k X — X vznikl až po W1, takže jeho rodičem není.
    #[test]
    fn recycled_parent_of_intermediate_host_is_rejected() {
        let mut rows = vec![
            row(200, 1234, 50, "msedgewebview2.exe", "sig:microsoft"),
            row(1234, 4, 70, "game.exe", "app:game"),
            row(300, 200, 100, "msedgewebview2.exe", "sig:microsoft"),
        ];
        reparent_hosts(&mut rows);
        assert_eq!(rows[0].identity_key, "sig:microsoft");
        assert_eq!(rows[2].identity_key, "sig:microsoft");
    }

    // Poctivý řetězec hostitel → WebView2 → renderer dál projde.
    #[test]
    fn genuine_chain_takes_owner_identity() {
        let mut rows = vec![
            row(100, 4, 10, "app.exe", "app:app"),
            row(200, 100, 50, "msedgewebview2.exe", "sig:microsoft"),
            row(300, 200, 100, "msedgewebview2.exe", "sig:microsoft"),
        ];
        reparent_hosts(&mut rows);
        assert_eq!(rows[1].identity_key, "app:app");
        assert_eq!(rows[2].identity_key, "app:app");
    }
}
