//! collector-etw — ETW session (SPEC kap. 3.2): realtime události
//! procesů (start/stop s exit kódem a pravým parent PID) + POVINNÝ
//! autologger — černá skříňka `.etl` (rotující ring 64 MB, zapisuje
//! jádro, přežije BSOD).
//!
//! Kernel-File až ve v4/v8 (mapa souborů); hard faulty a latence disku
//! se sbírají levněji mimo ETW (PDH/IOCTL, viz win-sys::pdhq a disk).

use core_types::config::Config;

pub use win_sys::etw::ProcEvent;

/// Jméno realtime session.
const RT_SESSION: &str = "syswatch-rt";
/// Jméno autologger session (černá skříňka).
pub const BB_SESSION: &str = "syswatch-blackbox";

/// Vynutí zápis rozepsaných bufferů černé skříňky na disk.
///
/// Buffery se jinak zapisují až plné (šetří to desítky gigabajtů
/// zápisů denně), takže posledních pár minut leží v paměti. Před
/// archivací okna incidentu se musí dostat na disk — jinak by
/// v archivu chybělo právě to, co se dělo těsně předtím.
pub fn flush_blackbox() {
    if let Err(e) = win_sys::etw::flush_session(BB_SESSION) {
        tracing::warn!(error = %e, "vyprázdnění černé skříňky selhalo");
    }
}

/// Chyby této crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
}

/// Stav kolektoru: běžící sessions + kanál událostí. Session pole se
/// drží kvůli Drop (zastavení sessions při shutdownu).
pub struct State {
    _rt: win_sys::etw::Session,
    _bb: Option<win_sys::etw::Session>,
    _consumer: win_sys::etw::Consumer,
    rx: std::sync::mpsc::Receiver<ProcEvent>,
    /// Cesta k .etl černé skříňky (pro incidenty).
    pub etl_path: Option<String>,
}

/// Inicializace: spustí realtime session + konzumenta + černou skříňku.
/// Selhání černé skříňky není fatální (loguje se) — realtime události
/// jsou pro v3 podstatnější; bez admin práv selže už realtime session.
pub fn init(_cfg: &Config) -> Result<State, Error> {
    let rt = win_sys::etw::start_realtime(RT_SESSION)?;
    let (rx, consumer) = win_sys::etw::consume(RT_SESSION)?;

    // Černá skříňka do datového adresáře služby.
    let etl_path = std::env::var_os("ProgramData")
        .map(|p| format!("{}\\syswatch\\blackbox.etl", p.to_string_lossy()));
    let bb =
        etl_path.as_deref().and_then(
            |path| match win_sys::etw::start_blackbox(BB_SESSION, path) {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!(error = %e, "autologger (černá skříňka) se nespustil");
                    None
                }
            },
        );
    let etl_path = bb.is_some().then_some(etl_path).flatten();

    tracing::info!(blackbox = etl_path.is_some(), "ETW session běží");
    Ok(State {
        _rt: rt,
        _bb: bb,
        _consumer: consumer,
        rx,
        etl_path,
    })
}

/// Vybere události nahromaděné od minulého ticku. Nikdy neblokuje.
pub fn drain(state: &mut State) -> Vec<ProcEvent> {
    let mut out = Vec::new();
    while let Ok(ev) = state.rx.try_recv() {
        out.push(ev);
    }
    out
}

/// Odebere síťové bajty per PID nasčítané od minulého volání
/// (v9, SPEC 12.1). Volá se 1×/s — delta je rovnou B/s.
pub fn take_net(state: &State) -> win_sys::etw::NetTotalsByPid {
    state._consumer.take_net()
}

/// Korektní ukončení (Drop zastaví sessions).
pub fn shutdown(_state: State) {}
