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
