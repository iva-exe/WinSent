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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ping_daemon,
            query_procs,
            query_system,
            query_self_usage
        ])
        .run(tauri::generate_context!())
        .expect("start Tauri selhal");
}
