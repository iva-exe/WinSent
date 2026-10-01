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

use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender};
use std::sync::{Mutex, OnceLock};

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
    /// Druhá položka je kam poslat, jestli se to povedlo.
    Set(String, Option<SyncSender<bool>>),
}

static TX: OnceLock<Sender<Cmd>> = OnceLock::new();
/// ID vlákna zkratky — přes něj se dá vlákno probudit z `GetMessageW`.
static TID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Vlastní zpráva „přišel příkaz do fronty".
const WM_PREREGISTRUJ: u32 = 0x0400 + 1; // WM_USER + 1

/// Proč zkratka právě teď neplatí; `None` = platí, nebo je lišta vypnutá.
///
/// Registrace při startu UI se nemá komu ohlásit — příkaz z Nastavení
/// v tu chvíli nikdo nevolá. Dřív to skončilo v `eprintln`, který
/// v okenní aplikaci bez konzole nikam nevede, a Nastavení pak tvrdilo,
/// že zkratka funguje. Tady si to Nastavení může vyzvednout.
static CHYBA: Mutex<Option<String>> = Mutex::new(None);

/// Proč zkratka neplatí (viz `CHYBA`).
pub fn chyba() -> Option<String> {
    CHYBA.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Text chyby pro zkratku, kterou se nepodařilo zabrat.
fn zprava_obsazena(accel: &str) -> String {
    format!("Zkratku {accel} už používá jiný program. Zvol jinou.")
}

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
            // Fronta zpráv vzniká až prvním voláním USER funkce. Bez
            // tohohle by `set()` mohl poslat probouzecí zprávu dřív, než
            // fronta existuje — PostThreadMessageW by tiše selhal
            // a vlákno by usnulo v GetMessageW s nevyřízeným příkazem.
            // SAFETY: PM_NOREMOVE jen nahlédne do (právě vzniklé) fronty.
            unsafe {
                let mut m = MSG::default();
                let _ = windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                    &mut m,
                    None,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::PM_NOREMOVE,
                );
            }
            // SAFETY: jen dotaz na ID vlastního vlákna.
            TID.store(
                unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
                std::sync::atomic::Ordering::SeqCst,
            );
            // Zápis, který opravdu platí — na něj se vrací, když nový
            // zabrat nejde. Jinak by po nepovedené změně nefungovala
            // ani stará zkratka, ani nová.
            let mut aktualni = prvni.clone();
            let mut aktivni = register(&prvni);
            zapis_stav(&aktualni, aktivni);
            loop {
                // Nejdřív vyřídit požadavky na změnu, pak čekat na zprávu.
                while let Ok(Cmd::Set(novy, odpoved)) = rx.try_recv() {
                    if aktivni {
                        // SAFETY: odregistrujeme jen to, co jsme sami
                        // v tomhle vlákně zaregistrovali.
                        unsafe {
                            let _ = UnregisterHotKey(None, HOTKEY_ID);
                        }
                    }
                    let zabrano = register(&novy);
                    // Prázdný zápis = vypnutá lišta; to se povést musí.
                    let ok = zabrano || novy.trim().is_empty();
                    if ok {
                        aktivni = zabrano;
                        aktualni = novy;
                    } else {
                        aktivni = register(&aktualni);
                    }
                    zapis_stav(&aktualni, aktivni);
                    if let Some(o) = odpoved {
                        let _ = o.send(ok);
                    }
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

/// Uloží do `CHYBA`, jestli platný zápis opravdu drží zkratku.
fn zapis_stav(aktualni: &str, aktivni: bool) {
    let stav = (!aktivni && !aktualni.trim().is_empty()).then(|| zprava_obsazena(aktualni));
    *CHYBA.lock().unwrap_or_else(|e| e.into_inner()) = stav;
}

/// Přeregistruje zkratku za běhu a počká, jak to dopadlo.
///
/// Dřív se jen poslal příkaz a hned vracelo „hotovo": když zkratku
/// držel jiný program (PowerToys Run má výchozí právě Alt+mezerník),
/// Nastavení ukázalo novou zkratku bez chyby a nefungovala žádná.
/// Když se nový zápis zabrat nepodaří, vlákno vrátí ten předchozí.
///
/// Vlákno visí v `GetMessageW`, takže samotné poslání do kanálu by nic
/// neudělalo — musí se probudit vlastní zprávou do jeho fronty.
///
/// Blokuje až na dvě sekundy, takže se nesmí volat z hlavního vlákna
/// ani z vlákna zkratky samotného.
pub fn set(accel: &str) -> Result<(), String> {
    // Vlákno ještě neběží — zápis převezme `start` při svém spuštění.
    let Some(tx) = TX.get() else { return Ok(()) };
    let (otx, orx) = sync_channel::<bool>(1);
    tx.send(Cmd::Set(accel.to_string(), Some(otx)))
        .map_err(|_| "vlákno klávesové zkratky neběží".to_string())?;
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
    match orx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(true) => Ok(()),
        Ok(false) => Err(zprava_obsazena(accel.trim())),
        Err(_) => Err("zkratku se nepodařilo nastavit — vlákno zkratky neodpovídá".into()),
    }
}

/// Zaregistruje zkratku. Vrací, jestli se to povedlo.
fn register(accel: &str) -> bool {
    // Prázdný zápis znamená „nemá se registrovat" — tak se lišta vypíná
    // v Nastavení. Není to chyba a nemá se to hlásit jako chyba.
    if accel.trim().is_empty() {
        return false;
    }
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

/// Varování do protokolu lišty.
///
/// Dřív to byl `eprintln`, jenže UI je okenní aplikace bez konzole
/// a výpis nikam nevedl — nepovedená registrace zkratky pak nebyla
/// k dohledání vůbec.
fn tracing_warn(msg: &str) {
    crate::spotlight::log(&format!("hotkey: {msg}"));
}

// ── Uložené nastavení ──────────────────────────────────────────────

/// Soubor s nastavením rozhraní (zatím jen zkratka).
pub fn prefs_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Winsent").join("ui.json")
}

/// Zámek nad čtením a zápisem `ui.json`.
///
/// Každé uložení čte ostatní volby a přepisuje celý soubor. Dva
/// souběžné příkazy (zvětšení a vypnutí lišty běží na pracovních
/// vláknech) by si jinak mohly navzájem přečíst rozepsaný soubor
/// a uložit výchozí hodnoty — vypnutá lišta by se po restartu sama
/// zapnula na Alt+mezerníku. Zámek musí krýt čtení i zápis dohromady.
static PREFS_LOCK: Mutex<()> = Mutex::new(());

fn zamek() -> std::sync::MutexGuard<'static, ()> {
    // Otrávený zámek nevadí — chrání jen pořadí, ne data v paměti.
    PREFS_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn precti() -> Option<String> {
    std::fs::read_to_string(prefs_path()).ok()
}

/// Je vyhledávací lišta vůbec zapnutá?
///
/// Výchozí je ano. Kdo ji nechce, vypne si ji v Nastavení a zkratka se
/// odregistruje — Alt+mezerník pak zase patří systému.
pub fn zapnuta() -> bool {
    precti().is_none_or(|t| zapnuta_z(&t))
}

fn zapnuta_z(text: &str) -> bool {
    !text.contains("\"spotlight_enabled\": false")
}

/// Všechny volby z JEDNOHO čtení souboru.
///
/// Tři samostatná čtení mohla každé zastihnout soubor v jiném stavu.
fn nacti() -> (String, bool, f64) {
    match precti() {
        Some(t) => (zkratka_z(&t), zapnuta_z(&t), zvetseni_z(&t)),
        None => (DEFAULT.to_string(), true, 1.0),
    }
}

/// Uloží, jestli je lišta zapnutá. Zkratka se zachová.
pub fn save_zapnuta(zapnuta: bool) -> Result<(), String> {
    let _g = zamek();
    let (accel, _, zv) = nacti();
    zapis(&accel, zapnuta, zv)
}

/// Obsah souboru. Všechny hodnoty vždycky pohromadě — kdyby se psaly
/// zvlášť, druhý zápis by ten první přemazal.
fn text_souboru(accel: &str, zapnuta: bool, zvetseni: f64) -> String {
    format!(
        "{{\n  \"spotlight_hotkey\": \"{}\",\n  \"spotlight_enabled\": {},\n  \"ui_zoom\": {:.2}\n}}\n",
        accel.replace('"', ""),
        zapnuta,
        zvetseni
    )
}

/// Zapíše celý soubor. Volá se jen pod `PREFS_LOCK`.
///
/// Atomicky přes dočasný soubor a přejmenování: `std::fs::write` soubor
/// napřed zkrátí na nulu, a kdo ho v tu chvíli četl (nebo kdyby mezi
/// zkrácením a zápisem vypadl proud), viděl prázdno — a prázdno znamená
/// výchozí hodnoty.
fn zapis(accel: &str, zapnuta: bool, zvetseni: f64) -> Result<(), String> {
    use std::io::Write;
    let p = prefs_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("nelze vytvořit {}: {e}", d.display()))?;
    }
    let text = text_souboru(accel, zapnuta, zvetseni);
    let tmp = p.with_extension("json.tmp");
    let pres_tmp = std::fs::File::create(&tmp)
        .and_then(|mut f| {
            f.write_all(text.as_bytes())?;
            f.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, &p));
    if pres_tmp.is_ok() {
        return Ok(());
    }
    // Přejmenování nemusí projít (antivir nebo jiný proces drží ui.json
    // bez sdílení mazání). Pak radši zapsat napřímo, než změnu ztratit.
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(&p, text).map_err(|e| format!("nelze zapsat {}: {e}", p.display()))
}

