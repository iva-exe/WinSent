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
use core_types::proc::{ProcRow, SystemSnapshot};

/// Chyby sampleru.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
}

/// Stav sampleru mezi ticky.
pub struct State {
    /// Znovupoužívaný buffer pro NtQuerySystemInformation.
    buf: Vec<u8>,
    /// Kumulativní CPU čas z minulého ticku: (pid, create_time) → 100ns.
    prev_cpu: HashMap<(u32, i64), u64>,
    prev_tick: Instant,
    prev_sys: win_sys::sysinfo::SystemTimes,
    prev_net: win_sys::net::NetTotals,
    /// Počet logických jader — normalizace na % celkové kapacity.
    n_cpus: f64,
}

/// Inicializace sampleru.
pub fn init(_cfg: &Config) -> Result<State, Error> {
    let n_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1) as f64;
    Ok(State {
        buf: Vec::new(),
        prev_cpu: HashMap::new(),
        prev_tick: Instant::now(),
        prev_sys: win_sys::sysinfo::system_times()?,
        prev_net: win_sys::net::net_totals()?,
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

    // CPU % per proces z delty; nový proces (nebo recyklovaný PID) začíná
    // od nuly — bez minulého vzorku deltu nemá.
    let mut next_cpu = HashMap::with_capacity(raw.len());
    let mut rows = Vec::with_capacity(raw.len());
    for p in &raw {
        let key = (p.pid, p.create_time);
        let cpu_pct = match state.prev_cpu.get(&key) {
            Some(prev) if p.cpu_time_100ns >= *prev => {
                ((p.cpu_time_100ns - prev) as f64 / (wall_100ns * state.n_cpus) * 100.0) as f32
            }
            _ => 0.0,
        };
        next_cpu.insert(key, p.cpu_time_100ns);

        // pid 0 (Idle) do tabulky nepatří — je to účetnictví nečinnosti,
        // ne proces; systémové CPU % ho už zohledňuje.
        if p.pid == 0 {
            continue;
        }
        rows.push(ProcRow {
            pid: p.pid,
            parent_pid: p.parent_pid,
            name: p.name.clone(),
            cpu_pct: cpu_pct.clamp(0.0, 100.0),
            ws_bytes: p.ws_bytes,
            priv_bytes: p.priv_bytes,
            threads: p.threads,
            session_id: p.session_id,
        });
    }
    state.prev_cpu = next_cpu;

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
    let wall_s = wall_100ns / 1e7;
    let net_rx_bps = (net.rx_bytes.saturating_sub(state.prev_net.rx_bytes) as f64 / wall_s) as u64;
    let net_tx_bps = (net.tx_bytes.saturating_sub(state.prev_net.tx_bytes) as f64 / wall_s) as u64;
    state.prev_net = net;

    let snapshot = SystemSnapshot {
        cpu_pct: cpu_pct.clamp(0.0, 100.0),
        mem_used_mb,
        mem_total_mb,
        proc_count: rows.len() as u32,
        net_rx_bps,
        net_tx_bps,
    };
    Ok((rows, snapshot))
}

/// Korektní ukončení sampleru.
pub fn shutdown(_state: State) {}
