//! Jádro démona — společné pro oba režimy (--service i --console).
//!
//! v0 skládá: kontrolu integrity, config s hot-reloadem, SQLite store
//! s retenční smyčkou, IPC server a heartbeat „žiju“ 1×/s.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Kdy naposledy dopadl zápis inventáře do DB (unix, 0 = zatím nikdy)
/// a jestli sken zrovna běží. Sken trvá přes 20 s; bez tohohle signálu
/// by „Obnovit" v UI jen mlčelo a seznam by se změnil až někdy potom.
/// Jeden démon = jeden proces, proto stačí statické proměnné.
static INV_WRITTEN_TS: AtomicI64 = AtomicI64::new(0);
static INV_SCANNING: AtomicBool = AtomicBool::new(false);

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

/// Stav auto-indexace a úklidové analýzy (v4E) sdílený s IPC.
#[derive(Default)]
struct CleanupState {
    /// (svazek, záznamů zatím, hotovo).
    indexing: Vec<(char, u64, bool, Option<String>)>,
    running: bool,
    report: Option<core_types::proc::CleanupReport>,
}
type CleanupShared = Arc<std::sync::Mutex<CleanupState>>;

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
    // Klon pro inventární vlákno (ikony i pro neběžící aplikace).
    let icon_store_inv = Arc::clone(&icon_store);
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
    // Klony pro inventární vlákno a IPC handler (lazy velikosti cest).
    let inv_tx = sample_tx.clone();
    let size_tx = sample_tx.clone();

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
                                                // U systémové skupiny („Windows")
                                                // je viníkem konkrétní proces —
                                                // „pád aplikace Windows" nic neříká.
                                                let is_os = key.starts_with("os:");
                                                let _ = sample_tx.try_send(StoreMsg::Incident {
                                                    ts,
                                                    kind: "app_crash",
                                                    identity_key: (!key.is_empty())
                                                        .then_some(key),
                                                    culprit: Some(if app.is_empty() || is_os {
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

    // MFT indexy svazků (v4C, SPEC 11.2): staví se on-demand, drží se
    // v paměti a po 5 min nečinnosti je janitor uvolní (paměťový
    // rozpočet — velký svazek je ~50 MB indexu).
    type FsIndexes =
        Arc<std::sync::Mutex<std::collections::HashMap<char, (fs_index::VolumeIndex, Instant)>>>;
    let fs_indexes: FsIndexes = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let fs_indexes_janitor = Arc::clone(&fs_indexes);

    // Úklidová analýza (v4E): auto-indexace všech NTFS svazků na pozadí
    // + duplicity/0B/junk. Stav sdílený s IPC (progres do UI).
    let cleanup: CleanupShared = Arc::new(std::sync::Mutex::new(CleanupState::default()));
    let cleanup_ipc = Arc::clone(&cleanup);
    let fs_indexes_auto = Arc::clone(&fs_indexes);
    let cleanup_handle = {
        let stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("cleanup".into())
            .spawn(move || {
                let _ = win_sys::threading::set_current_thread_below_normal();
                // Počkat, až se systém po startu služby usadí.
                wait_or_stop(&stop, Duration::from_secs(15));
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                let letters: Vec<char> = win_sys::volumes::volumes()
                    .into_iter()
                    .filter(|v| v.fixed && v.fs == "NTFS")
                    .map(|v| v.letter)
                    .collect();
                {
                    let mut c = cleanup.lock().expect("cleanup lock");
                    c.indexing = letters.iter().map(|&l| (l, 0, false, None)).collect();
                }
                let mut built = Vec::new();
                for &letter in &letters {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let progress = Arc::clone(&cleanup);
                    match fs_index::VolumeIndex::build_with(letter, move |n| {
                        let mut c = progress.lock().expect("cleanup lock");
                        if let Some(e) = c.indexing.iter_mut().find(|e| e.0 == letter) {
                            e.1 = n;
                        }
                    }) {
                        Ok(idx) => {
                            let entries = idx.len() as u64;
                            let mut c = cleanup.lock().expect("cleanup lock");
                            if let Some(e) = c.indexing.iter_mut().find(|e| e.0 == letter) {
                                *e = (letter, entries, true, None);
                            }
                            tracing::info!(volume = %letter, entries, "auto-index hotový");
                            built.push(idx);
                        }
                        Err(e) => {
                            // Chybu si pamatujeme a řekneme ji v UI —
                            // „disk chybí" je horší než „nešel a proč".
                            let mut c = cleanup.lock().expect("cleanup lock");
                            if let Some(slot) = c.indexing.iter_mut().find(|s| s.0 == letter) {
                                *slot = (letter, 0, true, Some(index_error_human(&e)));
                            }
                            drop(c);
                            tracing::warn!(volume = %letter, error = %e, "auto-index selhal")
                        }
                    }
                }
                // Analýza nad lokálně drženými indexy (zámek map se
                // nedrží — search jede dál); pak indexy do sdílené mapy
                // pro rychlé hledání.
                cleanup.lock().expect("cleanup lock").running = true;
                let t0 = Instant::now();
                let refs: Vec<&fs_index::VolumeIndex> = built.iter().collect();
                let rep = fs_index::cleanup_analysis(&refs);
                tracing::info!(
                    dups = rep.dups.len(),
                    zero = rep.zero_byte.len(),
                    ms = t0.elapsed().as_millis() as u64,
                    "úklidová analýza hotová"
                );
                // Největší soubory a složky po svazcích (v4F) — jeden
                // průchod stromem, velikosti z directory enumerace.
                let mut big_files = Vec::new();
                let mut big_dirs = Vec::new();
                for &letter in &letters {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let t = Instant::now();
                    let b = fs_index::largest_items(&format!("{letter}:\\"), 60, 900_000);
                    tracing::info!(
                        volume = %letter,
                        ms = t.elapsed().as_millis() as u64,
                        "největší položky spočteny"
                    );
                    big_files.extend(b.files.into_iter().map(|(p, s)| (letter, p, s)));
                    big_dirs.extend(b.dirs.into_iter().map(|(p, s)| (letter, p, s)));
                }
                {
                    let mut c = cleanup.lock().expect("cleanup lock");
                    c.running = false;
                    c.report = Some(core_types::proc::CleanupReport {
                        dups: rep.dups,
                        zero_byte: rep.zero_byte,
                        junk: rep.junk,
                        finished_ts: unix_now(),
                        big_files,
                        big_dirs,
                    });
                }
                let mut map = fs_indexes_auto.lock().expect("fs index lock");
                for idx in built {
                    map.insert(idx.letter, (idx, Instant::now()));
                }
            })?
    };

    // Inventář aplikací (v4, SPEC kap. 5): sken na pozadí při startu
    // a pak řídce (6 h); RescanApps ho vyžádá dřív. Nikdy v cyklu.
    let rescan = Arc::new(AtomicBool::new(false));
    let inv_handle = {
        let stop = Arc::clone(&stop);
        let rescan = Arc::clone(&rescan);
        let tx = inv_tx;
        std::thread::Builder::new()
            .name("inventory".into())
            .spawn(move || {
                let _ = win_sys::threading::set_current_thread_below_normal();
                // COM pro WIC dekódování PNG log (MSIX ikony).
                win_sys::wic::init_com_for_thread();
                let mut last_scan: Option<Instant> = None;
                while !stop.load(Ordering::SeqCst) {
                    let due = last_scan.is_none_or(|t| t.elapsed() > Duration::from_secs(6 * 3600))
                        || rescan.swap(false, Ordering::SeqCst);
                    if due {
                        let t0 = Instant::now();
                        INV_SCANNING.store(true, Ordering::SeqCst);
                        let apps = collector_inv::scan();
                        tracing::info!(
                            apps = apps.len(),
                            ms = t0.elapsed().as_millis() as u64,
                            "sken inventáře hotový"
                        );
                        // Seznam do DB HNED, ještě před ikonami. Ikony
                        // trvají další desítky sekund a čekat s nimi
                        // znamenalo, že odinstalovaná aplikace zůstala
                        // v seznamu skoro minutu po tom, co zmizela.
                        let scan: Vec<store::apps::ScanApp> =
                            apps.iter().cloned().map(to_scan_app).collect();
                        if tx
                            .try_send(crate::incidents::StoreMsg::Inventory(scan))
                            .is_err()
                        {
                            tracing::warn!("zápis inventáře se nevešel do kanálu");
                        }
                        // Sken skončil tady — ikony se doplňují dál, ale
                        // seznam aplikací už je hotový a UI na něj čeká.
                        INV_SCANNING.store(false, Ordering::SeqCst);
                        // Ikony i pro aplikace, jejichž proces neběží —
                        // z DisplayIcon / zástupce / instalace / MSIX
                        // loga. Jednou na klíč, na BELOW_NORMAL.
                        let lnk_index = build_lnk_index();
                        let app_paths = win_sys::shortcut::all_app_paths();
                        let mut icons_added = 0u32;
                        for app in &apps {
                            if stop.load(Ordering::SeqCst) {
                                break;
                            }
                            let missing = {
                                let m = icon_store_inv.lock().expect("icon cache lock");
                                !matches!(m.get(&app.identity_key), Some(Some(_)))
                            };
                            if !missing {
                                continue;
                            }
                            if let Some(ico) = inventory_icon(app, &lnk_index, &app_paths) {
                                icon_store_inv
                                    .lock()
                                    .expect("icon cache lock")
                                    .insert(app.identity_key.clone(), Some(ico));
                                icons_added += 1;
                            }
                        }
                        tracing::info!(added = icons_added, "ikony inventáře doplněny");
                        last_scan = Some(Instant::now());
                    }
                    // Janitor MFT indexů: nepoužité 5 min → pryč.
                    fs_indexes_janitor.lock().expect("fs index lock").retain(
                        |letter, (idx, last)| {
                            let keep = last.elapsed() < Duration::from_secs(300);
                            if !keep {
                                tracing::info!(
                                    volume = %letter,
                                    entries = idx.len(),
                                    "MFT index uvolněn (nečinnost)"
                                );
                            }
                            keep
                        },
                    );
                    wait_or_stop(&stop, Duration::from_secs(2));
                }
            })?
    };

    // Orchestrátor mutací (v5, SPEC 17): vlastní zapisovací spojení
    // pro synchronní audit; mutace jsou vzácné, busy_timeout stačí.
    let orch = {
        let conn = store::open(&db_path)?;
        let _ = conn.busy_timeout(std::time::Duration::from_millis(2000));
        Arc::new(crate::actions::Orchestrator::new(conn))
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
        let rescan_flag = Arc::clone(&rescan);
        let fs_idx = Arc::clone(&fs_indexes);
        let cleanup_state = cleanup_ipc;
        let orch = Arc::clone(&orch);
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
            // Inventář aplikací (v4, SPEC kap. 5).
            Request::QueryApps => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::apps::list_apps(&conn) {
                    Ok(rows) => Response::Apps(rows),
                    Err(e) => Response::Error {
                        message: format!("čtení inventáře selhalo: {e}"),
                    },
                }
            }
            Request::QueryAppMap { identity_key } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::apps::app_map(&conn, &identity_key) {
                    Ok(rows) => Response::AppMap(rows),
                    Err(e) => Response::Error {
                        message: format!("čtení mapy souborů selhalo: {e}"),
                    },
                }
            }
            // Lazy velikosti (SPEC 5.2): spočítat teď, vrátit čerstvé,
            // uložit do cache přes zapisovací vlákno. Pomalé — ale
            // on-demand na výslovnou žádost UI, v obslužném vlákně
            // klienta, sběr dat to neblokuje.
            Request::ComputeAppSizes { identity_key } => {
                let map = {
                    let conn = read_conn.lock().expect("read conn lock poisoned");
                    store::apps::app_map(&conn, &identity_key)
                };
                match map {
                    Ok(mut rows) => {
                        let now = unix_now();
                        for p in rows.iter_mut() {
                            if p.role == "registry" {
                                continue;
                            }
                            let size = collector_inv::dir_size(&p.path);
                            p.size_bytes = Some(size);
                            p.size_ts = Some(now);
                            let _ = size_tx.try_send(crate::incidents::StoreMsg::PathSize {
                                identity_key: identity_key.clone(),
                                path: p.path.clone(),
                                size_bytes: size,
                                ts: now,
                            });
                        }
                        Response::AppMap(rows)
                    }
                    Err(e) => Response::Error {
                        message: format!("čtení mapy souborů selhalo: {e}"),
                    },
                }
            }
            Request::RescanApps => {
                rescan_flag.store(true, Ordering::SeqCst);
                Response::Ack
            }
            // Stav skenu — UI podle razítka pozná, že „Obnovit" doběhlo.
            Request::QueryInvStatus => Response::InvStatus {
                scanning: INV_SCANNING.load(Ordering::SeqCst),
                last_scan_ts: INV_WRITTEN_TS.load(Ordering::SeqCst),
            },
            // Svazky + zdraví disků (v4C, SPEC 11.1). NVMe health log;
            // SATA poctivě None (žádná vymyšlená čísla).
            Request::QueryVolumes => {
                let volumes = win_sys::volumes::volumes()
                    .into_iter()
                    .map(|v| core_types::proc::VolumeRow {
                        letter: v.letter,
                        label: v.label,
                        fs: v.fs,
                        total_bytes: v.total_bytes,
                        free_bytes: v.free_bytes,
                        fixed: v.fixed,
                        disk_index: v.disk_index,
                    })
                    .collect();
                let health = statics
                    .disks
                    .iter()
                    .map(|d| {
                        let h = win_sys::smart::nvme_health(d.index);
                        core_types::proc::DiskHealthRow {
                            index: d.index,
                            model: d.model.clone(),
                            temp_c: h.map(|x| x.temp_c),
                            used_pct: h.map(|x| x.used_pct),
                            spare_pct: h.map(|x| x.spare_pct),
                            power_on_hours: h.map(|x| x.power_on_hours),
                            critical: h.map(|x| x.critical_warning),
                        }
                    })
                    .collect();
                Response::Volumes { volumes, health }
            }
            // Stavba MFT indexu (sekundy) — blokuje jen toto spojení,
            // sběr dat běží dál.
            Request::BuildFileIndex { letter } => match fs_index::VolumeIndex::build(letter) {
                Ok(idx) => {
                    let entries = idx.len() as u64;
                    fs_idx
                        .lock()
                        .expect("fs index lock")
                        .insert(letter, (idx, Instant::now()));
                    tracing::info!(volume = %letter, entries, "MFT index postaven");
                    Response::IndexInfo { letter, entries }
                }
                Err(e) => Response::Error {
                    message: format!("stavba indexu selhala: {e}"),
                },
            },
            Request::SearchFiles {
                letter,
                query,
                limit,
            } => {
                let mut map = fs_idx.lock().expect("fs index lock");
                match map.get_mut(&letter) {
                    Some((idx, last)) => {
                        *last = Instant::now();
                        let hits = idx.search(&query, limit.min(300) as usize);
                        drop(map);
                        let rows = hits
                            .into_iter()
                            .map(|h| {
                                // Velikost jen u souborů (metadata je levné
                                // pro pár set nálezů).
                                let size = (h.attrs & fs_index::ATTR_DIR == 0)
                                    .then(|| std::fs::metadata(&h.path).ok().map(|m| m.len()))
                                    .flatten();
                                core_types::proc::FileHit {
                                    path: h.path,
                                    name: h.name,
                                    attrs: h.attrs,
                                    size_bytes: size,
                                }
                            })
                            .collect();
                        Response::Files(rows)
                    }
                    None => Response::Error {
                        message: "index svazku není postavený".into(),
                    },
                }
            }
            // Duplicity (v4D, SPEC 11.3) — pomalé, on-demand, jen čte.
            Request::FindDuplicates { root, min_size } => {
                let groups = fs_index::find_duplicates(&root, min_size.max(1), 200_000);
                Response::Duplicates(groups.into_iter().map(|g| (g.size, g.paths)).collect())
            }
            // Kdo drží soubory (v8, SPEC 18.1) — čistě čtecí dotaz na
            // Restart Manager; nic se neukončuje.
            Request::QueryHolders { paths } => match win_sys::rm::holders(&paths) {
                Ok(hs) => Response::Holders(
                    hs.into_iter()
                        .map(|h| core_types::proc::HolderRow {
                            pid: h.pid,
                            name: h.name,
                            kind: h.kind.as_str().to_string(),
                            service: h.service,
                        })
                        .collect(),
                ),
                Err(e) => Response::Error {
                    message: format!("Restart Manager selhal: {e}"),
                },
            },
            // Co po aplikaci zbylo (v8, SPEC 5.3): mapa souborů proti
            // disku. Čistě čtecí — mazání je samostatné rozhodnutí.
            Request::QueryLeftovers { identity_key } => {
                let map = {
                    let conn = read_conn.lock().expect("read conn lock poisoned");
                    store::apps::app_map(&conn, &identity_key)
                };
                match map {
                    Ok(rows) => {
                        let paths: Vec<String> = rows.into_iter().map(|p| p.path).collect();
                        Response::Leftovers(actor_app::leftovers(&paths))
                    }
                    Err(e) => Response::Error {
                        message: format!("čtení mapy souborů selhalo: {e}"),
                    },
                }
            }
            // Odinstalace krok 2: znovu zvalidovat a vydat příkaz —
            // spustí ho UI ve své relaci, ne služba (session 0).
            Request::AuthorizeUninstall { plan_id } => match orch.authorize_uninstall(plan_id) {
                Ok((command, audit_id)) => Response::UninstallAuthorized { command, audit_id },
                Err(res) => Response::ActionResult(res),
            },
            // Odinstalace krok 3: odinstalátor doběhl — ověřit registr
            // a doplnit výsledek k auditu.
            Request::ReportUninstall {
                audit_id,
                identity_key,
                detail,
            } => Response::ActionResult(orch.report_uninstall(audit_id, &identity_key, &detail)),
            // Stav auto-úklidu (v4E) — progres indexace + výsledek.
            Request::QueryCleanup => {
                let c = cleanup_state.lock().expect("cleanup lock");
                Response::Cleanup {
                    indexing: c.indexing.clone(),
                    running: c.running,
                    report: c.report.clone(),
                }
            }
            // Smazání VLASTNÍHO záznamu incidentu (žádná mutace OS).
            Request::DeleteIncident { id } => {
                let _ = size_tx.try_send(crate::incidents::StoreMsg::DeleteIncident(id));
                Response::Ack
            }
            // ── Mutační cesta (v5, SPEC 17) — vše přes orchestrátor,
            // nic se nespustí bez Verdict::Allow z validate/.
            Request::ToggleAction { action } => Response::ActionResult(orch.toggle(action)),
            Request::PlanAction { action } => match orch.plan(action) {
                Ok(plan) => Response::PlanReady(plan),
                Err(result) => Response::ActionResult(result),
            },
            Request::ExecuteAction { plan_id } => Response::ActionResult(orch.execute(plan_id)),
            // Startup položky (v6): čtení 6 backendů + spárování
            // s aplikací z inventáře (přes .exe cestu).
            Request::QueryStartup => {
                // Task Scheduler jde přes COM — obslužné vlákno pipe
                // ho musí mít inicializované (idempotentní).
                win_sys::wic::init_com_for_thread();
                let items = collector_boot::scan();
                let conn = read_conn.lock().expect("read conn lock poisoned");
                let apps = store::apps::list_apps(&conn).unwrap_or_default();
                // Instalační cesty aplikací pro párování. Obecné kořeny
                // (Windows, System32, Program Files) se vyhazují —
                // jinak by „vlastnily“ půlku systému.
                let too_generic = |p: &str| {
                    let depth = p.matches('\\').count();
                    depth < 2
                        || matches!(
                            p,
                            r"c:\windows"
                                | r"c:\windows\system32"
                                | r"c:\windows\syswow64"
                                | r"c:\program files"
                                | r"c:\program files (x86)"
                                | r"c:\programdata"
                        )
                };
                let maps: Vec<(String, Vec<String>)> = apps
                    .iter()
                    .map(|a| {
                        let paths = store::apps::app_map(&conn, &a.identity_key)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|p| p.role == "install")
                            .map(|p| p.path.to_lowercase())
                            .filter(|p| !too_generic(p))
                            .collect();
                        (a.identity_key.clone(), paths)
                    })
                    .collect();
                drop(conn);
                let rows = items
                    .into_iter()
                    .map(|it| {
                        // Aplikace, pod jejíž instalační cestu .exe spadá.
                        // Nejdelší SHODNÝ prefix vyhrává (nejspecifičtější
                        // instalace, ne aplikace s nejdelší cestou vůbec).
                        let key = it.exe_path.as_ref().and_then(|exe| {
                            let lc = exe.to_lowercase();
                            maps.iter()
                                .filter_map(|(k, paths)| {
                                    paths
                                        .iter()
                                        .filter(|p| {
                                            lc.starts_with(p.as_str())
                                                && lc.as_bytes().get(p.len()) == Some(&b'\\')
                                        })
                                        .map(|p| p.len())
                                        .max()
                                        .map(|len| (k.clone(), len))
                                })
                                .max_by_key(|(_, len)| *len)
                                .map(|(k, _)| k)
                        });
                        let app = key
                            .as_ref()
                            .and_then(|k| apps.iter().find(|a| &a.identity_key == k));
                        core_types::proc::StartupRow {
                            id: it.id,
                            name: it.name,
                            source: it.source.as_str().to_string(),
                            command: it.command,
                            enabled: it.enabled,
                            toggleable: it.source.toggleable(),
                            identity_key: key,
                            app_name: app.map(|a| a.display_name.clone()),
                            publisher: app.and_then(|a| a.publisher.clone()),
                        }
                    })
                    .collect();
                Response::Startup(rows)
            }
            Request::QueryAudit { limit } => {
                let conn = read_conn.lock().expect("read conn lock poisoned");
                match store::audit::recent(&conn, limit.min(500)) {
                    Ok(rows) => Response::Audit(rows),
                    Err(e) => Response::Error {
                        message: format!("čtení auditu selhalo: {e}"),
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
    let _ = inv_handle.join();
    // Úklidová analýza se ZÁMĚRNĚ nejoinuje: běží minuty (hashování
    // duplicit, průchod stromem) a čekání na ni by protahovalo vypnutí
    // služby o celou tu dobu — SCM by hlásil Stopped, ale proces by
    // dál držel binárku. Výsledek je jen cache v paměti; zahodit ho
    // při vypnutí nic nestojí. Vlákno skončí s procesem.
    drop(cleanup_handle);
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
        StoreMsg::Inventory(apps) => {
            let n = apps.len();
            let r = store::apps::replace_inventory(conn, &apps);
            if r.is_ok() {
                // Razítko až TEĎ, po dopsání — UI podle něj pozná, že
                // „Obnovit" doběhlo a v DB je opravdu nový stav.
                INV_WRITTEN_TS.store(unix_now(), Ordering::SeqCst);
                tracing::info!(apps = n, "inventář aplikací zapsán");
            }
            r
        }
        StoreMsg::PathSize {
            identity_key,
            path,
            size_bytes,
            ts,
        } => store::apps::set_path_size(conn, &identity_key, &path, size_bytes, ts),
        StoreMsg::DeleteIncident(id) => store::events::delete_incident(conn, id),
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

/// Lidský důvod, proč se svazek nepodařilo zindexovat — uživatel má
/// vědět proč, ne jen „nejde".
fn index_error_human(e: &fs_index::Error) -> String {
    let s = e.to_string();
    // Kódy z CreateFileW na svazek: 5 = odepřen přístup (bez elevace),
    // 1 = nepodporováno (ne-NTFS), 32 = uzamčeno jiným procesem.
    if s.contains("code: 5") {
        "přístup odepřen — služba potřebuje práva správce".into()
    } else if s.contains("code: 1") || s.contains("code: 50") {
        "svazek nepodporuje USN žurnál (jen NTFS)".into()
    } else if s.contains("code: 32") {
        "svazek je uzamčený jiným programem".into()
    } else if s.contains("code: 21") {
        "zařízení není připravené (odpojený disk?)".into()
    } else {
        format!("nepodařilo se přečíst MFT ({s})")
    }
}

/// Normalizace názvu pro párování aplikace ↔ zástupce: malá písmena,
/// jen alfanumerické znaky. „Google Chrome" == „google chrome" ==
/// „GoogleChrome"; verze a interpunkce nepřekáží.
fn norm_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Index zástupců ze Start Menu: normalizovaný název → cesta k .lnk.
/// Zástupce je nejspolehlivější vodítko — instalátor ho vytvořil přesně
/// pro tu aplikaci. Ikona se ale MUSÍ vzít z cíle zástupce: .lnk není
/// PE soubor, takže by spadla na generickou shell ikonu (session 0).
fn build_lnk_index() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut roots: Vec<String> = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        roots.push(format!(r"{pd}\Microsoft\Windows\Start Menu\Programs"));
    }
    let users = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + r"\Users";
    if let Ok(rd) = std::fs::read_dir(&users) {
        for e in rd.flatten() {
            roots.push(format!(
                r"{}\AppData\Roaming\Microsoft\Windows\Start Menu\Programs",
                e.path().to_string_lossy()
            ));
        }
    }
    let mut stack: Vec<(std::path::PathBuf, u8)> =
        roots.into_iter().map(|r| (r.into(), 0u8)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                if let Some(stem) = p.file_stem() {
                    map.entry(norm_name(&stem.to_string_lossy()))
                        .or_insert_with(|| p.to_string_lossy().into_owned());
                }
            }
        }
    }
    map
}

/// Ikona aplikace z inventáře — řetěz zdrojů: DisplayIcon/UninstallString
/// → zástupce ze Start Menu (shell vyřeší cíl) → .exe v instalaci
/// (i o úroveň hlouběji) → PNG logo MSIX balíčku (WIC).
fn inventory_icon(
    app: &collector_inv::AppEntry,
    lnk_index: &std::collections::HashMap<String, String>,
    app_paths: &[(String, String)],
) -> Option<core_types::proc::IconData> {
    if let Some(hint) = app.icon_hint.as_deref() {
        let trimmed = hint.trim().trim_matches('"');
        if !std::path::Path::new(trimmed).is_dir() {
            if let Some(i) = win_sys::icon::extract_spec(hint) {
                return Some(to_icon_data(i));
            }
        }
    }

    // Zástupce ze Start Menu — přesná i částečná shoda normalizovaných
    // jmen („Discord Inc. → Discord"). Ikona se bere z CÍLE zástupce
    // (PE resource), případně z jeho IconLocation.
    let want = norm_name(&app.display_name);
    let lnk = lnk_index.get(&want).cloned().or_else(|| {
        // Částečná shoda: zástupce, jehož jméno je v názvu aplikace
        // (nebo naopak) a je dost dlouhé, aby to nebyla náhoda.
        lnk_index
            .iter()
            .filter(|(k, _)| k.len() >= 4 && (want.starts_with(k.as_str()) || k.starts_with(&want)))
            .max_by_key(|(k, _)| k.len())
            .map(|(_, v)| v.clone())
    });
    if let Some(lnk) = lnk {
        if let Some(target) = win_sys::shortcut::resolve_lnk(&lnk) {
            if let Some(i) = win_sys::icon::extract(&target) {
                return Some(to_icon_data(i));
            }
        }
        if let Some((icon_path, idx)) = win_sys::shortcut::lnk_icon_location(&lnk) {
            let spec = format!("{icon_path},{idx}");
            if let Some(i) = win_sys::icon::extract_spec(&spec) {
                return Some(to_icon_data(i));
            }
        }
    }

    // Registrované App Paths — aplikace tam samy hlásí svoje .exe.
    if let Some((_, exe)) = app_paths.iter().find(|(name, _)| {
        let stem = name.trim_end_matches(".exe");
        stem.len() >= 4 && (want.starts_with(stem) || stem.starts_with(&want))
    }) {
        if let Some(i) = win_sys::icon::extract(exe) {
            return Some(to_icon_data(i));
        }
    }
    let mut dirs: Vec<&str> = app
        .paths
        .iter()
        .filter(|p| p.role == "install")
        .map(|p| p.path.as_str())
        .collect();
    if let Some(hint) = app.icon_hint.as_deref() {
        if std::path::Path::new(hint).is_dir() {
            dirs.insert(0, hint);
        }
    }
    for d in &dirs {
        if let Some(i) = icon_from_dir(d, &want) {
            return Some(i);
        }
    }
    // MSIX: PNG logo z assetů balíčku (WIC dekódování).
    if app.kind == "msix" {
        for d in &dirs {
            if let Some(i) = msix_logo(d) {
                return Some(i);
            }
        }
    }
    None
}

/// Najde logo PNG v adresáři MSIX balíčku (Assets\*logo*.png apod.).
fn msix_logo(dir: &str) -> Option<core_types::proc::IconData> {
    let mut candidates: Vec<(u32, std::path::PathBuf)> = Vec::new();
    // Assets bývá i zanořené (Assets\Logos, Images\…), proto průchod
    // do dvou úrovní — jinak balíčky jako XboxCompanion ikonu nemají
    // kde vzít.
    let mut bases: Vec<std::path::PathBuf> = vec![
        format!("{dir}\\Assets").into(),
        dir.into(),
        format!("{dir}\\Images").into(),
    ];
    let mut extra = Vec::new();
    for b in &bases {
        if let Ok(rd) = std::fs::read_dir(b) {
            extra.extend(
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .take(6),
            );
        }
    }
    bases.extend(extra);
    for base in bases {
        let Ok(rd) = std::fs::read_dir(&base) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")) {
                continue;
            }
            let n = p
                .file_name()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let score = if n.contains("square44") {
                0
            } else if n.contains("storelogo") {
                1
            } else if n.contains("square150") {
                2
            } else if n.contains("logo") {
                3
            } else if n.contains("icon") || n.contains("applist") || n.contains("tile") {
                4
            } else {
                // Poslední možnost: jakýkoli malý PNG (balíčky bez
                // standardních jmen log).
                let small = std::fs::metadata(&p)
                    .map(|m| m.len() < 120_000)
                    .unwrap_or(false);
                if small {
                    5
                } else {
                    continue;
                }
            };
            candidates.push((score * 1000 + n.len() as u32, p));
        }
    }
    candidates.sort_by_key(|(s, _)| *s);
    for (_, p) in candidates.into_iter().take(4) {
        if let Some(i) = win_sys::wic::decode(&p.to_string_lossy()) {
            return Some(to_icon_data(i));
        }
    }
    None
}

