//! syswatch-ui — Tauri host. v0: prázdné okno, které se přes named
//! pipe ptá služby „žiješ?“ a ukazuje indikátor stavu démona.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;

/// Odpověď pro frontend na ping služby.
#[derive(Debug, Serialize)]
struct PingResult {
    protocol_version: u32,
    uptime_s: u64,
}

/// Zeptá se démona přes pipe. Chyba (služba neběží, vadný rámec…) se
/// vrací frontendu jako string — indikátor zčervená a detail se ukáže.
#[tauri::command]
fn ping_daemon() -> Result<PingResult, String> {
    match ipc::client::ping() {
        Ok(pong) => Ok(PingResult {
            protocol_version: pong.protocol_version,
            uptime_s: pong.uptime_s,
        }),
        Err(ipc::Error::NotAvailable) => Err("služba neběží (pipe nedostupná)".into()),
        Err(e) => Err(format!("chyba komunikace se službou: {e}")),
    }
}

/// Snapshot procesů pro frontend. Serializace typů z core-types
/// projde přímo (derive Serialize).
#[tauri::command]
fn query_procs() -> Result<Vec<core_types::proc::ProcRow>, String> {
    ipc::client::query_procs().map_err(|e| e.to_string())
}

/// Systémové metriky pro hlavní graf v Tasks.
#[tauri::command]
fn query_system() -> Result<core_types::proc::SystemSnapshot, String> {
    ipc::client::query_system().map_err(|e| e.to_string())
}

/// Historie systémových metrik pro pan/zoom grafu do minulosti.
#[tauri::command]
fn query_system_history(from: i64, to: i64) -> Result<Vec<core_types::proc::SystemPoint>, String> {
    ipc::client::query_system_history(from, to).map_err(|e| e.to_string())
}

/// Stav procesů v čase pod kurzorem/zámkem grafu.
#[derive(Debug, Serialize)]
struct ProcsAtDto {
    ts: i64,
    rows: Vec<core_types::proc::HistProcRow>,
}

#[tauri::command]
fn query_procs_at(ts: i64) -> Result<ProcsAtDto, String> {
    ipc::client::query_procs_at(ts)
        .map(|(ts, rows)| ProcsAtDto { ts, rows })
        .map_err(|e| e.to_string())
}

/// Statické informace o komponentách (CPU/RAM/GPU/disky).
#[tauri::command]
fn query_sys_info() -> Result<core_types::proc::StaticInfo, String> {
    ipc::client::query_sys_info().map_err(|e| e.to_string())
}

/// Detaily proměnných v čase pro zámek grafu.
#[derive(Debug, Serialize)]
struct DetailAtDto {
    ts: i64,
    cores: Vec<f32>,
    disks: Vec<core_types::proc::DiskRate>,
    gpu: Option<core_types::proc::GpuInfo>,
}

#[tauri::command]
fn query_detail_at(ts: i64) -> Result<DetailAtDto, String> {
    ipc::client::query_detail_at(ts)
        .map(|(ts, cores, disks, gpu)| DetailAtDto {
            ts,
            cores,
            disks,
            gpu,
        })
        .map_err(|e| e.to_string())
}

/// Historie disků pro per-disk grafy.
#[tauri::command]
fn query_disk_history(from: i64, to: i64) -> Result<Vec<(i64, u32, u64, u64)>, String> {
    ipc::client::query_disk_history(from, to).map_err(|e| e.to_string())
}

/// Historie jader CPU pro mini grafy při zámku času.
#[tauri::command]
fn query_core_history(from: i64, to: i64) -> Result<Vec<(i64, u32, f32)>, String> {
    ipc::client::query_core_history(from, to).map_err(|e| e.to_string())
}

/// Ikona aplikace podle identity_key (RGBA pixely; UI je vykreslí na canvas).
#[tauri::command]
fn query_icon(identity_key: String) -> Result<Option<core_types::proc::IconData>, String> {
    ipc::client::query_icon(identity_key).map_err(|e| e.to_string())
}

/// Události (záseky, pády) v rozsahu — markery na časové ose (v3).
#[tauri::command]
fn query_events(from: i64, to: i64) -> Result<Vec<core_types::proc::EventRow>, String> {
    ipc::client::query_events(from, to).map_err(|e| e.to_string())
}

/// Poslední incidenty (v3, SPEC kap. 16).
#[tauri::command]
fn query_incidents(limit: u32) -> Result<Vec<core_types::proc::IncidentRow>, String> {
    ipc::client::query_incidents(limit).map_err(|e| e.to_string())
}

/// Inventář aplikací (v4, SPEC kap. 5).
#[tauri::command]
fn query_apps() -> Result<Vec<core_types::proc::AppRow>, String> {
    ipc::client::query_apps().map_err(|e| e.to_string())
}

/// Mapa souborů aplikace.
#[tauri::command]
fn query_app_map(identity_key: String) -> Result<Vec<core_types::proc::AppPathRow>, String> {
    ipc::client::query_app_map(identity_key).map_err(|e| e.to_string())
}

/// Spočítá velikosti cest (pomalé — async přes spawn_blocking Tauri).
#[tauri::command(async)]
fn compute_app_sizes(identity_key: String) -> Result<Vec<core_types::proc::AppPathRow>, String> {
    ipc::client::compute_app_sizes(identity_key).map_err(|e| e.to_string())
}

