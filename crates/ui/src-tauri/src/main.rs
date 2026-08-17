//! syswatch-ui — Tauri host. v0: prázdné okno, které se přes named
//! pipe ptá služby „žiješ?“ a ukazuje indikátor stavu démona.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod display;
mod repair;
mod uninstall;
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

/// Připojené obrazovky (v9). Čte je UI, ne služba — EnumDisplayDevices
/// odpovídá za relaci volajícího a služba běží v session 0 bez plochy.
#[tauri::command(async)]
fn query_displays() -> Vec<core_types::proc::DisplayRow> {
    display::displays()
}

/// Stav skenu inventáře — UI podle něj pozná, kdy má načíst nový seznam.
#[derive(Debug, Serialize)]
struct InvStatusDto {
    scanning: bool,
    last_scan_ts: i64,
}

#[tauri::command(async)]
fn query_inv_status() -> Result<InvStatusDto, String> {
    ipc::client::query_inv_status()
        .map(|(scanning, last_scan_ts)| InvStatusDto {
            scanning,
            last_scan_ts,
        })
        .map_err(|e| e.to_string())
}

/// Hardwarový přehled (v9, SPEC kap. 15) — deska, BIOS, baterie,
/// teploty CPU se zdrojem, zdraví disků.
#[tauri::command(async)]
fn query_hardware() -> Result<core_types::proc::HardwareReport, String> {
    ipc::client::query_hardware().map_err(|e| e.to_string())
}

/// Security (v9) — stav ochrany + oprávnění aplikací.
#[tauri::command(async)]
fn query_security() -> Result<core_types::proc::SecurityReport, String> {
    ipc::client::query_security().map_err(|e| e.to_string())
}

/// Users (v9E) — účty a kdo z nich je správce.
///
/// Přihlášeného uživatele doplňuje UI, ne služba: ta běží jako SYSTEM
/// v session 0 a o relaci přihlášeného člověka nic neví. Zeptat se na
/// to odsud je jediné místo, kde odpověď platí.
#[tauri::command(async)]
fn query_users() -> Result<core_types::proc::UsersReport, String> {
    let mut r = ipc::client::query_users().map_err(|e| e.to_string())?;
    r.current_user = current_user_name();
    Ok(r)
}

/// Jméno účtu, pod kterým běží tenhle proces.
fn current_user_name() -> String {
    use windows::Win32::System::WindowsProgramming::GetUserNameW;
    let mut buf = [0u16; 257];
    let mut len = buf.len() as u32;
    // SAFETY: buffer má hlášenou velikost; při chybě zůstane prázdný.
    unsafe {
        if GetUserNameW(Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len).is_err() {
            return String::new();
        }
    }
    // Délka zahrnuje ukončovací nulu.
    String::from_utf16_lossy(&buf[..len.saturating_sub(1) as usize])
}

/// Stav připojení (v9) — adaptéry, IP konfigurace, WiFi.
#[tauri::command(async)]
fn query_connection() -> Result<core_types::proc::ConnectionReport, String> {
    ipc::client::query_connection().map_err(|e| e.to_string())
}

/// Spojení per aplikace (v9, SPEC kap. 12) — kdo je připojený kam.
#[tauri::command(async)]
fn query_network() -> Result<Vec<core_types::proc::AppNetRow>, String> {
    ipc::client::query_network().map_err(|e| e.to_string())
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
    indexing: Vec<(char, u64, bool, Option<String>)>,
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

/// T1 plán ukončení procesu (v7) — vrací kroky k potvrzení, nebo deny.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum PlanOrDeny {
    Plan(core_types::action::ActionPlan),
    Deny(core_types::action::ActionResult),
}

#[tauri::command(async)]
fn plan_kill(pid: u32, create_time: i64, tree: bool) -> Result<PlanOrDeny, String> {
    ipc::client::plan_action(core_types::action::Action::KillProc {
        pid,
        create_time,
        tree,
    })
    .map(|r| match r {
        Ok(p) => PlanOrDeny::Plan(p),
        Err(d) => PlanOrDeny::Deny(d),
    })
    .map_err(|e| e.to_string())
}

/// Provedení potvrzeného plánu (v5/v7).
#[tauri::command(async)]
fn execute_plan(plan_id: u64) -> Result<core_types::action::ActionResult, String> {
    ipc::client::execute_action(plan_id).map_err(|e| e.to_string())
}

/// Kdo drží soubory (v8) — „proč to nejde smazat".
#[tauri::command(async)]
fn query_holders(paths: Vec<String>) -> Result<Vec<core_types::proc::HolderRow>, String> {
    ipc::client::query_holders(paths).map_err(|e| e.to_string())
}

/// T1 plán smazání do koše (v8) — vrací kroky k potvrzení, nebo deny.
#[tauri::command(async)]
fn plan_delete(paths: Vec<String>) -> Result<PlanOrDeny, String> {
    ipc::client::plan_action(core_types::action::Action::DeleteFiles { paths })
        .map(|r| match r {
            Ok(p) => PlanOrDeny::Plan(p),
            Err(d) => PlanOrDeny::Deny(d),
        })
        .map_err(|e| e.to_string())
}

/// T1 plán odinstalace (v8) — vrací kroky k potvrzení, nebo deny.
#[tauri::command(async)]
fn plan_uninstall(identity_key: String) -> Result<PlanOrDeny, String> {
    ipc::client::plan_action(core_types::action::Action::UninstallApp { identity_key })
        .map(|r| match r {
            Ok(p) => PlanOrDeny::Plan(p),
            Err(d) => PlanOrDeny::Deny(d),
        })
        .map_err(|e| e.to_string())
}

