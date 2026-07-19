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
            query_icon
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
