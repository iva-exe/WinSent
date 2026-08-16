//! Zvednutí zastavené služby z UI.
//!
//! Služba se může ocitnout v klidu i bez pádu — stačí ruční „stop"
//! ve Správci služeb nebo přerušená aktualizace. Automatické akce
//! po pádu (`sc failure`) se na takové zastavení **nevztahují**:
//! Windows je považují za záměr správce a samy nic nespouští. Bez
//! zásahu tak aplikace mlčí a jediné, co uživatel vidí, je „služba
//! neběží".
//!
//! UI běží pod běžným uživatelem (SPEC 2.1) a na správu služeb
//! nedosáhne. Nesahá proto na SCM samo — pustí instalátor, který si
//! o práva správce řekne svým manifestem a Windows zobrazí obvyklou
//! výzvu. Spuštění bez parametrů je zároveň oprava: doplní chybějící
//! soubory, srovná registraci služby a nastartuje ji.
//!
//! Držíme tím linii celé aplikace: my ukážeme problém a cestu ven,
//! spoušť mačká uživatel.

use windows::core::HSTRING;
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Chyby opravy.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("instalátor jsem nenašel ({0}) — spusť WinsentSetup.exe ručně")]
    NotFound(String),
    #[error("instalátor se nepodařilo spustit: {0}")]
    Spawn(String),
}

/// Kopie instalátoru, kterou si sám položil vedle aplikace.
fn setup_path() -> std::path::PathBuf {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    std::path::PathBuf::from(pf)
        .join("Winsent")
        .join("WinsentSetup.exe")
}

/// Pustí instalátor v opravném režimu a hned se vrátí. Na výsledek se
/// nečeká — pozná se sám na sobě: až služba naběhne, ukazatel v hlavičce
/// se rozsvítí při nejbližším pollu.
///
/// Přes `ShellExecuteExW`, ne `CreateProcess`: jen ten respektuje
/// manifest s požadavkem na práva správce, takže výzvu UAC zobrazí
/// Windows samy a v obvyklé podobě.
pub fn launch() -> Result<(), Error> {
    let exe = setup_path();
    if !exe.is_file() {
        return Err(Error::NotFound(exe.display().to_string()));
    }
    let wexe = HSTRING::from(exe.to_string_lossy().as_ref());
    let wdir = exe
        .parent()
        .map(|p| HSTRING::from(p.to_string_lossy().as_ref()));

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // NOASYNC → volání dokončí práci dřív, než se vrátí.
        fMask: SEE_MASK_NOASYNC,
        lpFile: windows::core::PCWSTR(wexe.as_ptr()),
        lpDirectory: wdir
            .as_ref()
            .map(|d| windows::core::PCWSTR(d.as_ptr()))
            .unwrap_or(windows::core::PCWSTR::null()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    // SAFETY: struktura je vyplněná dle kontraktu API a řetězce žijí
    // po celou dobu volání.
    unsafe { ShellExecuteExW(&mut info).map_err(|e| Error::Spawn(format!("{e}")))? };
    Ok(())
}
