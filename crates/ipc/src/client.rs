//! Named pipe klient — používá UI (a testy) k dotazům na službu.
//!
//! Klientská strana pipe jde otevřít přes std::fs — server instanci
//! vytvořil, my ji jen otevíráme jako soubor pro čtení+zápis.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::time::Duration;

use core_types::ipc::{Request, Response, PROTOCOL_VERSION};
use core_types::proc::{
    DiskRate, GpuInfo, HistProcRow, ProcRow, StaticInfo, SystemPoint, SystemSnapshot,
};

use crate::{frame, Error, PIPE_NAME};

/// Výsledek úspěšného pingu služby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PongInfo {
    pub protocol_version: u32,
    pub uptime_s: u64,
}

/// Otevře spojení na službu. `Error::NotAvailable` = služba neběží.
/// ERROR_PIPE_BUSY (všechny instance obsazené) řeší krátký retry.
pub fn connect() -> Result<File, Error> {
    const ERROR_PIPE_BUSY: i32 = 231;
    const ATTEMPTS: u32 = 3;

    for attempt in 0..ATTEMPTS {
        match OpenOptions::new().read(true).write(true).open(PIPE_NAME) {
            Ok(f) => return Ok(f),
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(Error::NotAvailable),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempt + 1 < ATTEMPTS => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::NotAvailable)
}

/// Pošle požadavek a přečte jednu odpověď.
pub fn request(stream: &mut File, req: &Request) -> Result<Response, Error> {
    frame::write_msg(stream, req)?;
    frame::read_msg(stream)?.ok_or(Error::NotAvailable)
}

/// „Žiješ?“ — jedno spojení, jeden ping, jedna odpověď.
pub fn ping() -> Result<PongInfo, Error> {
    let mut stream = connect()?;
    match request(
        &mut stream,
        &Request::Ping {
            protocol_version: PROTOCOL_VERSION,
        },
    )? {
        Response::Pong {
            protocol_version,
            uptime_s,
        } => Ok(PongInfo {
            protocol_version,
            uptime_s,
        }),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Aktuální snapshot procesů ze sampleru služby.
pub fn query_procs() -> Result<Vec<ProcRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryProcs)? {
        Response::Procs(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Vlastní spotřeba nástroje (SPEC kap. 2.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelfUsage {
    pub cpu_pct: f32,
    pub ws_bytes: u64,
    pub db_bytes: u64,
}

/// Dotaz na vlastní spotřebu služby.
pub fn query_self_usage() -> Result<SelfUsage, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QuerySelfUsage)? {
        Response::SelfUsage {
            cpu_pct,
            ws_bytes,
            db_bytes,
        } => Ok(SelfUsage {
            cpu_pct,
            ws_bytes,
            db_bytes,
        }),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Historie systémových metrik [from, to] ze system_1s.
pub fn query_system_history(from: i64, to: i64) -> Result<Vec<SystemPoint>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QuerySystemHistory { from, to })? {
        Response::SystemHistory(points) => Ok(points),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Stav procesů v čase (nejbližší vzorek ±2 s).
pub fn query_procs_at(ts: i64) -> Result<(i64, Vec<HistProcRow>), Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryProcsAt { ts })? {
        Response::ProcsAt { ts, rows } => Ok((ts, rows)),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Statické informace o komponentách.
pub fn query_sys_info() -> Result<StaticInfo, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QuerySysInfo)? {
        Response::SysInfo(info) => Ok(info),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Detaily proměnných v čase (jádra, disky, GPU).
#[allow(clippy::type_complexity)]
pub fn query_detail_at(ts: i64) -> Result<(i64, Vec<f32>, Vec<DiskRate>, Option<GpuInfo>), Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryDetailAt { ts })? {
        Response::DetailAt {
            ts,
            cores,
            disks,
            gpu,
        } => Ok((ts, cores, disks, gpu)),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Historie jader CPU [from, to].
pub fn query_core_history(from: i64, to: i64) -> Result<Vec<(i64, u32, f32)>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryCoreHistory { from, to })? {
        Response::CoreHistory(points) => Ok(points),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Ikona aplikace podle identity_key (None = ikona není / ještě není hotová).
pub fn query_icon(identity_key: String) -> Result<Option<core_types::proc::IconData>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryIcon { identity_key })? {
        Response::Icon(icon) => Ok(icon),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Historie disků [from, to].
pub fn query_disk_history(from: i64, to: i64) -> Result<Vec<(i64, u32, u64, u64)>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryDiskHistory { from, to })? {
        Response::DiskHistory(points) => Ok(points),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Události (záseky, pády) v rozsahu — markery na časové ose (v3).
pub fn query_events(from: i64, to: i64) -> Result<Vec<core_types::proc::EventRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryEvents { from, to })? {
        Response::Events(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Poslední incidenty (nejnovější první).
pub fn query_incidents(limit: u32) -> Result<Vec<core_types::proc::IncidentRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryIncidents { limit })? {
        Response::Incidents(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Inventář aplikací (v4).
pub fn query_apps() -> Result<Vec<core_types::proc::AppRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryApps)? {
        Response::Apps(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Mapa souborů aplikace.
pub fn query_app_map(identity_key: String) -> Result<Vec<core_types::proc::AppPathRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryAppMap { identity_key })? {
        Response::AppMap(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Spočítá velikosti cest aplikace (pomalé, on-demand) a vrátí mapu.
pub fn compute_app_sizes(identity_key: String) -> Result<Vec<core_types::proc::AppPathRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::ComputeAppSizes { identity_key })? {
        Response::AppMap(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Vyžádá nový sken inventáře.
pub fn rescan_apps() -> Result<(), Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::RescanApps)? {
        Response::Ack => Ok(()),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Svazky + zdraví fyzických disků (v4C).
#[allow(clippy::type_complexity)]
pub fn query_volumes() -> Result<
    (
        Vec<core_types::proc::VolumeRow>,
        Vec<core_types::proc::DiskHealthRow>,
    ),
    Error,
> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryVolumes)? {
        Response::Volumes { volumes, health } => Ok((volumes, health)),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Postaví MFT index svazku; vrací počet záznamů.
pub fn build_file_index(letter: char) -> Result<u64, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::BuildFileIndex { letter })? {
        Response::IndexInfo { entries, .. } => Ok(entries),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Hledání v MFT indexu svazku.
pub fn search_files(
    letter: char,
    query: String,
    limit: u32,
) -> Result<Vec<core_types::proc::FileHit>, Error> {
    let mut stream = connect()?;
    match request(
        &mut stream,
        &Request::SearchFiles {
            letter,
            query,
            limit,
        },
    )? {
        Response::Files(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// T0 akce (v5): validace + provedení + ověření v jednom.
pub fn toggle_action(
    action: core_types::action::Action,
) -> Result<core_types::action::ActionResult, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::ToggleAction { action })? {
        Response::ActionResult(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// T1 fáze 1 (v5): plán, nebo rovnou deny výsledek.
#[allow(clippy::result_large_err)]
pub fn plan_action(
    action: core_types::action::Action,
) -> Result<Result<core_types::action::ActionPlan, core_types::action::ActionResult>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::PlanAction { action })? {
        Response::PlanReady(p) => Ok(Ok(p)),
        Response::ActionResult(r) => Ok(Err(r)),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// T1 fáze 2–4 (v5): provedení potvrzeného plánu.
pub fn execute_action(plan_id: u64) -> Result<core_types::action::ActionResult, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::ExecuteAction { plan_id })? {
        Response::ActionResult(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Startup položky (v6).
pub fn query_startup() -> Result<Vec<core_types::proc::StartupRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryStartup)? {
        Response::Startup(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Auditní záznamy (v5).
pub fn query_audit(limit: u32) -> Result<Vec<core_types::action::AuditRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryAudit { limit })? {
        Response::Audit(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Kdo drží soubory (v8) — Restart Manager.
pub fn query_holders(paths: Vec<String>) -> Result<Vec<core_types::proc::HolderRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryHolders { paths })? {
        Response::Holders(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Co po aplikaci zbylo na disku (v8).
pub fn query_leftovers(identity_key: String) -> Result<Vec<String>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryLeftovers { identity_key })? {
        Response::Leftovers(paths) => Ok(paths),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Stav skenu inventáře: (běží zrovna, kdy dopadl poslední zápis).
pub fn query_inv_status() -> Result<(bool, i64), Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryInvStatus)? {
        Response::InvStatus {
            scanning,
            last_scan_ts,
        } => Ok((scanning, last_scan_ts)),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Schválení odinstalace (v8): služba znovu validuje a vrátí příkaz,
/// který má volající spustit VE SVÉ relaci. Při zamítnutí vrací
/// `ActionResult` s důvodem.
pub fn authorize_uninstall(
    plan_id: u64,
) -> Result<Result<(String, i64), core_types::action::ActionResult>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::AuthorizeUninstall { plan_id })? {
        Response::UninstallAuthorized { command, audit_id } => Ok(Ok((command, audit_id))),
        Response::ActionResult(r) => Ok(Err(r)),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Hlášení konce odinstalátoru (v8) — služba ověří registr a doplní
/// výsledek do auditu.
pub fn report_uninstall(
    audit_id: i64,
    identity_key: String,
    detail: String,
) -> Result<core_types::action::ActionResult, Error> {
    let mut stream = connect()?;
    match request(
        &mut stream,
        &Request::ReportUninstall {
            audit_id,
            identity_key,
            detail,
        },
    )? {
        Response::ActionResult(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Stav auto-úklidu (v4E): indexace, běh analýzy, výsledek.
#[allow(clippy::type_complexity)]
pub fn query_cleanup() -> Result<
    (
        Vec<(char, u64, bool, Option<String>)>,
        bool,
        Option<core_types::proc::CleanupReport>,
    ),
    Error,
> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryCleanup)? {
        Response::Cleanup {
            indexing,
            running,
            report,
        } => Ok((indexing, running, report)),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Smaže záznam incidentu.
pub fn delete_incident(id: i64) -> Result<(), Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::DeleteIncident { id })? {
        Response::Ack => Ok(()),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Duplicity pod kořenem (v4D): skupiny (velikost, cesty).
pub fn find_duplicates(root: String, min_size: u64) -> Result<Vec<(u64, Vec<String>)>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::FindDuplicates { root, min_size })? {
        Response::Duplicates(groups) => Ok(groups),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Aktuální systémové metriky ze sampleru služby.
pub fn query_system() -> Result<SystemSnapshot, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QuerySystem)? {
        Response::System(s) => Ok(s),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Hardwarový přehled (v9): deska, BIOS, baterie, teploty, disky.
pub fn query_hardware() -> Result<core_types::proc::HardwareReport, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryHardware)? {
        Response::Hardware(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Spojení per aplikace (v9): kdo je připojený kam, porty, PTR jména.
pub fn query_network() -> Result<Vec<core_types::proc::AppNetRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryNetwork)? {
        Response::Network(rows) => Ok(rows),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Stav připojení (v9): adaptéry, IP konfigurace, WiFi.
pub fn query_connection() -> Result<core_types::proc::ConnectionReport, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryConnection)? {
        Response::Connection(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}




/// Hlášení o pádech z protokolu Windows, přeložená do lidské řeči.
pub fn query_crash_reports(limit: u32) -> Result<Vec<core_types::proc::CrashReportRow>, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryCrashReports { limit })? {
        Response::CrashReports(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Stav sběru — diagnostika prázdné tabulky.
pub fn query_collector_health() -> Result<core_types::proc::CollectorHealth, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryCollectorHealth)? {
        Response::CollectorHealth(h) => Ok(h),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Ovladače (v10): co běží, od koho a jak staré.
pub fn query_drivers() -> Result<core_types::proc::DriversReport, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryDrivers)? {
        Response::Drivers(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Users (v9E): účty na tomhle počítači a kdo z nich je správce.
pub fn query_users() -> Result<core_types::proc::UsersReport, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QueryUsers)? {
        Response::Users(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}


/// Historie použití oprávnění (v9D): sezení za posledních `days` dní
/// a jejich součet v sekundách.
pub fn query_perm_use(
    app: String,
    capability: String,
    days: u32,
) -> Result<(Vec<core_types::proc::PermUseRow>, i64), Error> {
    let mut stream = connect()?;
    match request(
        &mut stream,
        &Request::QueryPermUse {
            app,
            capability,
            days,
        },
    )? {
        Response::PermUse { sessions, total_s } => Ok((sessions, total_s)),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}

/// Security (v9): stav ochrany + oprávnění aplikací.
pub fn query_security() -> Result<core_types::proc::SecurityReport, Error> {
    let mut stream = connect()?;
    match request(&mut stream, &Request::QuerySecurity)? {
        Response::Security(r) => Ok(r),
        Response::Error { message } => Err(Error::Remote { message }),
        other => Err(Error::Remote {
            message: format!("nečekaná odpověď: {other:?}"),
        }),
    }
}
