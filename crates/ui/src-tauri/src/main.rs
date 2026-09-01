//! syswatch-ui — Tauri host. v0: prázdné okno, které se přes named
//! pipe ptá služby „žiješ?“ a ukazuje indikátor stavu démona.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod display;
mod hotkey;
mod launch;
mod repair;
mod spotlight;
mod uninstall;
use serde::Serialize;

/// Sekce, kterou zkratka vyvolává. Spotlight modul umí libovolnou
/// cestu — až přibude druhá, přidá se sem výběr.
const SPOTLIGHT_ROUTE: &str = "spotlight";

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



/// Součty použití všech oprávnění za období (v9D) — jeden dotaz.
#[tauri::command(async)]
fn query_perm_use_totals(days: u32) -> Result<Vec<(String, String, i64)>, String> {
    ipc::client::query_perm_use_totals(days).map_err(|e| e.to_string())
}

/// Historie použití oprávnění (v9D) — sezení a součet za období.
#[derive(Debug, Serialize)]
struct PermUseDto {
    sessions: Vec<core_types::proc::PermUseRow>,
    total_s: i64,
}

#[tauri::command(async)]
fn query_perm_use(app: String, capability: String, days: u32) -> Result<PermUseDto, String> {
    ipc::client::query_perm_use(app, capability, days)
        .map(|(sessions, total_s)| PermUseDto { sessions, total_s })
        .map_err(|e| e.to_string())
}




