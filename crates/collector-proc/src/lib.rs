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

/// Chyby sampleru.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
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
    prev_net: win_sys::net::NetTotals,
    prev_cores: Vec<win_sys::sysinfo::CoreTimes>,
    /// NVML kontext; None = GPU metrika nedostupná (bez NVIDIA).
    gpu: Option<win_sys::gpu::Nvml>,
    /// Otevřené fyzické disky + minulé čítače (pro delty).
    disks: Vec<win_sys::disk::Disk>,
    prev_disks: Vec<win_sys::disk::DiskCounters>,
    /// Per-proces GPU přes PDH (SPEC kap. 3.1); None = counter není.
    gpu_proc: Option<win_sys::gpuproc::GpuPerProc>,
    /// Statické info komponent — zjištěno jednou při init.
    statics: StaticInfo,
    /// Engine identity aplikací (v2, SPEC kap. 4) — cache + background.
    identity: identity::Engine,
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
    let n_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1) as f64;
    let gpu = win_sys::gpu::Nvml::init();
    if gpu.is_none() {
        tracing::info!("NVML nedostupné — GPU metrika bude hlášena jako nedostupná");
    }

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
            })
            .collect(),
        ram_slots,
        gpu_name: gpu.as_ref().and_then(|g| g.name()),
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
        prev_net: win_sys::net::net_totals()?,
        prev_cores: win_sys::sysinfo::core_times(n_cpus as usize)?,
        gpu,
        disks,
        prev_disks,
        gpu_proc: win_sys::gpuproc::GpuPerProc::init(),
        statics,
        identity: identity::Engine::new(identity::load_tables()),
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

    // Per-proces GPU % (PDH GPU Engine, součet přes enginy daného PID).
    let gpu_by_pid = state
        .gpu_proc
        .as_mut()
        .map(|g| g.sample())
        .unwrap_or_default();

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
        let (id, prot) = state.identity.identify(p.pid, &p.name);
        rows.push(ProcRow {
            pid: p.pid,
            parent_pid: p.parent_pid,
            name: p.name.clone(),
            cpu_pct: cpu_pct.clamp(0.0, 100.0),
            ws_bytes: p.ws_bytes,
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
    state.prev_cpu = next_cpu;

    // Úklid identity cache od zaniklých procesů (levné, jednou za tick).
    let live: std::collections::HashSet<u32> = raw.iter().map(|p| p.pid).collect();
    state.identity.retain_pids(&live);

    // Systém: busy = (kernel - idle) + user z delty GetSystemTimes.
    let sys = win_sys::sysinfo::system_times()?;
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

    let (mem_used_mb, mem_total_mb) = win_sys::sysinfo::memory_status_mb()?;

    // Síť: delta kumulativních bajtů / delta stěny → B/s.
    let net = win_sys::net::net_totals()?;
    let net_rx_bps = (net.rx_bytes.saturating_sub(state.prev_net.rx_bytes) as f64 / wall_s) as u64;
    let net_tx_bps = (net.tx_bytes.saturating_sub(state.prev_net.tx_bytes) as f64 / wall_s) as u64;
    state.prev_net = net;

    // Per-core zátěž: busy = 1 − idle_d / (kernel_d + user_d).
    let cores_now = win_sys::sysinfo::core_times(state.n_cpus as usize)?;
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

    // Disky: delty kumulativních bajtů → B/s per disk.
    let mut disk_rates = Vec::with_capacity(state.disks.len());
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
                state.prev_disks[i] = now;
            }
            Err(e) => tracing::debug!(disk = d.index, error = %e, "čtení čítačů disku selhalo"),
        }
    }

    // Takty CPU (stupeň 3 kaskády, SPEC 15.2) + uptime + součty.
    let (cpu_clock_mhz, cpu_clock_max_mhz) =
        win_sys::sysinfo::cpu_clocks(state.n_cpus as usize).unwrap_or((0, 0));
    let threads_total: u32 = raw.iter().map(|p| p.threads).sum();
    let handles_total: u32 = raw.iter().map(|p| p.handles).sum();

    let gpu_detail = state.gpu.as_ref().map(|g| {
        let d = g.details();
        core_types::proc::GpuInfo {
            temp_c: d.temp_c,
            vram_used_mb: d.vram_used_mb,
            vram_total_mb: d.vram_total_mb,
            power_w: d.power_w,
            clock_mhz: d.clock_mhz,
        }
    });

    let snapshot = SystemSnapshot {
        cpu_pct: cpu_pct.clamp(0.0, 100.0),
        mem_used_mb,
        mem_total_mb,
        proc_count: rows.len() as u32,
        net_rx_bps,
        net_tx_bps,
        gpu_pct: state.gpu.as_ref().and_then(|g| g.utilization_pct()),
        cores,
        gpu: gpu_detail,
        disks: disk_rates,
        cpu_clock_mhz,
        cpu_clock_max_mhz,
        uptime_s: win_sys::sysinfo::system_uptime_s(),
        threads_total,
        handles_total,
    };
    Ok((rows, snapshot))
}

/// Korektní ukončení sampleru.
pub fn shutdown(_state: State) {}
