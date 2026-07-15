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

    // SQLite store: otevření + migrace. Zápis vzorků i retence běží
    // v jediném zapisovacím vlákně (SQLite má jednoho zapisovatele)
    // na BELOW_NORMAL prioritě (SPEC kap. 3.4, 8).
    let db_path = store::db_path()?;
    let conn = store::open(&db_path)?;
    tracing::info!(db = %db_path.display(), "databáze otevřena, schéma zmigrováno");

    // Kanál sampler → zapisovací vlákno. Bounded: když zápis nestíhá
    // (disk saturovaný), vzorky se zahazují — sampler NIKDY nesmí
    // blokovat na I/O (SPEC kap. 3.4).
    let (sample_tx, sample_rx) =
        std::sync::mpsc::sync_channel::<(i64, Vec<ProcRow>, SystemSnapshot)>(4);

    let store_handle = {
        let stop = Arc::clone(&stop);
        let cfg = Arc::clone(&cfg);
        std::thread::Builder::new()
            .name("store-writer".into())
            .spawn(move || store_loop(conn, cfg, stop, sample_rx))?
    };

    // Sampler procesů (v1, SPEC kap. 3.1): 1 Hz vlákno plní sdílený
    // poslední vzorek (pro IPC) a posílá tick zapisovacímu vláknu.
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
                            let ts = unix_now();
                            // Plný kanál = zápis nestíhá; vzorek se zahodí
                            // a zaloguje, sampler neblokuje.
                            if let Err(std::sync::mpsc::TrySendError::Full(_)) =
                                sample_tx.try_send((ts, procs.clone(), system.clone()))
                            {
                                tracing::warn!("zapisovací vlákno nestíhá — vzorek zahozen");
                            }
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
                // Drop sample_tx → zapisovací vlákno pozná konec.
            })?
    };

    // IPC server: navázání na pipe je synchronní — kolize s jinou
    // instancí démona (běžící služba vs. --console) shodí start hned,
    // s jasnou chybou. Akceptační smyčka pak běží ve vlastním vlákně.
    let ipc_bound = ipc::server::bind()?;
    let ipc_handle = {
        let stop = Arc::clone(&stop);
        let live = Arc::clone(&live);
        let self_db_path = db_path.clone();
        // Read-only spojení pro dotazy historie — čtenář ve WAL režimu
        // neblokuje zapisovací vlákno. Mutex: handler běží ve více
        // obslužných vláknech pipe.
        let read_conn = std::sync::Mutex::new(store::open_readonly(&db_path)?);
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
                Response::System(live.read().expect("live lock poisoned").system.clone())
            }
            // Sebemonitoring (SPEC kap. 2.3): vlastní řádek ze stejného
            // sampleru jako všechny ostatní procesy — žádný speciální kód.
            Request::QuerySelfUsage => {
                let own_pid = std::process::id();
                let live = live.read().expect("live lock poisoned");
                let own = live.procs.iter().find(|p| p.pid == own_pid);
                let db_bytes = db_size_bytes(&self_db_path);
                match own {
                    Some(p) => Response::SelfUsage {
                        cpu_pct: p.cpu_pct,
                        ws_bytes: p.ws_bytes,
                        db_bytes,
                    },
                    None => Response::Error {
                        message: "vlastní proces zatím není ve vzorku".into(),
                    },
                }
            }
            // Historie z SQLite (graf do minulosti, stav tasků v čase).
            Request::QuerySystemHistory { from, to } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::history::system_history(&conn, from, to) {
                    Ok(points) => Response::SystemHistory(points),
                    Err(e) => Response::Error {
                        message: format!("čtení historie selhalo: {e}"),
                    },
                }
            }
            Request::QueryProcsAt { ts } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::history::procs_at(&conn, ts) {
                    Ok(Some((actual, rows))) => Response::ProcsAt { ts: actual, rows },
                    Ok(None) => Response::Error {
                        message: "pro tento čas není vzorek v historii".into(),
                    },
                    Err(e) => Response::Error {
                        message: format!("čtení historie selhalo: {e}"),
                    },
                }
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
    let _ = store_handle.join();
    tracing::info!("démon ukončen čistě");
    Ok(())
}

/// Zapisovací vlákno store: přijímá ticky sampleru, dávkově zapisuje
/// do SQLite a v intervalu pouští retenci. Jediný zapisovatel DB.
fn store_loop(
    mut conn: store::Connection,
    cfg: Arc<RwLock<Config>>,
    stop: Arc<AtomicBool>,
    rx: std::sync::mpsc::Receiver<(i64, Vec<ProcRow>, SystemSnapshot)>,
) {
    use std::sync::mpsc::RecvTimeoutError;

    if let Err(e) = win_sys::threading::set_current_thread_below_normal() {
        tracing::warn!(error = %e, "nepodařilo se snížit prioritu zapisovacího vlákna");
    }

    let mut last_retention = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok((ts, procs, sys)) => {
                if let Err(e) = store::samples::insert_tick(&mut conn, ts, &sys, &procs) {
                    // Chyba zápisu nesmí shodit službu (SPEC kap. 22).
                    tracing::error!(error = %e, "zápis vzorku selhal");
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // Sampler skončil a kanál je prázdný → konec.
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let interval = {
            let cfg = cfg.read().expect("config lock poisoned");
            store::retention_interval(&cfg)
        };
        if last_retention.elapsed() >= interval {
            last_retention = Instant::now();
            if let Err(e) = store::retention::tick(&conn) {
                tracing::error!(error = %e, "retenční krok selhal");
            }
        }

        if stop.load(Ordering::SeqCst) {
            // Doprázdnit kanál, ať poslední vzorky nezmizí, pak konec.
            while let Ok((ts, procs, sys)) = rx.try_recv() {
                if let Err(e) = store::samples::insert_tick(&mut conn, ts, &sys, &procs) {
                    tracing::error!(error = %e, "zápis vzorku při ukončení selhal");
                }
            }
            break;
        }
    }
}

/// Velikost databáze vč. WAL (pro dlaždici spotřeby).
fn db_size_bytes(db_path: &std::path::Path) -> u64 {
    let main = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    let wal = std::fs::metadata(db_path.with_extension("db-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    main + wal
}

/// Unix čas v sekundách.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