/// Uloží textový záznam (incident nebo celý počítač) a vrátí cestu.
///
/// Píše se tudy, ne stažením přes prohlížeč: jen tak víme, KAM to
/// spadlo, a můžeme pak otevřít složku. Blob download v Tauri cestu
/// nevrací, takže by se uživateli řeklo „uloženo" a on by pak soubor
/// hledal.
#[tauri::command(async)]
fn save_report(name: String, text: String) -> Result<String, String> {
    // Jméno souboru skládá UI, ale ověřuje se tady: cesta v něm nemá
    // co dělat a přepsat něco mimo Stažené soubory už vůbec ne.
    if name.contains([char::from(92u8), '/', ':']) || name.contains("..") {
        return Err("neplatné jméno souboru".into());
    }
    let base = std::env::var("USERPROFILE")
        .map(|p| std::path::PathBuf::from(p).join("Downloads"))
        .unwrap_or_else(|_| std::env::temp_dir());
    // Když složka Stažené soubory neexistuje (přesunutá knihovna),
    // spadne se do dočasné složky, ať se záznam neztratí.
    let dir = if base.is_dir() { base } else { std::env::temp_dir() };
    let path = dir.join(&name);
    std::fs::write(&path, text).map_err(|e| format!("nelze zapsat {}: {e}", path.display()))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Hlášení o pádech z Windows, přeložená do lidské řeči.
#[tauri::command(async)]
fn query_crash_reports(limit: u32) -> Result<Vec<core_types::proc::CrashReportRow>, String> {
    ipc::client::query_crash_reports(limit).map_err(|e| e.to_string())
}


/// Výpisy paměti k incidentu — čte je služba, protože do
/// C:\Windows\Minidump a do cizích profilů uživatel nevidí.
#[tauri::command(async)]
fn query_incident_dumps(app: String, ts: i64, dumpPath: String) -> Result<String, String> {
    ipc::client::query_incident_dumps(app, ts, dumpPath).map_err(|e| e.to_string())
}

/// Stav sběru — proč je tabulka prázdná.
#[tauri::command(async)]
fn query_collector_health() -> Result<core_types::proc::CollectorHealth, String> {
    ipc::client::query_collector_health().map_err(|e| e.to_string())
}

/// Ovladače (v10) — co v počítači běží, od koho a jak staré.
#[tauri::command(async)]
fn query_drivers() -> Result<core_types::proc::DriversReport, String> {
    ipc::client::query_drivers().map_err(|e| e.to_string())
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

/// Čas vzniku procesu chodí z UI jako ŘETĚZEC.
///
/// FILETIME je dnes zhruba 1,34 × 10¹⁷ a JavaScript umí přesně jen celá
/// čísla do 2⁵³. Kdyby se posílalo jako číslo, vrátí se zaokrouhlené,
/// validační vrstva ho neuzná za tentýž proces a ukončení odmítne
/// s hláškou o recyklovaném PID — což se taky dělo skoro pokaždé.
#[tauri::command(async)]
fn plan_kill(pid: u32, create_time: String, tree: bool) -> Result<PlanOrDeny, String> {
    let create_time: i64 = create_time
        .parse()
        .map_err(|_| format!("neplatný čas vzniku procesu: {create_time:?}"))?;
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


/// T1 plán úklidu záznamu po programu, který na disku není (v10).
#[tauri::command(async)]
fn plan_purge_ghost(identity_key: String) -> Result<PlanOrDeny, String> {
    ipc::client::plan_action(core_types::action::Action::PurgeGhost { identity_key })
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
    /// Odinstalaci převzal launcher (Steam a spol.) a čeká na potvrzení
    /// ve svém okně. Není to selhání — jen ještě není hotovo.
    handed_off: bool,
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
        handed_off: res.outcome.as_deref() == Some("handed"),
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
/// Cesta do uvozovek pro explorer.exe. Koncová zpětná lomítka se musí
/// zdvojit: `"C:\"` přečte Windows jako escapovanou uvozovku, takže
/// z argumentu zbyde `C:"` a Průzkumník otevře výchozí složku. Týká se
/// to kořene svazku, který mezi největšími složkami klidně být může.
fn quoted(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    let tail = s.len() - s.trim_end_matches(char::from(92u8)).len();
    format!("\"{s}{}\"", String::from_utf8(vec![92u8; tail]).unwrap_or_default())
}

/// Otevře cestu v Průzkumníku (adresář přímo, soubor s /select).
/// Jen otevření — žádná mutace; registry cesty sem nepatří.
#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    if path.starts_with("HKLM") || path.starts_with("HKU") || path.starts_with("HKCU") {
        return Err("registry větev nejde otevřít v Průzkumníku".into());
    }

    let p = std::path::Path::new(&path);

    // Cesta mohla mezitím zmizet (uživatel ji smazal z Průzkumníku).
    // Ok(false) znamená „opravdu tam není"; Err znamená jen „nešlo to
    // zjistit" (třeba práva) — tam se o krok výš nechodí, jinak by se
    // u chráněných souborů otevírala nadřazená složka zbytečně.
    let target = if matches!(p.try_exists(), Ok(false)) {
        match p.ancestors().find(|a| matches!(a.try_exists(), Ok(true))) {
            Some(a) => a.to_path_buf(),
            None => return Err("cesta ani žádná nadřazená složka už neexistuje".into()),
        }
    } else {
        p.to_path_buf()
    };

    // Explorer.exe má vlastní parser příkazové řádky a chce přesně
    // tvar `/select,"cesta"` — uvozovky JEN kolem cesty. Command::arg
    // ale uvozuje celý token, takže u cesty s mezerou vznikne
    // `"/select,C:\Program Files\…"`, což explorer nepřečte a místo
    // souboru otevře výchozí složku. Proto se řádka skládá ručně přes
    // raw_arg. (Uvozovka v cestě na Windows být nemůže, escapovat
    // není co.)
    let mut cmd = std::process::Command::new("explorer.exe");
    if target.is_dir() {
        cmd.raw_arg(quoted(&target));
    } else {
        cmd.raw_arg(format!("/select,{}", quoted(&target)));
    }
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Cesta bezpečná i nad MAX_PATH. Bez prefixu vrací Windows u dlouhých
/// cest „path not found" — a to by řádek nesprávně schovalo jako
/// smazaný. Cesty z MFT jsou kanonické (`X:\…`, bez `.` a `..`), takže
/// prefix nic nerozbije.
fn long_path(p: &str) -> std::path::PathBuf {
    if p.len() > 250 && p.as_bytes().get(1) == Some(&b':') && !p.starts_with(r"\\") {
        std::path::PathBuf::from(format!(r"\?\{p}"))
    } else {
        std::path::PathBuf::from(p)
    }
}

/// Které z cest na disku ještě jsou (jen čtení).
///
/// Úklidový report spočte služba jednou po startu a dál ho drží
/// v paměti — smazaný soubor v něm visí dál, ať ho uživatel smazal
/// v aplikaci, nebo v Průzkumníku. Než UI seznam vykreslí, zeptá se
/// proto souborového systému samo. Na službu se tu nesahá, je to
/// `std::fs` v procesu UI.
///
/// Za „chybí" se počítá VÝHRADNĚ `NotFound`. Report staví služba pod
/// SYSTEM, UI běží pod uživatelem — na cestu, kam UI nedosáhne, přijde
/// `PermissionDenied`, a tu v seznamu NECHÁVÁME. Schovat něco, o čem
/// nic nevíme, by lhalo víc než to ukázat.
///
/// Návratem není `Result`: chyba u jedné cesty JE odpovědí o ní, ne
/// selháním celého dotazu. Délka výstupu vždy odpovídá délce vstupu.
#[tauri::command(async)]
fn paths_exist(paths: Vec<String>) -> Vec<bool> {
    // Pojistka proti utrženému seznamu. Co je nad limit, hlásíme jako
    // existující — nikdy neschováme víc, než jsme opravdu ověřili.
    const MAX: usize = 2_000;
    paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i >= MAX {
                return true;
            }
            // symlink_metadata nenásleduje reparse pointy: zajímá nás
            // sama položka v adresáři, ne cíl odkazu.
            match std::fs::symlink_metadata(long_path(p)) {
                Ok(_) => true,
                Err(e) => e.kind() != std::io::ErrorKind::NotFound,
            }
        })
        .collect()
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

/// Vyhledá text ve výchozím prohlížeči uživatele.
///
/// Slouží položce „Co to je?" v kontextovém menu: uživatel klikne pravým
/// na „NVIDIA GeForce RTX 3070" a Windows otevřou jeho vyhledávač.
/// Nic se nestahuje ani neinstaluje — jen se předá URL systému.
///
/// Dotaz se skládá TADY, ne v UI: do `ShellExecuteW` nesmí přijít nic
/// jiného než https URL na známý vyhledávač. Kdyby URL sestavoval
/// frontend, dalo by se přes tenhle příkaz otevřít cokoliv.
#[tauri::command(async)]
fn search_web(query: String) -> Result<(), String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("prázdný dotaz".into());
    }
    // Delší text než tohle není hledání, ale omyl (vlepená cesta,
    // celý řádek logu). Ořízne se, ať se do prohlížeče nepošle román.
    let q: String = q.chars().take(200).collect();
    let url = format!("https://duckduckgo.com/?q={}", url_encode(&q));

    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let open = HSTRING::from("open");
    let file = HSTRING::from(url.as_str());
    // SAFETY: oba řetězce žijí přes celé volání; ShellExecuteW vrací
    // pseudohandle, který se nezavírá.
    let rc = unsafe { ShellExecuteW(None, &open, &file, None, None, SW_SHOWNORMAL) };
    if rc.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("prohlížeč se nepodařilo otevřít (kód {})", rc.0 as usize))
    }
}

