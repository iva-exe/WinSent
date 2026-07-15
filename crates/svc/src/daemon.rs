//! Jádro démona — společné pro oba režimy (--service i --console).
//!
//! v0 skládá: kontrolu integrity, config s hot-reloadem, SQLite store
//! s retenční smyčkou, IPC server a heartbeat „žiju“ 1×/s.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use core_types::config::Config;
use core_types::ipc::{Request, Response, PROTOCOL_VERSION};
use core_types::proc::{ProcRow, SystemSnapshot};

/// Chyby startu démona. Za běhu se chyby logují a jednotlivé části se
/// restartují/degradují, ale start musí být buď celý, nebo žádný.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("store: {0}")]
    Store(#[from] store::Error),
    #[error("config: {0}")]
    Config(#[from] crate::config::Error),
    #[error("nelze spustit vlákno: {0}")]
    Thread(#[from] std::io::Error),
    #[error("ipc: {0}")]
    Ipc(#[from] ipc::Error),
    #[error("collector-proc: {0}")]
    Collector(#[from] collector_proc::Error),
}

/// Poslední vzorek sampleru sdílený s IPC handlerem.
#[derive(Default)]
struct LiveSample {
    procs: Vec<ProcRow>,
    system: SystemSnapshot,
}

/// Spustí démona a blokuje, dokud `stop` nenastaví okolí (SCM handler
/// nebo Ctrl+C). Vrací se až po korektním úklidu.
pub fn run(stop: Arc<AtomicBool>) -> Result<(), Error> {
    let started = Instant::now();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "syswatch démon startuje"
    );

    // Kontrola integrity vlastních binárek (SPEC kap. 2.3). Během
    // vývoje nepodepsané → jen varování; fatální bude s code signingem (v11).
    crate::integrity::report_own_binaries();

    // Konfigurace: načíst (případně založit default soubor) + hot-reload.
    let cfg_path = store::data_dir()?.join("config.toml");
    let cfg: Arc<RwLock<Config>> = Arc::new(RwLock::new(crate::config::load_or_create(&cfg_path)?));
    // Watcher musí zůstat živý po celou dobu běhu — drop by reload vypnul.
    let _cfg_watcher = crate::config::watch(&cfg_path, Arc::clone(&cfg))?;

    // SQLite store: otevření + migrace, pak retenční smyčka ve vlastním
    // vlákně na BELOW_NORMAL prioritě (SPEC kap. 8).
    let db_path = store::db_path()?;
    let conn = store::open(&db_path)?;
    tracing::info!(db = %db_path.display(), "databáze otevřena, schéma zmigrováno");

    let retention_handle = {
        let stop = Arc::clone(&stop);
        let cfg = Arc::clone(&cfg);
        std::thread::Builder::new()
            .name("retention".into())
            .spawn(move || retention_loop(conn, cfg, stop))?
    };

    // Sampler procesů (v1, SPEC kap. 3.1): 1 Hz vlákno plní sdílený
    // poslední vzorek, ze kterého IPC handler odpovídá bez čekání.
    let live: Arc<RwLock<LiveSample>> = Arc::new(RwLock::new(LiveSample::default()));
    let sampler_handle = {
        let stop = Arc::clone(&stop);
        let live = Arc::clone(&live);
        let mut state = collector_proc::init(&cfg.read().expect("config lock poisoned"))?;
        std::thread::Builder::new()
            .name("sampler".into())
            .spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match collector_proc::tick(&mut state) {
                        Ok((procs, system)) => {
                            let mut slot = live.write().expect("live lock poisoned");
                            slot.procs = procs;
                            slot.system = system;
                        }
                        // Kolektor nesmí shodit službu — chyba se loguje
                        // a další tick to zkusí znovu (SPEC kap. 22).
                        Err(e) => tracing::error!(error = %e, "tick sampleru selhal"),
                    }
                    wait_or_stop(&stop, Duration::from_millis(1000));
                }
                collector_proc::shutdown(state);
            })?
    };

    // IPC server: navázání na pipe je synchronní — kolize s jinou
    // instancí démona (běžící služba vs. --console) shodí start hned,
    // s jasnou chybou. Akceptační smyčka pak běží ve vlastním vlákně.
    let ipc_bound = ipc::server::bind()?;
    let ipc_handle = {
        let stop = Arc::clone(&stop);
        let live = Arc::clone(&live);
        let handler: ipc::server::Handler = Arc::new(move |req| match req {
            Request::Ping { protocol_version } => {
                if protocol_version != PROTOCOL_VERSION {
                    tracing::warn!(
                        client = protocol_version,
                        server = PROTOCOL_VERSION,
                        "klient s jinou verzí protokolu"
                    );
                }
                Response::Pong {
                    protocol_version: PROTOCOL_VERSION,
                    uptime_s: started.elapsed().as_secs(),
                }
            }
            Request::QueryProcs => {
                Response::Procs(live.read().expect("live lock poisoned").procs.clone())
            }
            Request::QuerySystem => {
                Response::System(live.read().expect("live lock poisoned").system)
            }
        });
        std::thread::Builder::new()
            .name("ipc-server".into())
            .spawn(move || {
                if let Err(e) = ipc::server::run(ipc_bound, handler, stop) {
                    tracing::error!(error = %e, "IPC server spadl");
                }
            })?
    };

    // Heartbeat — v0 jediná „práce“ démona. Interval čte z configu
    // při každém kole, takže hot-reload se projeví hned.
    while !stop.load(Ordering::SeqCst) {
        let interval = {
            let cfg = cfg.read().expect("config lock poisoned");
            Duration::from_millis(cfg.heartbeat_ms.max(100))
        };
        tracing::info!(uptime_s = started.elapsed().as_secs(), "žiju");
        wait_or_stop(&stop, interval);
    }

    // Úklid: probudit IPC server z blokujícího čekání a počkat na vlákna.
    tracing::info!("zastavuji démona");
    ipc::server::wake();
    let _ = ipc_handle.join();
    let _ = sampler_handle.join();
    let _ = retention_handle.join();
    tracing::info!("démon ukončen čistě");
    Ok(())
}

/// Retenční smyčka (v0 naprázdno). Vlastní vlákno, BELOW_NORMAL.
fn retention_loop(conn: store::Connection, cfg: Arc<RwLock<Config>>, stop: Arc<AtomicBool>) {
    if let Err(e) = win_sys::threading::set_current_thread_below_normal() {
        tracing::warn!(error = %e, "nepodařilo se snížit prioritu retenčního vlákna");
    }
    while !stop.load(Ordering::SeqCst) {
        if let Err(e) = store::retention::tick(&conn) {
            // Chyba retence nesmí shodit službu — zaloguje se a jede dál.
            tracing::error!(error = %e, "retenční krok selhal");
        }
        let interval = {
            let cfg = cfg.read().expect("config lock poisoned");
            store::retention_interval(&cfg)
        };
        wait_or_stop(&stop, interval);
    }
}

/// Spí po daný interval, ale reaguje na stop do ~50 ms.
fn wait_or_stop(stop: &AtomicBool, total: Duration) {
    const SLICE: Duration = Duration::from_millis(50);
    let deadline = Instant::now() + total;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(SLICE.min(deadline.saturating_duration_since(Instant::now())));
    }
}
