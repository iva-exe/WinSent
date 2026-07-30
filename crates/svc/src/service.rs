//! Režim `--service` — běh pod Service Control Managerem.
//!
//! SCM handshake: registrace control handleru, přechod do Running,
//! reakce na Stop/Shutdown, po úklidu Stopped. Logy jdou do souboru
//! v `%ProgramData%\syswatch\logs\` (session 0 nemá konzoli).

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

use crate::daemon;

/// Jméno služby v SCM. Musí sedět se `service.ps1` a `sc failure`.
pub const SERVICE_NAME: &str = "syswatch";

define_windows_service!(ffi_service_main, service_main);

/// Vstup z main: připojí proces na SCM dispatcher (blokuje do konce služby).
pub fn run() -> windows_service::Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

/// Tělo služby — volá SCM po startu dispatcheru.
fn service_main(_args: Vec<OsString>) {
    init_file_logging();
    if let Err(e) = run_service() {
        tracing::error!(error = %e, "služba spadla při startu/běhu");
    }
}

/// Chyby servisního režimu.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("SCM: {0}")]
    Scm(#[from] windows_service::Error),
    #[error(transparent)]
    Daemon(#[from] daemon::Error),
}

/// SCM handshake + běh démona + korektní přechody stavů.
fn run_service() -> Result<(), Error> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_handler = Arc::clone(&stop);

    let status_handle = service_control_handler::register(SERVICE_NAME, move |control| {
        match control {
            // Stop i Shutdown ukončují stejně — nastavit flag a probudit
            // IPC server z blokujícího čekání.
            ServiceControl::Stop | ServiceControl::Shutdown => {
                stop_handler.store(true, Ordering::SeqCst);
                ipc::server::wake();
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    })?;

    let running = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    };
    status_handle.set_service_status(running.clone())?;

    let result = daemon::run(stop);

    // I při chybě démona musí služba ohlásit Stopped — jinak v SCM visí.
    let exit_code = match &result {
        Ok(()) => ServiceExitCode::Win32(0),
        Err(_) => ServiceExitCode::ServiceSpecific(1),
    };
    status_handle.set_service_status(ServiceStatus {
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code,
        ..running
    })?;

    result.map_err(Error::from)
}

/// Strop jednoho log souboru. Přes limit se svc.log přejmenuje na
/// svc.log.1 (starší .1 se zahodí) — celkem tedy nejvýš ~16 MB.
/// Bez rotace rostl log append-only donekonečna; data nástroje musí
/// mít strop vždy (SPEC kap. 2.3).
const LOG_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Zapisovač s rotací podle velikosti. Velikost se počítá při zápisu,
/// takže rotace funguje i za dlouhého běhu služby, ne jen při startu.
struct RotatingLog {
    path: std::path::PathBuf,
    file: std::fs::File,
    written: u64,
}

impl RotatingLog {
    fn open(path: std::path::PathBuf) -> std::io::Result<RotatingLog> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(RotatingLog {
            path,
            file,
            written,
        })
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        let old = self.path.with_extension("log.1");
        // Selhání přejmenování (soubor drží čtečka) rotaci jen odloží.
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&self.path, &old);
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.written = 0;
        Ok(())
    }
}

impl std::io::Write for RotatingLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written > LOG_MAX_BYTES {
            self.rotate()?;
        }
        let n = std::io::Write::write(&mut self.file, buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut self.file)
    }
}

/// Logování do souboru — služba v session 0 nemá stdout.
/// Selhání logování nesmí zabránit startu služby (horší než bez logů).
fn init_file_logging() {
    let Ok(dir) = store::data_dir() else { return };
    let log_dir = dir.join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let Ok(log) = RotatingLog::open(log_dir.join("svc.log")) else {
        return;
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(Mutex::new(log))
        .with_ansi(false)
        .try_init();
}