/// Otevře stránku Nastavení Windows.
///
/// Winsent oprávnění ani systémová nastavení nepřepíná — jen doveze
/// uživatele na místo, kde to udělá sám (SPEC 13.4: my ukazujeme,
/// spoušť mačká on). Přijímá se jen známý seznam stránek, aby se přes
/// tenhle příkaz nedalo spustit libovolné URI schéma.
#[tauri::command(async)]
fn open_settings_page(page: String) -> Result<(), String> {
    let uri = match page.as_str() {
        "privacy-webcam" => "ms-settings:privacy-webcam",
        "privacy-microphone" => "ms-settings:privacy-microphone",
        "privacy-location" => "ms-settings:privacy-location",
        "privacy-general" => "ms-settings:privacy",
        "windowsupdate" => "ms-settings:windowsupdate",
        "windowsdefender" => "windowsdefender:",
        "startupapps" => "ms-settings:startupapps",
        "appsfeatures" => "ms-settings:appsfeatures",
        "network" => "ms-settings:network-status",
        "otherusers" => "ms-settings:otherusers",
        // Neznámá schopnost → obecné soukromí. Lepší než nic neudělat.
        p if p.starts_with("privacy-") => "ms-settings:privacy",
        _ => return Err(format!("neznámá stránka nastavení: {page}")),
    };

    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let open = HSTRING::from("open");
    let file = HSTRING::from(uri);
    // SAFETY: oba řetězce žijí přes celé volání.
    let rc = unsafe { ShellExecuteW(None, &open, &file, None, None, SW_SHOWNORMAL) };
    if rc.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("Nastavení se nepodařilo otevřít (kód {})", rc.0 as usize))
    }
}

/// Spustí nainstalovanou aplikaci z vyhledávání.
///
/// Spouští UI proces, protože běží v relaci uživatele — služba je
/// v session 0 a okno by nemělo kam vykreslit.
#[tauri::command(async)]
fn launch_app(
    identity_key: String,
    display_name: String,
    aumid: Option<String>,
) -> Result<String, String> {
    launch::launch(&identity_key, &display_name, aumid.as_deref())
}

/// Spouští se Winsent po přihlášení uživatele?
///
/// Čte se ze systému, ne z uloženého nastavení: uživatel může položku
/// vypnout ve Správci úloh a přepínač v Nastavení by pak tvrdil něco
/// jiného, než co se doopravdy děje.
#[tauri::command(async)]
fn query_autostart() -> bool {
    autostart::zapnuto()
}

