//! Registrace a ovládání služby syswatch.
//!
//! Přes `windows-service` místo volání `sc.exe`: cesta k binárce
//! obsahuje mezery („C:\Program Files\Winsent\…") a předávat ji
//! přes příkazovou řádku znamená hádat se s uvozovkami. Tady jde
//! rovnou do API.

use std::ffi::OsString;
use std::time::{Duration, Instant};

use windows_service::service::{
    ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

pub const NAME: &str = "syswatch";
const DISPLAY: &str = "Winsent — systémový monitor";
const DESCRIPTION: &str =
    "Sbírá metriky systému pro aplikaci Winsent. Data zůstávají v tomto počítači.";

pub type Result<T> = std::result::Result<T, String>;

fn manager(access: ServiceManagerAccess) -> Result<ServiceManager> {
    ServiceManager::local_computer(None::<&str>, access)
        .map_err(|e| format!("nelze otevřít správce služeb: {e}"))
}

/// Zastaví službu a počká, až opravdu skončí. Bez čekání by zůstal
/// zamčený .exe a kopie nové verze by selhala.
pub fn stop_and_wait() -> Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    let svc = match mgr.open_service(NAME, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS) {
        Ok(s) => s,
        // Služba neexistuje — čistá instalace, není co zastavovat.
        Err(_) => return Ok(()),
    };
    let status = svc
        .query_status()
        .map_err(|e| format!("nelze zjistit stav služby: {e}"))?;
    if status.current_state == ServiceState::Stopped {
        return Ok(());
    }
    let _ = svc.stop();

    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(300));
        if let Ok(s) = svc.query_status() {
            if s.current_state == ServiceState::Stopped {
                return Ok(());
            }
        }
    }
    Err("služba se nezastavila do 30 s".into())
}

/// Zaregistruje službu, nebo u existující jen aktualizuje konfiguraci.
pub fn install(exe: &std::path::Path) -> Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    let info = ServiceInfo {
        name: OsString::from(NAME),
        display_name: OsString::from(DISPLAY),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe.to_path_buf(),
        launch_arguments: vec![OsString::from("--service")],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };

    let access = ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS;
    match mgr.open_service(NAME, access) {
        // Aktualizace: přepíšeme konfiguraci (cesta se mohla změnit).
        Ok(svc) => {
            svc.change_config(&info)
                .map_err(|e| format!("nelze aktualizovat službu: {e}"))?;
            svc.set_description(DESCRIPTION)
                .map_err(|e| format!("nelze nastavit popis: {e}"))?;
        }
        Err(_) => {
            let svc = mgr
                .create_service(&info, access)
                .map_err(|e| format!("nelze vytvořit službu: {e}"))?;
            svc.set_description(DESCRIPTION)
                .map_err(|e| format!("nelze nastavit popis: {e}"))?;
        }
    }

    // Pád služby nesmí znamenat konec sběru — tři pokusy o restart.
    // Actions API windows-service je pro tenhle případ zbytečně
    // upovídané; sc.exe tu nemá co pokazit (argumenty bez mezer).
    // `.output()` místo `.status()`: sc.exe jinak vypíše své
    // „[SC] ChangeServiceConfig2 SUCCESS" doprostřed hlášení instalátoru.
    let _ = std::process::Command::new("sc.exe")
        .args([
            "failure",
            NAME,
            "reset=",
            "86400",
            "actions=",
            "restart/5000/restart/10000/restart/30000",
        ])
        .output();
    Ok(())
}

/// Spustí službu a počká, až naběhne.
pub fn start_and_wait() -> Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    let svc = mgr
        .open_service(NAME, ServiceAccess::START | ServiceAccess::QUERY_STATUS)
        .map_err(|e| format!("služba není zaregistrovaná: {e}"))?;

    let running = matches!(svc.query_status(), Ok(s) if s.current_state == ServiceState::Running);
    if !running {
        svc.start(&[] as &[&str])
            .map_err(|e| format!("službu nelze spustit: {e}"))?;
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(300));
        if let Ok(s) = svc.query_status() {
            if s.current_state == ServiceState::Running {
                return Ok(());
            }
        }
    }
    Err("služba se nerozeběhla do 30 s".into())
}

/// Odregistruje službu.
pub fn uninstall() -> Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    let svc = match mgr.open_service(NAME, ServiceAccess::DELETE) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    svc.delete()
        .map_err(|e| format!("nelze odebrat službu: {e}"))
}
