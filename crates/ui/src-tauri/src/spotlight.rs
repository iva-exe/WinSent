//! Spotlight — jedna sekce aplikace jako samostatné okno na zkratku.
//!
//! Modul je schválně obecný: umí vyvolat LIBOVOLNOU cestu ve frontendu,
//! ne jen vyhledávání. Okno je jedno a jen se mu mění adresa, takže
//! přidat další „spotlight sekci" znamená zavolat `show` s jinou cestou.
//!
//! Vzhled okna: bez rámu a bez tlačítek Windows, průhledné pozadí se
//! stejným rozostřením jako hlavní okno, vždy nad ostatními a vždy
//! uprostřed obrazovky, na které je zrovna myš. Do hlavního panelu
//! nepatří — je to vyvolávací lišta, ne aplikace.
//!
//! Zavírá se samo při ztrátě zaměření. Okno bez křížku, které zůstane
//! viset, když uživatel klikne jinam, by nešlo zavřít vůbec.

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// Jméno okna. Jedno pro všechny sekce — druhé okno by znamenalo druhý
/// webview a s ním desítky megabajtů paměti navíc.
pub const LABEL: &str = "spotlight";

/// Rozměry okna. Šířka je pevná (jako u Spotlightu), výška je strop —
/// obsah si ji zmenší sám, když je výsledků málo.
/// Kdy se okno naposledy ukázalo (ms od startu procesu).
///
/// Hned po  chodí  — okno ještě zaměření
/// nezískalo a obsluha, která na něj reaguje schováním, ho zase
/// sklapla dřív, než ho uživatel uviděl. Naměřeno: v protokolu stálo
/// "okno vytvořeno a zobrazeno", ale mezi viditelnými okny žádné
/// nebylo. Ztrátu zaměření proto krátce po zobrazení ignorujeme.
static POSLEDNI_SHOW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const OCHRANNA_LHUTA_MS: u64 = 600;

fn ted_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn oznac_zobrazeni() {
    POSLEDNI_SHOW.store(ted_ms(), std::sync::atomic::Ordering::SeqCst);
}

fn cerstve_zobrazeno() -> bool {
    ted_ms().saturating_sub(POSLEDNI_SHOW.load(std::sync::atomic::Ordering::SeqCst))
        < OCHRANNA_LHUTA_MS
}

const WIDTH: f64 = 860.0;
const HEIGHT: f64 = 520.0;

/// Poznámka do vlastního protokolu.
///
/// UI je `windows_subsystem = "windows"`, takže nemá konzoli a `eprintln`
/// nikam nevede. Když se lišta nevyvolá, tohle je jediné místo, kde se
/// dá zjistit proč — bez něj zbývá hádání.
pub fn log(msg: &str) {
    let Ok(base) = std::env::var("APPDATA") else {
        return;
    };
    let dir = std::path::PathBuf::from(base).join("Winsent");
    let _ = std::fs::create_dir_all(&dir);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("spotlight.log"))
    {
        let _ = writeln!(f, "{msg}");
    }
}

/// Ukáže sekci ve spotlight okně; když už svítí tatáž, zase ho schová.
///
/// `route` je cesta ve frontendu (například `spotlight/search`).
pub fn toggle(app: &AppHandle, route: &str) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        let vidno = w.is_visible().unwrap_or(false);
        if vidno {
            let _ = w.hide();
            return Ok(());
        }
        let (x, y) = pozice(app);
        let _ = w.set_position(tauri::LogicalPosition::new(x, y));
        oznac_zobrazeni();
        let _ = w.show();
        let _ = w.set_focus();
        // Přeposlat, kterou sekci ukázat — okno může být z minula jiné.
        let _ = w.emit_to(LABEL, "spotlight:route", route.to_string());
        return Ok(());
    }
    create(app, route)
}

/// Schová okno, pokud existuje.
pub fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/// Založí okno. Dělá se jen jednou; pak se recykluje.
///
/// Pozice se počítá PŘED stavbou a okno se rovnou staví viditelné.
/// Postavit ho skryté a hned nato zavolat show() nefungovalo: okno
/// vzniklo, protokol hlásil úspěch, ale mezi viditelnými okny žádné
/// nebylo — zobrazení se ztratilo v závodu se zaměřením.
fn create(app: &AppHandle, route: &str) -> Result<(), String> {
    let (x, y) = pozice(app);
    let url = WebviewUrl::App(route.to_string().into());
    oznac_zobrazeni();
    let w = WebviewWindowBuilder::new(app, LABEL, url)
        .title("Winsent")
        .inner_size(WIDTH, HEIGHT)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(true)
        .visible(true)
        .build()
        .map_err(|e| {
            log(&format!("stavba okna selhala: {e}"));
            format!("spotlight okno nejde vytvořit: {e}")
        })?;

    // Zmizet při ztrátě zaměření. Bez tohohle by okno bez křížku
    // zůstalo viset přes celou plochu a nešlo by se ho zbavit.
    if !w.is_visible().unwrap_or(false) {
        log("okno se postavilo, ale není vidět");
    }

    let handle = app.clone();
    w.on_window_event(move |e| {
        if let tauri::WindowEvent::Focused(false) = e {
            if cerstve_zobrazeno() {
                return;
            }
            hide(&handle);
        }
    });

    Ok(())
}

/// Kam okno postavit: střed monitoru, na kterém je kurzor.
///
/// Primární monitor by nestačil — na dvou obrazovkách by se lišta
/// otevírala jinde, než se uživatel dívá, a on ji hledá tam, kde má myš.
/// Trochu nad optickým středem, jak to má Spotlight i Raycast; přesný
/// střed působí, jako by lišta padala pod těžiště obrazovky.
fn pozice(app: &AppHandle) -> (f64, f64) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(m) = monitor else {
        return (200.0, 200.0);
    };
    let scale = m.scale_factor();
    let size = m.size().to_logical::<f64>(scale);
    let origin = m.position().to_logical::<f64>(scale);
    let x = origin.x + (size.width - WIDTH) / 2.0;
    let y = origin.y + (size.height - HEIGHT) / 2.0 - size.height * 0.08;
    (x, y.max(origin.y + 24.0))
}