/// Zapne nebo vypne spouštění po přihlášení. Vrací stav, jaký po
/// změně opravdu platí — ne ten, o který se žádalo.
#[tauri::command(async)]
fn set_autostart(enabled: bool) -> Result<bool, String> {
    autostart::nastav(enabled)?;
    Ok(autostart::zapnuto())
}

/// Jedna spustitelná položka složky „Aplikace".
#[derive(Debug, Serialize)]
struct LaunchableRow {
    /// Jméno tak, jak ho ukazuje nabídka Start — lokalizované.
    name: String,
    /// AUMID nebo cesta; předává se rovnou do `launch_app`.
    aumid: String,
    /// `msix:{family}` u balíčkových položek — párování s inventářem,
    /// ať se u nich neztratí velikosti a mapa souborů.
    identity_key: Option<String>,
    /// Cíl na disku neexistuje.
    missing: bool,
    /// Je to systémový nástroj Windows (Poznámkový blok, Ovládací
    /// panely…)? Rozhoduje cesta, resp. shell GUID.
    system: bool,
}

/// `msix:{family}` pro balíčkovou položku, jinak nic.
///
/// Podmínka je schválně tvrdá. AUMID balíčku je `{family}!{appid}`
/// a family končí třináctiznakovým identifikátorem vydavatele; bez
/// téhle kontroly by cesta `C:\…\osu!\osu!.exe` prošla jako balíček
/// „msix:C:\…" a slepila dvě různé aplikace do jedné.
fn msix_klic(aumid: &str) -> Option<String> {
    let (family, _) = aumid.split_once('!')?;
    if family.contains('\\') || family.contains('/') {
        return None;
    }
    let (_, publisher) = family.rsplit_once('_')?;
    (publisher.len() == 13 && publisher.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| format!("msix:{family}"))
}

/// Co jde na tomhle stroji spustit.
///
/// Enumeruje hostitel, ne služba, a to ze dvou důvodů: služba běží
/// pod SYSTEM, takže se v její relaci nedají přečíst ani lokalizovaná
/// jména balíčků („Microsoft.WindowsCalculator" místo „Kalkulačka"),
/// ani větev registru přihlášeného uživatele. Inventář služby je navíc
/// seznam INSTALÁTORŮ — nejsou v něm vestavěné nástroje Windows,
/// přenosné programy ani hry bez odinstalačního záznamu. Naměřeno:
/// ze 260 spustitelných položek jich inventář neznal 178.
#[tauri::command(async)]
fn query_launchables() -> Vec<LaunchableRow> {
    launch::seznam()
        .into_iter()
        // Odkaz na webovou stránku není program — stejné pravidlo,
        // jaké platí pro spouštění.
        .filter(|p| !p.aumid.contains("://"))
        .map(|p| {
            let lc = p.aumid.to_ascii_lowercase();
            LaunchableRow {
                identity_key: msix_klic(&p.aumid),
                // Položka shellu (Ovládací panely, Plánovač úloh) je
                // GUID; nástroje ve Windows leží pod C:\Windows.
                system: lc.starts_with('{') || lc.contains("\\windows\\"),
                missing: p.chybi,
                name: p.jmeno,
                aumid: p.aumid,
            }
        })
        .collect()
}

/// Ikona spustitelné položky. Volá se až tehdy, když ji služba nemá.
#[tauri::command(async)]
fn query_launchable_icon(aumid: String) -> Option<core_types::proc::IconData> {
    launch::ikona_polozky(&aumid)
}

/// Jaká zkratka vyvolává vyhledávací lištu.
#[tauri::command]
fn get_spotlight_hotkey() -> String {
    hotkey::load()
}

/// Změní zkratku. Projeví se hned, ne až po restartu.
#[tauri::command]
fn set_spotlight_hotkey(accel: String) -> Result<(), String> {
    hotkey::save(&accel)?;
    hotkey::set(&accel);
    Ok(())
}

/// Zvětšení uživatelského rozhraní (1.0 = beze změny).
#[tauri::command(async)]
fn query_ui_zoom() -> f64 {
    hotkey::zvetseni()
}

/// Nastaví zvětšení UI a hned ho použije na všechna okna.
///
/// Přibližuje se celé webview, ne jen písmo: rozvržení míchá rem
/// a pixely, takže samotná změna velikosti textu by ho posunula a
/// rámečky nechala, kde byly.
#[tauri::command(async)]
fn set_ui_zoom(app: tauri::AppHandle, zoom: f64) -> Result<f64, String> {
    hotkey::save_zvetseni(zoom)?;
    let z = hotkey::zvetseni();
    let h = app.clone();
    let _ = app.run_on_main_thread(move || pouzij_zvetseni(&h, z));
    Ok(z)
}