/// Meze zvětšení UI.
///
/// Padesát procent je hodně málo a text je na hraně čitelnosti, ale na
/// malé obrazovce s vysokým rozlišením to má smysl — a je to volba
/// uživatele, ne naše. Nad sto padesáti se rozvržení rozbíjí, protože
/// počítá s tím, že se sekce vejde na obrazovku.
pub const ZVETSENI_MIN: f64 = 0.5;
pub const ZVETSENI_MAX: f64 = 1.5;

/// Zvětšení uživatelského rozhraní (1.0 = beze změny).
///
/// Řeší se přiblížením celého webview, ne přepočtem stylů: rozvržení
/// aplikace míchá rem a pixely, takže samotná změna velikosti písma by
/// posunula text a nechala rámečky, kde byly. Přiblížení zvětší
/// všechno stejně.
pub fn zvetseni() -> f64 {
    precti().map_or(1.0, |t| zvetseni_z(&t))
}

fn zvetseni_z(text: &str) -> f64 {
    let Some((_, zbytek)) = text.split_once("\"ui_zoom\"") else {
        return 1.0;
    };
    let cislo: String = zbytek
        .trim_start()
        .trim_start_matches(':')
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    cislo
        .parse::<f64>()
        .ok()
        .filter(|z| (ZVETSENI_MIN..=ZVETSENI_MAX).contains(z))
        .unwrap_or(1.0)
}

