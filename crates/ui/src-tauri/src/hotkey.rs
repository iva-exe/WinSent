//! Globální klávesová zkratka pro spotlight.
//!
//! Registruje se přes `RegisterHotKey` a poslouchá ve vlastním vlákně
//! s vlastní frontou zpráv. Vlastní vlákno je nutnost: `RegisterHotKey`
//! doručuje `WM_HOTKEY` tomu vláknu, které zkratku zaregistrovalo,
//! takže se musí registrovat i číst na jednom místě.
//!
//! Zkratka se dá změnit za běhu — vlákno se o to postará samo, aby
//! `RegisterHotKey` a `UnregisterHotKey` běžely v tomtéž vlákně.
//!
//! Nastavení bydlí v `%APPDATA%\Winsent\ui.json`. Do konfigurace služby
//! nepatří: je to volba uživatelského rozhraní, ne hlídače, a služba
//! běží pod SYSTEMem, kde by ji nastavil někdo jiný, než kdo ji používá.

use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

/// Identifikátor zkratky uvnitř vlákna. Jediná, takže stačí jednička.
const HOTKEY_ID: i32 = 1;

/// Výchozí zkratka. Alt+mezerník je na Windows systémové menu okna,
/// ale to má smysl jen u okna s rámem — naše okna rám nemají.
pub const DEFAULT: &str = "Alt+Space";

/// Zprávy do vlákna zkratky.
enum Cmd {
    /// Přeregistrovat na nový zápis; prázdné = jen odregistrovat.
    Set(String),
}

static TX: OnceLock<Sender<Cmd>> = OnceLock::new();
/// ID vlákna zkratky — přes něj se dá vlákno probudit z `GetMessageW`.
static TID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Vlastní zpráva „přišel příkaz do fronty".
const WM_PREREGISTRUJ: u32 = 0x0400 + 1; // WM_USER + 1

/// Spustí vlákno zkratky a zaregistruje `accel`. Volá se jednou.
///
/// `on_fire` běží v tom vlákně — má jen předat práci dál, ne dělat
/// cokoli dlouhého, jinak by se zkratka přestala hlásit.
pub fn start(accel: &str, on_fire: impl Fn() + Send + 'static) {
    let (tx, rx) = channel::<Cmd>();
    if TX.set(tx).is_err() {
        return; // už běží
    }
    let prvni = accel.to_string();
    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || {
            // SAFETY: jen dotaz na ID vlastního vlákna.
            TID.store(
                unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
                std::sync::atomic::Ordering::SeqCst,
            );
            let mut aktivni = register(&prvni);
            loop {
                // Nejdřív vyřídit požadavky na změnu, pak čekat na zprávu.
                while let Ok(Cmd::Set(novy)) = rx.try_recv() {
                    if aktivni {
                        // SAFETY: odregistrujeme jen to, co jsme sami
                        // v tomhle vlákně zaregistrovali.
                        unsafe {
                            let _ = UnregisterHotKey(None, HOTKEY_ID);
                        }
                    }
                    aktivni = register(&novy);
                }
                let mut msg = MSG::default();
                // SAFETY: GetMessageW plní lokální strukturu; -1 = chyba.
                let rc = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                if rc.0 == -1 {
                    break;
                }
                if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_ID {
                    on_fire();
                }
            }
        })
        .ok();
}