/// Použije zvětšení na hlavní okno i na lištu.
fn pouzij_zvetseni(app: &tauri::AppHandle, zoom: f64) {
    use tauri::Manager;
    for label in ["main", spotlight::LABEL] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_zoom(zoom);
        }
    }
}

/// Je vyhledávací lišta zapnutá?
#[tauri::command(async)]
fn get_spotlight_enabled() -> bool {
    hotkey::zapnuta()
}

/// Zapne nebo vypne vyhledávací lištu.
///
/// Vypnutá znamená, že se odregistruje i klávesová zkratka — jinak by
/// Winsent dál držel Alt+mezerník, který by pak nefungoval ani jemu,
/// ani nikomu jinému.
#[tauri::command(async)]
fn set_spotlight_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    hotkey::save_zapnuta(enabled)?;
    let zapis = if enabled { hotkey::load() } else { String::new() };
    hotkey::set(&zapis);
    if !enabled {
        let h = app.clone();
        let _ = app.run_on_main_thread(move || spotlight::hide(&h));
    }
    Ok(())
}

/// Lišta hlásí, že je vykreslená a chce zaostřit.
///
/// Volá se při každém vyvolání, ale záleží na tom hlavně při prvním:
/// tehdy okno teprve vzniká a zaměření se ztratí dřív, než ho má kdo
/// převzít. Okna se smějí obsluhovat jen z hlavního vlákna.
#[tauri::command(async)]
fn focus_spotlight(app: tauri::AppHandle) {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || spotlight::zaostri(&h));
}

/// Poznámka od lišty do jejího protokolu.
///
/// UI je windows_subsystem "windows" a nemá konzoli, takže když se
/// něco pokazí ve stránce, není to kde vidět. Tohle je jediná cesta,
/// jak takový problém vyšetřit u uživatele — píše se jen na
/// nestandardní cestě, ne při běžném běhu.
#[tauri::command(async)]
fn spotlight_note(msg: String) {
    spotlight::log(&msg);
}

/// Schová vyhledávací lištu. Volá ji samotná lišta při Escape —
/// okno nemá křížek, takže tohle je jediná cesta ven zevnitř.
#[tauri::command]
fn hide_spotlight(app: tauri::AppHandle) {
    spotlight::hide(&app);
}

/// Otevře lištu i bez zkratky (z hlavního okna).
#[tauri::command]
fn show_spotlight(app: tauri::AppHandle) -> Result<(), String> {
    if !hotkey::zapnuta() {
        return Err("vyhledávací lišta je v Nastavení vypnutá".into());
    }
    spotlight::toggle(&app, SPOTLIGHT_ROUTE)
}

/// Procentní kódování pro dotaz v URL. Vlastní, protože kvůli jedné
/// funkci nemá smysl přidávat závislost.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Kde leží databáze a kam by se dala přesunout.
#[tauri::command(async)]
fn query_db_location() -> Result<ipc::client::DbLocation, String> {
    ipc::client::query_db_location().map_err(|e| e.to_string())
}

/// Přesune databázi jinam. Prázdno = zpátky na výchozí místo.
///
/// Služba jen ověří, že se do adresáře dá zapsat, a uloží přání —
/// samotné stěhování udělá až její příští start, kdy databázi nikdo
/// nedrží otevřenou.
#[tauri::command(async)]
fn set_db_dir(dir: String) -> Result<(), String> {
    ipc::client::set_db_dir(dir).map_err(|e| e.to_string())
}

/// Nechá uživatele vybrat složku. Vrací prázdno, když výběr zrušil.
///
/// Vlastní volání `IFileDialog` místo pluginu: aplikace žádný dialogový
/// plugin nepoužívá a kvůli jednomu tlačítku ho tahat nemá smysl.
#[tauri::command(async)]
fn pick_folder() -> Result<String, String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };

    // SAFETY: COM se inicializuje pro tohle vlákno; všechna rozhraní
    // drží Rust a uvolní je Drop.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("dialog se nepodařilo otevřít: {e}"))?;
        let opts = dlg.GetOptions().unwrap_or_default();
        let _ = dlg.SetOptions(opts | FOS_PICKFOLDERS);
        // Uživatel zrušil výběr — není to chyba, jen prázdný výsledek.
        if dlg.Show(None).is_err() {
            return Ok(String::new());
        }
        let item: IShellItem = dlg.GetResult().map_err(|e| e.to_string())?;
        let p = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|e| e.to_string())?;
        let s = p.to_string().map_err(|e| e.to_string())?;
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        Ok(s)
    }
}

