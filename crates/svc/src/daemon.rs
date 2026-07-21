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

    // Sampler se inicializuje před store vláknem — statické info
    // komponent (SPEC 15.1) vzniká jednou a názvy disků se zapíšou
    // hlavním spojením, dokud ho máme.
    let sampler_state = collector_proc::init(&cfg.read().expect("config lock poisoned"))?;
    let statics = collector_proc::static_info(&sampler_state);
    // Sdílená cache ikon aplikací — plní identity worker, čte IPC handler.
    let icon_store = collector_proc::icon_store(&sampler_state);
    if let Err(e) = store::samples::upsert_disk_names(&conn, &statics.disks) {
        tracing::warn!(error = %e, "zápis názvů disků selhal");
    }

    // Nečisté vypnutí + BSOD sken (SPEC 16.2) — ještě hlavním spojením,
    // před startem zapisovacího vlákna.
    startup_crash_scan(&conn);

    // ETW (v3, SPEC 3.2): realtime události procesů + černá skříňka.
    // Selhání degraduje (bez pádů procesů), službu neshazuje.
    let mut etw = match collector_etw::init(&cfg.read().expect("config lock poisoned")) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(error = %e, "ETW nedostupné — pády procesů se nezaznamenají");
            None
        }
    };
    let etl_path = etw.as_ref().and_then(|s| s.etl_path.clone());

    // Heartbeat detekce záseku (SPEC 3.3) — TIME_CRITICAL vlákno.
    let mut stall = match collector_proc::stall::Detector::start() {
        Ok(d) => Some(d),
        Err(e) => {
            tracing::warn!(error = %e, "detektor záseků se nespustil");
            None
        }
    };

    // Kanál sampler → zapisovací vlákno. Bounded: když zápis nestíhá
    // (disk saturovaný), vzorky se zahazují — sampler NIKDY nesmí
    // blokovat na I/O (SPEC kap. 3.4). Události jdou týmž kanálem —
    // DB má jediného zapisovatele.
    let (sample_tx, sample_rx) = std::sync::mpsc::sync_channel::<crate::incidents::StoreMsg>(16);

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
        let mut state = sampler_state;
        std::thread::Builder::new()
            .name("sampler".into())
            .spawn(move || {
                use crate::incidents::{classify_stall, is_crash_exit, json_str, StoreMsg};
                // Jména/identity naposledy viděných PIDů — proces, který
                // umřel, už v aktuálním vzorku není a jeho stop event
                // navíc dorazí z ETW bufferů až o pár sekund později.
                // Záznamy se proto drží 15 s po posledním spatření.
                let mut seen: std::collections::HashMap<u32, (String, String, String, i64)> =
                    std::collections::HashMap::new();
                let mut last_stall_ts: i64 = 0;
                let mut last_sent_ts: i64 = 0;
                // Rate-limit incidentů: jedna aplikace max 1 incident
                // pádu za 2 minuty (opakované pády = tentýž problém).
                let mut last_crash: std::collections::HashMap<String, i64> =
                    std::collections::HashMap::new();
                // Po záseku 10 s burst na 10 Hz (SPEC 3.3) — jemnější
                // živý obraz okna; do DB jde pořád max 1 vzorek/s.
                let mut burst_until = Instant::now();

                while !stop.load(Ordering::SeqCst) {
                    match collector_proc::tick(&mut state) {
                        Ok((procs, system)) => {
                            let ts = unix_now();
                            // Pády procesů z ETW exit kódů (SPEC 16.1).
                            if let Some(etw_state) = etw.as_mut() {
                                for ev in collector_etw::drain(etw_state) {
                                    if let collector_etw::ProcEvent::Stop { ts, pid, exit_code } =
                                        ev
                                    {
                                        if is_crash_exit(exit_code) {
                                            let (name, app, key, _) =
                                                seen.get(&pid).cloned().unwrap_or_default();
                                            let detail = format!(
                                                "{{\"exit_code\":{exit_code},\"name\":\"{}\",\"app\":\"{}\"}}",
                                                json_str(&name),
                                                json_str(&app)
                                            );
                                            let _ = sample_tx.try_send(StoreMsg::Event {
                                                ts,
                                                kind: "proc_crash",
                                                pid: Some(pid),
                                                detail: detail.clone(),
                                            });
                                            // Incident jen pro proces, který sampler
                                            // znal jménem (žil ≥ 1 tick) — filtruje
                                            // sub-sekundové workery; a max 1×/2 min
                                            // na aplikaci.
                                            let rate_key =
                                                if key.is_empty() { name.clone() } else { key.clone() };
                                            let recently = last_crash
                                                .get(&rate_key)
                                                .is_some_and(|&t| ts - t < 120);
                                            if !name.is_empty() && !recently {
                                                last_crash.insert(rate_key, ts);
                                                let _ = sample_tx.try_send(StoreMsg::Incident {
                                                    ts,
                                                    kind: "app_crash",
                                                    identity_key: (!key.is_empty())
                                                        .then_some(key),
                                                    culprit: Some(if app.is_empty() {
                                                        name
                                                    } else {
                                                        app
                                                    }),
                                                    detail,
                                                    etl_path: etl_path.clone(),
                                                    window_from: ts - 300,
                                                    window_to: ts + 30,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                            // Záseky: heartbeat hity → klasifikace z metrik
                            // aktuálního vzorku (SPEC 3.3).
                            if let Some(det) = stall.as_mut() {
                                for hit in det.drain() {
                                    if hit.ts - last_stall_ts < 10 {
                                        continue; // pokračování téhož záseku
                                    }
                                    last_stall_ts = hit.ts;
                                    burst_until = Instant::now() + Duration::from_secs(10);
                                    let v = classify_stall(&system, &procs);
                                    let top: Vec<String> = v
                                        .top
                                        .iter()
                                        .map(|(pid, name, val)| {
                                            format!(
                                                "{{\"pid\":{pid},\"name\":\"{}\",\"value\":{val:.0}}}",
                                                json_str(name)
                                            )
                                        })
                                        .collect();
                                    let detail = format!(
                                        "{{\"lag_ms\":{},\"cause\":\"{}\",\"top\":[{}]}}",
                                        hit.lag_ms,
                                        v.cause,
                                        top.join(",")
                                    );
                                    tracing::warn!(
                                        lag_ms = hit.lag_ms,
                                        cause = v.cause,
                                        "detekován zásek systému"
                                    );
                                    let _ = sample_tx.try_send(StoreMsg::Event {
                                        ts: hit.ts,
                                        kind: "stall",
                                        pid: v.culprit.as_ref().map(|c| c.0),
                                        detail: detail.clone(),
                                    });
                                    let _ = sample_tx.try_send(StoreMsg::Incident {
                                        ts: hit.ts,
                                        kind: "stall",
                                        identity_key: v
                                            .culprit
                                            .as_ref()
                                            .map(|c| c.2.clone())
                                            .filter(|k| !k.is_empty()),
                                        culprit: v.culprit.as_ref().map(|c| c.1.clone()),
                                        detail,
                                        etl_path: etl_path.clone(),
                                        window_from: hit.ts - 10 - (hit.lag_ms / 1000) as i64,
                                        window_to: hit.ts + 10,
                                    });
                                }
                            }
                            // Aktualizace mapy viděných PIDů až PO obsluze
                            // stop událostí (umřelé procesy potřebují stará
                            // jména); staré záznamy vypadnou po 15 s.
                            for p in &procs {
                                seen.insert(
                                    p.pid,
                                    (
                                        p.name.clone(),
                                        p.app_name.clone(),
                                        p.identity_key.clone(),
                                        ts,
                                    ),
                                );
                            }
                            seen.retain(|_, (_, _, _, last)| ts - *last < 15);
                            // Do DB max 1 vzorek/s i během burstu.
                            if ts != last_sent_ts {
                                last_sent_ts = ts;
                                // Plný kanál = zápis nestíhá; vzorek se
                                // zahodí a zaloguje, sampler neblokuje.
                                if let Err(std::sync::mpsc::TrySendError::Full(_)) = sample_tx
                                    .try_send(StoreMsg::Tick(ts, procs.clone(), system.clone()))
                                {
                                    tracing::warn!("zapisovací vlákno nestíhá — vzorek zahozen");
                                }
                            }
                            let mut slot = live.write().expect("live lock poisoned");
                            slot.procs = procs;
                            slot.system = system;
                        }
                        // Kolektor nesmí shodit službu — chyba se loguje
                        // a další tick to zkusí znovu (SPEC kap. 22).
                        Err(e) => tracing::error!(error = %e, "tick sampleru selhal"),
                    }
                    let interval = if Instant::now() < burst_until {
                        Duration::from_millis(100)
                    } else {
                        Duration::from_millis(1000)
                    };
                    wait_or_stop(&stop, interval);
                }
                if let Some(s) = etw.take() {
                    collector_etw::shutdown(s);
                }
                drop(stall.take());
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
        let statics = statics.clone();
        let icons = icon_store;
        let handler: ipc::server::Handler = Arc::new(move |req| match req {
            Request::QuerySysInfo => Response::SysInfo(statics.clone()),
            Request::QueryIcon { identity_key } => {
                // Ikona z cache identity workeru; když ještě není hotová,
                // vrátí None a UI si zkusí znovu (worker ji doplní).
                let ico = icons
                    .lock()
                    .expect("icon cache lock")
                    .get(&identity_key)
                    .cloned()
                    .flatten();
                Response::Icon(ico)
            }
            Request::QueryDetailAt { ts } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::history::detail_at(&conn, ts) {
                    Ok(Some((ts, cores, disks, gpu))) => Response::DetailAt {
                        ts,
                        cores,
                        disks,
                        gpu,
                    },
                    Ok(None) => Response::Error {
                        message: "pro tento čas není vzorek v historii".into(),
                    },
                    Err(e) => Response::Error {
                        message: format!("čtení historie selhalo: {e}"),
                    },
                }
            }
            Request::QueryDiskHistory { from, to } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::history::disk_history(&conn, from, to) {
                    Ok(points) => Response::DiskHistory(points),
                    Err(e) => Response::Error {
                        message: format!("čtení historie selhalo: {e}"),
                    },
                }
            }
            Request::QueryCoreHistory { from, to } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::history::core_history(&conn, from, to) {
                    Ok(points) => Response::CoreHistory(points),
                    Err(e) => Response::Error {
                        message: format!("čtení historie selhalo: {e}"),
                    },
                }
            }
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
            // Události a incidenty (v3, SPEC kap. 16).
            Request::QueryEvents { from, to } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::events::events_in(&conn, from, to) {
                    Ok(rows) => Response::Events(
                        rows.into_iter()
                            .map(|e| core_types::proc::EventRow {
                                id: e.id,
                                ts: e.ts,
                                kind: e.kind,
                                pid: e.pid,
                                detail: e.detail,
                            })
                            .collect(),
                    ),
                    Err(e) => Response::Error {
                        message: format!("čtení událostí selhalo: {e}"),
                    },
                }
            }
            Request::QueryIncidents { limit } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::events::recent_incidents(&conn, limit.min(500)) {
                    Ok(rows) => Response::Incidents(
                        rows.into_iter()
                            .map(|i| core_types::proc::IncidentRow {
                                id: i.id,
                                ts: i.ts,
                                kind: i.kind,
                                identity_key: i.identity_key,
                                culprit: i.culprit,
                                detail: i.detail,
                                window_from: i.window_from,
                                window_to: i.window_to,
                            })
                            .collect(),
                    ),
                    Err(e) => Response::Error {
                        message: format!("čtení incidentů selhalo: {e}"),
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
    rx: std::sync::mpsc::Receiver<crate::incidents::StoreMsg>,
) {
    use std::sync::mpsc::RecvTimeoutError;

    if let Err(e) = win_sys::threading::set_current_thread_below_normal() {
        tracing::warn!(error = %e, "nepodařilo se snížit prioritu zapisovacího vlákna");
    }

    let mut last_retention = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(msg) => {
                if let Err(e) = store_msg(&mut conn, msg) {
                    // Chyba zápisu nesmí shodit službu (SPEC kap. 22).
                    tracing::error!(error = %e, "zápis do store selhal");
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
            while let Ok(msg) = rx.try_recv() {
                if let Err(e) = store_msg(&mut conn, msg) {
                    tracing::error!(error = %e, "zápis při ukončení selhal");
                }
            }
            break;
        }
    }
    // Čisté ukončení — příští start pozná, že minule nešlo o pád.
    if let Err(e) = store::meta_set(&conn, "clean_shutdown", "1") {
        tracing::warn!(error = %e, "zápis clean_shutdown selhal");
    }
}

/// Zapíše jednu zprávu do store.
fn store_msg(
    conn: &mut store::Connection,
    msg: crate::incidents::StoreMsg,
) -> Result<(), store::SqlError> {
    use crate::incidents::StoreMsg;
    match msg {
        StoreMsg::Tick(ts, procs, sys) => store::samples::insert_tick(conn, ts, &sys, &procs),
        StoreMsg::Event {
            ts,
            kind,
            pid,
            detail,
        } => store::events::insert_event(conn, ts, kind, pid, Some(&detail)).map(|_| ()),
        StoreMsg::Incident {
            ts,
            kind,
            identity_key,
            culprit,
            detail,
            etl_path,
            window_from,
            window_to,
        } => store::events::insert_incident(
            conn,
            ts,
            kind,
            identity_key.as_deref(),
            culprit.as_deref(),
            Some(&detail),
            etl_path.as_deref(),
            Some(window_from),
            Some(window_to),
        )
        .map(|_| ()),
    }
}

/// Sken po startu (SPEC 16.2): nový minidump → incident BSOD; nečisté
/// vypnutí s restartem stroje bez dumpu → incident bez dumpu. Dedup
/// přes čas incidentu, aby restart služby nezakládal duplicity.
fn startup_crash_scan(conn: &store::Connection) {
    // Úklid dat ze starší verze: incidenty bez viníka (sub-sekundové
    // workery) už nevznikají — staré záznamy jsou jen šum.
    let _ = conn.execute(
        "DELETE FROM incident WHERE kind = 'app_crash' AND (culprit IS NULL OR culprit = '')",
        [],
    );

    let was_clean = store::meta_get(conn, "clean_shutdown").as_deref() == Some("1");
    let _ = store::meta_set(conn, "clean_shutdown", "0");

    let last_scan: i64 = store::meta_get(conn, "bsod_scan_ts")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut newest = last_scan;
    let mut dump_found = false;
    let mut archived_etl: Option<String> = None;
    for f in collector_crash::scan_minidumps(last_scan) {
        newest = newest.max(f.ts);
        dump_found = true;
        match store::events::incident_exists(conn, "bsod", f.ts, 120) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(error = %e, "dedup BSOD selhal");
                continue;
            }
        }
        if archived_etl.is_none() {
            archived_etl = archive_blackbox(f.ts);
        }
        let detail = format!(
            "{{\"bugcheck\":{},\"params\":[{},{},{},{}],\"dump\":\"{}\"}}",
            f.bugcheck,
            f.params[0],
            f.params[1],
            f.params[2],
            f.params[3],
            crate::incidents::json_str(&f.dump_path),
        );
        tracing::warn!(bugcheck = f.bugcheck, human = f.human, "nalezen BSOD");
        if let Err(e) = store::events::insert_incident(
            conn,
            f.ts,
            "bsod",
            None,
            Some(f.human),
            Some(&detail),
            archived_etl.as_deref(),
            Some(f.ts - 300),
            Some(f.ts + 60),
        ) {
            tracing::error!(error = %e, "zápis BSOD incidentu selhal");
        }
    }
    if newest > last_scan {
        let _ = store::meta_set(conn, "bsod_scan_ts", &newest.to_string());
    }

    // Nečisté vypnutí bez dumpu: jen když se stroj skutečně restartoval
    // (boot je novější než poslední zapsaný vzorek) — jinak jde jen
    // o tvrdé ukončení služby, ne pád systému.
    if !was_clean && !dump_found {
        let boot_ts = unix_now() - win_sys::sysinfo::system_uptime_s() as i64;
        let last_sample: i64 = conn
            .query_row("SELECT COALESCE(MAX(ts), 0) FROM system_1s", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        if last_sample > 0 && boot_ts > last_sample {
            let exists =
                store::events::incident_exists(conn, "bsod", last_sample, 300).unwrap_or(true);
            if !exists {
                tracing::warn!("nečekané vypnutí bez dumpu (výpadek napájení / tvrdý pád)");
                let etl = archive_blackbox(last_sample);
                let _ = store::events::insert_incident(
                    conn,
                    last_sample,
                    "bsod",
                    None,
                    Some("nečekané vypnutí — bez minidumpu (výpadek napájení?)"),
                    Some("{\"no_dump\":true}"),
                    etl.as_deref(),
                    Some(last_sample - 300),
                    Some(last_sample),
                );
            }
        }
    }
}

/// Archivuje .etl černé skříňky z minulého běhu (obsahuje okno pádu),
/// než ji nová session přepíše. Vrací cestu k archivu.
fn archive_blackbox(ts: i64) -> Option<String> {
    let dir = store::data_dir().ok()?;
    let src = dir.join("blackbox.etl");
    if !src.exists() {
        return None;
    }
    let dst = dir.join(format!("incident-{ts}.etl"));
    match std::fs::rename(&src, &dst) {
        Ok(()) => Some(dst.to_string_lossy().into_owned()),
        Err(e) => {
            tracing::warn!(error = %e, "archivace černé skříňky selhala");
            None
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