/// Vyžádá nový sken inventáře.
#[tauri::command]
fn rescan_apps() -> Result<(), String> {
    ipc::client::rescan_apps().map_err(|e| e.to_string())
}

/// Svazky + zdraví disků (v4C).
#[derive(Debug, Serialize)]
struct VolumesDto {
    volumes: Vec<core_types::proc::VolumeRow>,
    health: Vec<core_types::proc::DiskHealthRow>,
}

#[tauri::command]
fn query_volumes() -> Result<VolumesDto, String> {
    ipc::client::query_volumes()
        .map(|(volumes, health)| VolumesDto { volumes, health })
        .map_err(|e| e.to_string())
}

/// Postaví MFT index svazku (sekundy — async, ať UI nezamrzne).
#[tauri::command(async)]
fn build_file_index(letter: char) -> Result<u64, String> {
    ipc::client::build_file_index(letter).map_err(|e| e.to_string())
}

/// Hledání v MFT indexu.
#[tauri::command(async)]
fn search_files(
    letter: char,
    query: String,
    limit: u32,
) -> Result<Vec<core_types::proc::FileHit>, String> {
    ipc::client::search_files(letter, query, limit).map_err(|e| e.to_string())
}

/// Stav auto-úklidu (v4E).
#[derive(Debug, Serialize)]
struct CleanupDto {
    indexing: Vec<(char, u64, bool)>,
    running: bool,
    report: Option<core_types::proc::CleanupReport>,
}

#[tauri::command]
fn query_cleanup() -> Result<CleanupDto, String> {
    ipc::client::query_cleanup()
        .map(|(indexing, running, report)| CleanupDto {
            indexing,
            running,
            report,
        })
        .map_err(|e| e.to_string())
}

/// Startup položky (v6, SPEC kap. 7).
#[tauri::command(async)]
fn query_startup() -> Result<Vec<core_types::proc::StartupRow>, String> {
    ipc::client::query_startup().map_err(|e| e.to_string())
}

/// Přepnutí startup položky — T0 přes validační vrstvu (v6).
#[tauri::command(async)]
fn toggle_startup(id: String, on: bool) -> Result<core_types::action::ActionResult, String> {
    ipc::client::toggle_action(core_types::action::Action::StartupToggle { id, on })
        .map_err(|e| e.to_string())
}

/// Auditní záznamy (v5) — historie zásahů do systému.
#[tauri::command]
fn query_audit(limit: u32) -> Result<Vec<core_types::action::AuditRow>, String> {
    ipc::client::query_audit(limit).map_err(|e| e.to_string())
}

/// Smaže záznam incidentu (vlastní DB záznam).
#[tauri::command]
fn delete_incident(id: i64) -> Result<(), String> {
    ipc::client::delete_incident(id).map_err(|e| e.to_string())
}

/// Duplicity (v4D) — pomalé, async command.
#[tauri::command(async)]
fn find_duplicates(root: String, min_size: u64) -> Result<Vec<(u64, Vec<String>)>, String> {
    ipc::client::find_duplicates(root, min_size).map_err(|e| e.to_string())
}

/// Otevře cestu v Průzkumníku (adresář přímo, soubor s /select).
/// Jen otevření — žádná mutace; registry cesty sem nepatří.
#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    if path.starts_with("HKLM") || path.starts_with("HKU") || path.starts_with("HKCU") {
        return Err("registry větev nejde otevřít v Průzkumníku".into());
    }
    let p = std::path::Path::new(&path);
    let mut cmd = std::process::Command::new("explorer.exe");
    if p.is_dir() {
        cmd.arg(&path);
    } else {
        cmd.arg(format!("/select,{path}"));
    }
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Vlastní spotřeba nástroje pro dlaždici v Settings (SPEC kap. 2.3).
#[derive(Debug, Serialize)]
struct SelfUsageDto {
    cpu_pct: f32,
    ws_bytes: u64,
    db_bytes: u64,
}

#[tauri::command]
fn query_self_usage() -> Result<SelfUsageDto, String> {
    ipc::client::query_self_usage()
        .map(|u| SelfUsageDto {
            cpu_pct: u.cpu_pct,
            ws_bytes: u.ws_bytes,
            db_bytes: u.db_bytes,
        })
        .map_err(|e| e.to_string())
}

/// Ukáže a vyzdvihne hlavní okno.
fn show_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Tray ikona v oznamovací oblasti Windows. UI proces žije v tray i po
/// zavření okna (zavřít = schovat) — monitoring (démon) běží dál a
/// ikona je vidět; „Ukončit“ v menu zavře jen UI, ne službu.
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "Otevřít Winsent", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Ukončit UI", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("winsent")
        .icon(app.default_window_icon().expect("chybí ikona okna").clone())
        .tooltip("Winsent — systémový monitor")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Levý klik = otevřít okno (pravý nechává kontextové menu).
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ping_daemon,
            query_procs,
            query_system,
            query_self_usage,
            query_system_history,
            query_procs_at,
            query_sys_info,
            query_detail_at,
            query_disk_history,
            query_core_history,
            query_icon,
            query_events,
            query_incidents,
            query_apps,
            query_app_map,
            compute_app_sizes,
            rescan_apps,
            open_path,
            query_volumes,
            build_file_index,
            search_files,
            find_duplicates,
            query_cleanup,
            delete_incident,
            query_startup,
            toggle_startup,
            query_audit
        ])
        .setup(|app| {
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Zavření okna = schovat do tray, UI běží dál (ikona zůstává).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("start Tauri selhal");
}