// ── Aktualizace aplikace ───────────────────────────────────────────
//
// Zdroj pravdy je `release/version.txt` v repozitáři — týž soubor, ze
// kterého bere verzi instalátor. Kontrola i stažení jdou přes commit
// SHA, ne přes větev: `raw.githubusercontent.com` drží soubory v CDN
// cache pět minut a query parametry ignoruje, takže by se mohla vrátit
// stará `version.txt` k novým binárkám. Adresa s konkrétním commitem
// je neměnná, takže tenhle problém nemá.
//
// Samotnou aktualizaci NEDĚLÁME sami: stáhne se `WinsentSetup.exe`
// a spustí se. Ten už umí zastavit službu, zavřít okno aplikace,
// přepsat binárky, službu vrátit do běhu a aplikaci znovu spustit —
// druhá implementace téhož by se s ním nutně rozešla.

const RAW_HOST: &str = "raw.githubusercontent.com";
const API_HOST: &str = "api.github.com";
const REPO: &str = "iva-exe/WinSent";

/// Kde bydlí nainstalovaná kopie (`%ProgramFiles%\Winsent`).
fn install_dir() -> std::path::PathBuf {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    std::path::PathBuf::from(pf).join("Winsent")
}

/// Verze nainstalované kopie. `None` = běží se z vývojového stromu,
/// kde soubor s verzí není — tam se aktualizace nenabízí.
fn installed_version() -> Option<String> {
    std::fs::read_to_string(install_dir().join("version.txt"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Commit, na kterém repozitář právě stojí.
fn latest_commit() -> Result<String, String> {
    let body = win_sys::http::get(API_HOST, &format!("/repos/{REPO}/commits/main"), |_| {})
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&body);
    let pos = text
        .find("\"sha\":\"")
        .ok_or("odpověď GitHubu neobsahuje commit")?;
    let sha: String = text[pos + 7..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if sha.len() < 7 {
        return Err("GitHub vrátil neplatný commit".into());
    }
    Ok(sha)
}

#[derive(Debug, Serialize)]
struct UpdateInfo {
    /// Verze, která běží teď. Prázdná u vývojového stromu.
    current: String,
    /// Verze v repozitáři; prázdná, když se nepodařilo zjistit.
    latest: String,
    /// Je co aktualizovat? Jen když obě verze známe a liší se.
    available: bool,
    /// Proč se nepodařilo zjistit (síť, GitHub) — UI to nekřičí,
    /// ale v ladění se to hodí.
    error: Option<String>,
}

/// Je v repozitáři jiná verze, než která běží?
///
/// Rozdíl, ne „vyšší": verze je `0.1.0+RRRRMMDD.HHMM` a porovnávat to
/// jako číslo by znamenalo psát parser, který se u prvního jiného tvaru
/// splete. Vydavatel navíc může vydat i starší build zpátky, a i to je
/// změna, o které má uživatel vědět.
#[tauri::command(async)]
fn check_update() -> UpdateInfo {
    let current = installed_version().unwrap_or_default();
    if current.is_empty() {
        return UpdateInfo {
            current,
            latest: String::new(),
            available: false,
            error: Some("aplikace neběží z instalace — aktualizace se nenabízí".into()),
        };
    }
    // Verze se čte PŘÍMO z větve, ne přes commit z API.
    //
    // GitHub API má pro nepřihlášené 60 dotazů za hodinu z jedné IP.
    // Kontrola každých 30 s je 120 za hodinu, takže limit dojde vždycky
    // — a pak vrací 403. Naměřeno: brána updatecheck spadla na
    // „server odpověděl chybou 403" a instalátor, který si SHA bere
    // odtud, začal stahovat starší commit. `raw` jede přes CDN, nemá
    // hodinový limit a pro odpověď „jaká verze je venku" stačí.
    //
    // Commit se dohledá teprve tehdy, když je co stahovat (`run_update`).
    let latest = match win_sys::http::get(
        RAW_HOST,
        &format!("/{REPO}/main/release/version.txt"),
        |_| {},
    )
    .map_err(|e| e.to_string())
    .map(|b| String::from_utf8_lossy(&b).trim().to_string())
    {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            return UpdateInfo {
                current,
                latest: String::new(),
                available: false,
                error: Some("server vrátil prázdnou verzi".into()),
            }
        }
        Err(e) => {
            return UpdateInfo {
                current,
                latest: String::new(),
                available: false,
                error: Some(e),
            }
        }
    };
    UpdateInfo {
        available: latest != current,
        current,
        latest,
        error: None,
    }
}

/// Stáhne instalátor a spustí ho.
///
/// Vrací se hned, jakmile je instalátor na světě — čekat nemá smysl:
/// jeho první práce je zavřít tohle okno. Zbytek (služba, binárky,
/// nové spuštění aplikace) dělá on.
#[tauri::command(async)]
fn run_update() -> Result<String, String> {
    let sha = latest_commit()?;
    let data = win_sys::http::get(
        RAW_HOST,
        &format!("/{REPO}/{sha}/release/WinsentSetup.exe"),
        |_| {},
    )
    .map_err(|e| e.to_string())?;
    // Každý PE soubor začíná „MZ". Když místo instalátoru dorazí
    // chybová stránka nebo půlka souboru, pozná se to tady — ne až
    // ve chvíli, kdy má zastavit službu.
    if data.len() < 100_000 || &data[..2] != b"MZ" {
        return Err(format!(
            "instalátor se stáhl poškozený ({} B) — zkus to za chvíli znovu",
            data.len()
        ));
    }

    // Do vlastní podsložky v TEMPu se jménem podle commitu: běžící
    // instalátor drží svůj soubor zamčený a přepisovat ho pod rukama
    // by skončilo chybou sdílení.
    let dir = std::env::temp_dir().join("winsent-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exe = dir.join(format!("WinsentSetup-{}.exe", &sha[..7.min(sha.len())]));
    std::fs::write(&exe, &data).map_err(|e| format!("instalátor nejde uložit: {e}"))?;

    // `runas` = spuštění se zvýšenými právy (Windows se zeptají).
    // Instalátor má práva správce i v manifestu, ale bez „runas" by
    // ShellExecute zdědil token téhle aplikace a UAC by se neukázalo.
    launch_elevated(&exe, "/quiet")?;
    Ok(exe.to_string_lossy().to_string())
}

/// Spustí program se zvýšenými právy přes ShellExecuteW(„runas").
fn launch_elevated(exe: &std::path::Path, args: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = HSTRING::from("runas");
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(args);
    // SAFETY: všechny řetězce žijí až za volání; ShellExecuteW vrací
    // pseudohandle, který se nezavírá.
    let rc = unsafe {
        ShellExecuteW(
            None,
            &verb,
            &file,
            &params,
            None,
            SW_SHOWNORMAL,
        )
    };
    // Návratová hodnota ≤ 32 je chybový kód; 5 = uživatel odmítl UAC.
    let code = rc.0 as usize;
    match code {
        c if c > 32 => Ok(()),
        5 => Err("aktualizace potřebuje souhlas správce — nebyl udělen".into()),
        c => Err(format!("instalátor se nepodařilo spustit (kód {c})")),
    }
}

/// Ukáže a vyzdvihne hlavní okno.
fn show_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    // Nejdřív probrat webview, teprve pak ukázat okno — jinak by se
    // první snímek kreslil do něčeho, co má vypnutou kompozici.
    uspi_webview(app, "main", false);
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Uspí nebo probudí webview daného okna.
///
/// Schování okna samo o sobě webview NEZASTAVÍ: Tauri sáhne jen na
/// okno Windows, kdežto WebView2 dál kreslí, tiká časovači a chodí se
/// ptát služby. Naměřeno: aplikace zavřená do oznamovací oblasti brala
/// úplně stejně jako otevřená — přes dvě procenta systému a stovky
/// megabajtů, a to všechno do okna, které nikdo nevidí.
///
/// Tohle je to chybějící slovo, kterým se WebView2 řekne, že se na něj
/// nikdo nedívá. Chromium si pak sám utlumí časovače a zastaví
/// kompozici; ve stránce k tomu navíc začne platit `document.hidden`,
/// takže se na to dá časem navázat i tam.
fn uspi_webview(app: &tauri::AppHandle, label: &str, uspat: bool) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window(label) {
        let v: &tauri::Webview<tauri::Wry> = w.as_ref();
        let _ = if uspat { v.hide() } else { v.show() };
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
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // Druhé spuštění s --tray okno NEVYTAHUJE. Stane se to,
            // když Windows po přihlášení obnoví aplikaci z minulé
            // relace a hned nato ji pustí i položka po spuštění —
            // uživatel by dostal okno přes celou plochu, přestože si
            // vybral tichý start.
            if argv.iter().any(|a| a == autostart::PREPINAC_TRAY) {
                return;
            }
            show_main_window(app);
        }))
        .invoke_handler(tauri::generate_handler![
            ping_daemon,
            query_procs,
            query_system,
            query_self_usage,
            query_db_location,
            search_web,
            open_settings_page,
            launch_app,
            query_launchables,
            query_launchable_icon,
            query_autostart,
            set_autostart,
            get_spotlight_hotkey,
            set_spotlight_hotkey,
            hide_spotlight,
            focus_spotlight,
            get_spotlight_enabled,
            set_spotlight_enabled,
            query_ui_zoom,
            set_ui_zoom,
            spotlight_note,
            show_spotlight,
            set_db_dir,
            pick_folder,
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
            paths_exist,
            query_volumes,
            query_hardware,
            query_displays,
            query_network,
            query_connection,
            query_security,
            query_users,
            query_drivers,
            query_collector_health,
            query_crash_reports,
            save_report,
            query_incident_dumps,
            query_perm_use,
            query_perm_use_totals,
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
            plan_purge_ghost,
            check_update,
            run_update,
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
            // Zvětšení UI z minule. Musí se nastavit i tady, ne jen
            // při změně — webview startuje vždycky na sto procentech.
            pouzij_zvetseni(app.handle(), hotkey::zvetseni());
            // Spouštění po přihlášení. Ve výchozím stavu zapnuté a
            // zabere to jen při úplně prvním startu; pak rozhoduje
            // uživatel přepínačem v Nastavení.
            autostart::zajisti_vychozi();
            // Start po přihlášení patří do oznamovací oblasti, ne přes
            // celou plochu. Okno se schovává AŽ TADY, ne stavbou
            // skrytého okna: postavit okno skryté a hned nato ho
            // ukázat už jednou v tomhle projektu nefungovalo (viz
            // spotlight.rs) a cena za to je jen krátké bliknutí.
            if autostart::tichy_start() {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            // Globální zkratka pro vyhledávací lištu. Registruje se ve
            // vlastním vlákně (viz hotkey) a jen posílá práci sem.
            let handle = app.handle().clone();
            // Prázdný zápis = neregistrovat. Vlákno běží tak jako tak,
            // aby se zkratka dala zapnout za běhu bez restartu.
            let zkratka = if hotkey::zapnuta() {
                hotkey::load()
            } else {
                String::new()
            };
            hotkey::start(&zkratka, move || {
                let h = handle.clone();
                // Okna se smějí obsluhovat jen z hlavního vlákna.
                let _ = handle.run_on_main_thread(move || {
                    if let Err(e) = spotlight::toggle(&h, SPOTLIGHT_ROUTE) {
                        spotlight::log(&format!("toggle selhal: {e}"));
                    }
                });
            });
            // Seznam spustitelných aplikací dopředu, na pozadí.
            // Enumerace složky „Aplikace" stojí přes půl sekundy a bez
            // tohohle by ji zaplatilo první vyvolání lišty — tedy
            // přesně ta chvíle, kdy má být lišta nejrychlejší.
            std::thread::spawn(|| {
                let _ = launch::seznam();
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Zavření okna = schovat do tray, UI běží dál (ikona zůstává).
            // Spotlight se jen schovává — zavřít ho znamená zahodit
            // webview a příští vyvolání by se zdrželo jeho stavbou.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                use tauri::Manager;
                api.prevent_close();
                let _ = window.hide();
                // Uspat i webview, jinak běží dál naprázdno (viz
                // `uspi_webview`). Spotlight se schovává vlastní cestou.
                uspi_webview(window.app_handle(), window.label(), true);
            }
        })
        .run(tauri::generate_context!())
        .expect("start Tauri selhal");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msix_klic_nespoji_cestu_s_balickem() {
        assert_eq!(
            msix_klic("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App").as_deref(),
            Some("msix:Microsoft.WindowsCalculator_8wekyb3d8bbwe")
        );
        // Cesta s vykřičníkem v názvu složky. Bez tvrdé podmínky by
        // z ní vznikl klíč „msix:C:\…" a slepil dvě různé aplikace
        // do jedné položky — reálná položka na testovaném stroji.
        assert_eq!(msix_klic(r"C:\Users\X\AppData\Local\osu!\osu!.exe"), None);
        // Prostá cesta bez vykřičníku.
        assert_eq!(msix_klic(r"D:\steam\Steam.exe"), None);
        // Vypadá jako rodina, ale identifikátor vydavatele nemá 13 znaků.
        assert_eq!(msix_klic("Neco.Divneho_kratke!App"), None);
    }
}
