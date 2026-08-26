//! Ikona, manifest a informace o verzi instalátoru.
//!
//! Manifest s `requireAdministrator` je důležitý: Windows si o práva
//! řeknou samy hned při spuštění (výzva UAC). Tester tak nemusí nic
//! vědět o „spustit jako správce" — jen dvakrát klikne.
//!
//! Kompletní blok VERSIONINFO tu není kosmetika. Heuristiky antivirů
//! berou „malý .exe bez jména výrobce, bez popisu a bez verze, který
//! stahuje a spouští další .exe" jako typický profil downloaderu.
//! Vyplněná metadata ten profil rozbíjejí — a hlavně jsou pravdivá:
//! uživatel ve vlastnostech souboru uvidí, co to je a od koho.
//!
//! Na obrazovku „Windows ochránily váš počítač" (SmartScreen) to samo
//! nestačí. Ta se ukazuje u každého programu, který nemá podpis
//! a reputaci; jediné, co ji spolehlivě odstraní, je podepsat binárku
//! certifikátem pro podpis kódu. Viz README.

fn main() {
    if !cfg!(target_os = "windows") {
        return;
    }
    let ico = "../ui/src-tauri/icons/icon.ico";
    println!("cargo:rerun-if-changed={ico}");
    println!("cargo:rerun-if-changed=installer.manifest");

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".into());

    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico);
    res.set_manifest_file("installer.manifest");
    res.set("FileDescription", "Winsent — instalace a aktualizace");
    res.set("ProductName", "Winsent");
    res.set("CompanyName", "Winsent");
    res.set("InternalName", "WinsentSetup");
    res.set("OriginalFilename", "WinsentSetup.exe");
    res.set("LegalCopyright", "© Winsent");
    res.set("Comments", "Nainstaluje monitor Windows Winsent a jeho službu.");
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    // Bez rc.exe se resource nezkompiluje — instalátor pak jen nemá
    // ikonu a manifest; build kvůli tomu padat nesmí.
    if let Err(e) = res.compile() {
        println!("cargo:warning=resource se nepodařilo zabudovat: {e}");
    }
}