/// První použitelná ikona z .exe v adresáři (nejkratší jméno bývá
/// hlavní binárka — instalátory/updatery mívají dlouhá).
fn icon_from_dir(dir: &str, hint: &str) -> Option<core_types::proc::IconData> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut exes: Vec<std::path::PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
        .collect();
    // Nejdřív .exe, jehož jméno odpovídá aplikaci (Discord.exe pro
    // Discord), pak kratší jména — instalátory a updatery mívají
    // dlouhá („DiscordSetup", „unins000").
    exes.sort_by_key(|p| {
        let stem = norm_name(
            &p.file_stem()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default(),
        );
        let matches = !hint.is_empty()
            && stem.len() >= 3
            && (hint.starts_with(&stem) || stem.starts_with(hint));
        let junk = stem.contains("unins") || stem.contains("setup") || stem.contains("update");
        (!matches, junk, stem.len())
    });
    for exe in exes.into_iter().take(4) {
        if let Some(i) = win_sys::icon::extract(exe.to_str()?) {
            return Some(to_icon_data(i));
        }
    }
    // Hlouběji (bin/, app-1.2.3/, Client/…) — spousta aplikací nemá
    // .exe přímo v kořeni instalace. Do 3 úrovní, se stejným
    // hodnocením jmen; .dll až nakonec (bundly tam nosí ikonu).
    let mut stack: Vec<(std::path::PathBuf, u8)> = vec![(dir.into(), 0)];
    let mut candidates: Vec<(bool, bool, usize, std::path::PathBuf)> = Vec::new();
    let mut dirs_seen = 0usize;
    while let Some((d, depth)) = stack.pop() {
        dirs_seen += 1;
        if dirs_seen > 40 || candidates.len() > 24 {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let ext_ico = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ico"));
            let is_exe = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe"));
            if !is_exe && !ext_ico {
                continue;
            }
            let stem = norm_name(
                &p.file_stem()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default(),
            );
            let matches = !hint.is_empty()
                && stem.len() >= 3
                && (hint.starts_with(&stem) || stem.starts_with(hint));
            let junk = stem.contains("unins")
                || stem.contains("setup")
                || stem.contains("update")
                || stem.contains("crashpad")
                || stem.contains("vcredist");
            // .ico má přednost před .exe při stejné shodě jména.
            candidates.push((!matches, junk, if ext_ico { 0 } else { stem.len() + 1 }, p));
        }
    }
    candidates.sort_by_key(|c| (c.0, c.1, c.2));
    for (_, _, _, exe) in candidates.into_iter().take(6) {
        if let Some(i) = win_sys::icon::extract(exe.to_str()?) {
            return Some(to_icon_data(i));
        }
    }
    None
}

fn to_icon_data(i: win_sys::icon::IconRgba) -> core_types::proc::IconData {
    core_types::proc::IconData {
        w: i.w,
        h: i.h,
        rgba: i.rgba,
    }
}

/// Převod výsledku skenu inventáře na store tvar (store nesmí záviset
/// na kolektorech — oddělené cesty, SPEC kap. 2).
fn to_scan_app(a: collector_inv::AppEntry) -> store::apps::ScanApp {
    store::apps::ScanApp {
        identity_key: a.identity_key,
        kind: a.kind.to_string(),
        display_name: a.display_name,
        publisher: a.publisher,
        version: a.version,
        install_ts: a.install_ts,
        paths: a
            .paths
            .into_iter()
            .map(|p| store::apps::ScanPath {
                path: p.path,
                role: p.role.to_string(),
                source: p.source.to_string(),
                confidence: p.confidence.to_string(),
            })
            .collect(),
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