/// Přeregistruje zkratku za běhu.
///
/// Vlákno visí v `GetMessageW`, takže samotné poslání do kanálu by nic
/// neudělalo — musí se probudit vlastní zprávou do jeho fronty.
pub fn set(accel: &str) {
    let Some(tx) = TX.get() else { return };
    let _ = tx.send(Cmd::Set(accel.to_string()));
    let tid = TID.load(std::sync::atomic::Ordering::SeqCst);
    if tid != 0 {
        // SAFETY: zpráva do fronty vlastního vlákna; parametry se nečtou.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                tid,
                WM_PREREGISTRUJ,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
}

/// Zaregistruje zkratku. Vrací, jestli se to povedlo.
fn register(accel: &str) -> bool {
    let Some((m, vk)) = parse(accel) else {
        tracing_warn(&format!("zkratku {accel:?} neumím přečíst"));
        return false;
    };
    // SAFETY: registrace pro vlákno, ve kterém se i čte fronta zpráv.
    let ok = unsafe { RegisterHotKey(None, HOTKEY_ID, m | MOD_NOREPEAT, vk) }.is_ok();
    if !ok {
        // Nejčastější důvod: zkratku už drží jiný program.
        tracing_warn(&format!("zkratku {accel:?} se nepodařilo zabrat"));
    }
    ok
}

/// „Ctrl+Shift+P" → (modifikátory, virtuální kód klávesy).
///
/// Vlastní parser místo knihovny: rozumí jen tomu, co nabízíme
/// v nastavení, a nesrozumitelný zápis raději odmítne, než aby si
/// domyslel něco jiného, než uživatel napsal.
pub fn parse(accel: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut m = HOT_KEY_MODIFIERS(0);
    let mut key = None;
    for kus in accel.split('+') {
        let k = kus.trim();
        if k.is_empty() {
            continue;
        }
        match k.to_ascii_lowercase().as_str() {
            "alt" => m |= MOD_ALT,
            "ctrl" | "control" => m |= MOD_CONTROL,
            "shift" => m |= MOD_SHIFT,
            "win" | "super" | "meta" => m |= MOD_WIN,
            jiné => key = Some(vk_code(jiné)?),
        }
    }
    // Zkratka bez modifikátoru by zabrala klávesu celému systému.
    if m.0 == 0 {
        return None;
    }
    Some((m, key?))
}

/// Jméno klávesy → virtuální kód.
fn vk_code(k: &str) -> Option<u32> {
    Some(match k {
        "space" | "mezerník" => 0x20,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "backspace" => 0x08,
        "insert" => 0x2D,
        "delete" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        // F1–F24.
        f if f.starts_with('f') && f[1..].parse::<u32>().is_ok() => {
            let n = f[1..].parse::<u32>().ok()?;
            if !(1..=24).contains(&n) {
                return None;
            }
            0x6F + n
        }
        // Jedno písmeno nebo číslice — jejich VK kód je ASCII velkého znaku.
        s if s.chars().count() == 1 => {
            let c = s.chars().next()?.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                c as u32
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// Varování do logu. Vlastní funkce, ať se `tracing` nemusí tahat do
/// všech míst v tomhle modulu.
fn tracing_warn(msg: &str) {
    eprintln!("hotkey: {msg}");
}

// ── Uložené nastavení ──────────────────────────────────────────────

/// Soubor s nastavením rozhraní (zatím jen zkratka).
pub fn prefs_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Winsent").join("ui.json")
}

/// Přečte uloženou zkratku; když soubor není nebo je vadný, výchozí.
pub fn load() -> String {
    let Ok(text) = std::fs::read_to_string(prefs_path()) else {
        return DEFAULT.to_string();
    };
    // Vlastní minimální čtení místo serde_json: jedna hodnota nestojí
    // za další závislost v hostiteli.
    for radek in text.lines() {
        if let Some(v) = radek.split_once("\"spotlight_hotkey\"") {
            if let Some(zac) = v.1.find('"') {
                let zbytek = &v.1[zac + 1..];
                if let Some(kon) = zbytek.find('"') {
                    let s = zbytek[..kon].trim().to_string();
                    if !s.is_empty() {
                        return s;
                    }
                }
            }
        }
    }
    DEFAULT.to_string()
}

/// Uloží zkratku. Ověří se, že jí rozumíme — neplatný zápis by po
/// restartu znamenal, že zkratka nefunguje a nikdo neví proč.
pub fn save(accel: &str) -> Result<(), String> {
    if parse(accel).is_none() {
        return Err(format!(
            "zkratce {accel:?} nerozumím — potřebuje modifikátor (Alt, Ctrl, Shift, Win) a klávesu"
        ));
    }
    let p = prefs_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("nelze vytvořit {}: {e}", d.display()))?;
    }
    let text = format!("{{\n  \"spotlight_hotkey\": \"{}\"\n}}\n", accel.replace('"', ""));
    std::fs::write(&p, text).map_err(|e| format!("nelze zapsat {}: {e}", p.display()))
}
