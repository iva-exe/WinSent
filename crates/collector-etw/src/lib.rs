//! collector-etw — ETW session (SPEC kap. 3.2): realtime události
//! procesů (start/stop s exit kódem a pravým parent PID) + černá
//! skříňka `.etl` (rotující ring 64 MB).
//!
//! Skříňka NENÍ registrovaná jako autologger v registru: zakládá se
//! až po startu služby, takže restart stroje nepřežije a po BSODu
//! v ní chybí posledních nejvýš pět sekund (co leželo v bufferech).
//! SPEC 16.3 v tomhle bodě popisuje cíl, ne stav — viz win_sys::etw.
//!
//! Kernel-File až ve v4/v8 (mapa souborů); hard faulty a latence disku
//! se sbírají levněji mimo ETW (PDH/IOCTL, viz win-sys::pdhq a disk).

use core_types::config::Config;

pub use win_sys::etw::ProcEvent;

/// Jméno realtime session.
const RT_SESSION: &str = "syswatch-rt";
/// Jméno session černé skříňky.
pub const BB_SESSION: &str = "syswatch-blackbox";

/// Vynutí zápis rozepsaných bufferů černé skříňky na disk.
///
/// Volá se před archivací okna incidentu. Když session neběží, nemá
/// se co zapisovat a není to chyba — po restartu stroje se archivuje
/// dřív, než se skříňka vůbec založí (soubor by jinak `StartTraceW`
/// přepsal). Proto jen `debug`, ne varování: hlásit „selhalo" po
/// každém BSODu znamená poslat člověka hledat problém, který není.
pub fn flush_blackbox() {
    if let Err(e) = win_sys::etw::flush_session(BB_SESSION) {
        tracing::debug!(error = %e, "černá skříňka se nevyprázdnila (nejspíš ještě neběží)");
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