/// Uloží zvětšení. Ostatní volby zůstanou.
pub fn save_zvetseni(z: f64) -> Result<(), String> {
    let z = z.clamp(ZVETSENI_MIN, ZVETSENI_MAX);
    let _g = zamek();
    let (accel, zapnuta, _) = nacti();
    zapis(&accel, zapnuta, z)
}

/// Přečte uloženou zkratku; když soubor není nebo je vadný, výchozí.
pub fn load() -> String {
    precti().map_or_else(|| DEFAULT.to_string(), |t| zkratka_z(&t))
}

fn zkratka_z(text: &str) -> String {
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

/// Rozumíme zápisu zkratky? Chyba je text pro uživatele.
///
/// Zvlášť od `save`, protože se ptá i ten, kdo zkratku nejdřív
/// registruje a ukládá až potom — neplatný zápis by jinak skončil
/// matoucí hláškou „zkratku používá jiný program".
pub fn over(accel: &str) -> Result<(), String> {
    match parse(accel) {
        Some(_) => Ok(()),
        None => Err(format!(
            "zkratce {accel:?} nerozumím — potřebuje modifikátor (Alt, Ctrl, Shift, Win) a klávesu"
        )),
    }
}

/// Uloží zkratku. Ověří se, že jí rozumíme — neplatný zápis by po
/// restartu znamenal, že zkratka nefunguje a nikdo neví proč.
pub fn save(accel: &str) -> Result<(), String> {
    over(accel)?;
    let _g = zamek();
    let (_, zapnuta, zv) = nacti();
    zapis(accel, zapnuta, zv)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Co se zapíše, to se zase přečte — všechny tři volby z jednoho textu.
    #[test]
    fn volby_projdou_zapisem_i_ctenim() {
        let t = text_souboru("Ctrl+Shift+F", false, 1.25);
        assert_eq!(zkratka_z(&t), "Ctrl+Shift+F");
        assert!(!zapnuta_z(&t));
        assert!((zvetseni_z(&t) - 1.25).abs() < 1e-9);

        let t = text_souboru("Alt+Space", true, 1.0);
        assert!(zapnuta_z(&t));
        assert_eq!(zkratka_z(&t), "Alt+Space");
    }

    // Prázdný soubor (zkrácený souběžným zápisem) dává výchozí hodnoty.
    // Právě proto se zapisuje atomicky a pod zámkem: tohle se pak
    // nesmí dostat zpátky do souboru.
    #[test]
    fn prazdny_text_znamena_vychozi() {
        assert_eq!(zkratka_z(""), DEFAULT);
        assert!(zapnuta_z(""));
        assert_eq!(zvetseni_z(""), 1.0);
    }

    #[test]
    fn neplatna_zkratka_neprojde() {
        assert!(over("Ctrl+Shift+F").is_ok());
        assert!(over("F").is_err());
        assert!(over("Ctrl+").is_err());
    }
}