/// Co po aplikaci zbylo na disku (v8).
#[tauri::command(async)]
fn query_leftovers(identity_key: String) -> Result<Vec<String>, String> {
    ipc::client::query_leftovers(identity_key).map_err(|e| e.to_string())
}

/// Spuštěná odinstalace — co UI potřebuje k dalším dvěma krokům.
#[derive(Debug, Serialize)]
struct UninstallStarted {
    /// Zamítnutí vrstvou; když je vyplněné, nic se nespustilo.
    deny_reason: Option<String>,
    audit_id: i64,
    /// Jméno spuštěné binárky — podle ní se pozná, že ještě běží.
    exe_name: String,
    /// Cesty aplikace zachycené PŘED odinstalací. Inventář je po ní
    /// ze své databáze smaže, takže je musíme mít stranou.
    paths: Vec<String>,
}

/// Odinstalace, krok 1 — SPUSTIT. Vrací se hned, ať UI může ukázat,
/// že odinstalátor běží; čekání řeší `uninstall_running`.
///
/// Služba plán znovu zvaliduje a vydá příkaz, spouštíme ho ale **tady**,
/// v relaci uživatele — služba běží jako SYSTEM v session 0, kde by
/// odinstalátor neměl viditelnou plochu ani správný `HKEY_CURRENT_USER`.
#[tauri::command(async)]
fn start_uninstall(plan_id: u64, identity_key: String) -> Result<UninstallStarted, String> {
    // Cesty aplikace ještě než do nich odinstalátor sáhne.
    let paths: Vec<String> = ipc::client::query_app_map(identity_key)
        .map(|rows| rows.into_iter().map(|p| p.path).collect())
        .unwrap_or_default();

    let (command, audit_id) = match ipc::client::authorize_uninstall(plan_id) {
        Ok(Ok(pair)) => pair,
        // Zamítnutí není chyba volání — vracíme ho UI k zobrazení.
        Ok(Err(deny)) => {
            return Ok(UninstallStarted {
                deny_reason: Some(deny.deny_reason.unwrap_or_else(|| "zamítnuto".into())),
                audit_id: deny.audit_id,
                exe_name: String::new(),
                paths,
            })
        }
        Err(e) => return Err(e.to_string()),
    };
    match uninstall::launch(&command) {
        Ok(exe_name) => Ok(UninstallStarted {
            deny_reason: None,
            audit_id,
            exe_name,
            paths,
        }),
        // Neúspěšný start se hlásí hned, ať audit nezůstane „running".
        Err(e) => {
            let _ = ipc::client::report_uninstall(audit_id, String::new(), e.to_string());
            Err(e.to_string())
        }
    }
}

/// Zvedne zastavenou službu — pustí instalátor v opravném režimu.
/// Výzvu UAC zobrazí Windows, potvrzuje ji uživatel.
#[tauri::command(async)]
fn repair_service() -> Result<(), String> {
    repair::launch().map_err(|e| e.to_string())
}

/// Odinstalace, krok 2 — BĚŽÍ JEŠTĚ? UI se ptá po sekundách a mezitím
/// ukazuje, co se děje.
#[tauri::command(async)]
fn uninstall_running(exe_name: String) -> bool {
    uninstall::still_running(&exe_name)
}

/// Výsledek odinstalace pro UI.
#[derive(Debug, Serialize)]
struct UninstallDone {
    /// Je aplikace pořád v registru? (Odinstalátor šlo zavřít.)
    still_installed: bool,
    /// Cesty aplikace, které na disku zůstaly.
    leftovers: Vec<String>,
}

/// Odinstalace, krok 3 — DOKONČIT: projít cesty aplikace, ověřit registr,
/// doplnit audit a vyžádat nový sken inventáře.
#[tauri::command(async)]
fn finish_uninstall(
    audit_id: i64,
    identity_key: String,
    paths: Vec<String>,
) -> Result<UninstallDone, String> {
    uninstall::close_running();
    let leftovers = uninstall::remaining(&paths);
    let detail = format!(
        "po odinstalaci zbylo {} z {} cest",
        leftovers.len(),
        paths.len()
    );
    // Registr rozhoduje o tom, zda aplikace zmizela — ne odinstalátor.
    let res =
        ipc::client::report_uninstall(audit_id, identity_key, detail).map_err(|e| e.to_string())?;
    // Inventář ještě drží starý stav — nový sken ho srovná.
    let _ = ipc::client::rescan_apps();
    Ok(UninstallDone {
        still_installed: res.outcome.as_deref() != Some("ok"),
        leftovers,
    })
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
        // MUSÍ být první plugin (vyžaduje dokumentace pluginu).
        //
        // Bez něj vyrobí každé spuštění další proces s vlastní ikonou
        // v oznamovací oblasti — a protože se okno zavřením jen schová,
        // nasčítaly se ikony jedna za druhou. Teď druhá instance jen
        // ukáže okno té běžící a sama hned skončí: první spuštění
        // rozjede aplikaci, každé další je „otevři okno".
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
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
            query_inv_status,
            open_path,
            query_volumes,
            query_hardware,
            query_displays,
            query_network,
            query_connection,
            query_security,
            query_users,
            build_file_index,
            search_files,
            find_duplicates,
            query_cleanup,
            delete_incident,
            query_startup,
            toggle_startup,
            query_audit,
            plan_kill,
            plan_delete,
            plan_uninstall,
            start_uninstall,
            uninstall_running,
            finish_uninstall,
            query_leftovers,
            query_holders,
            repair_service,
            execute_plan
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
