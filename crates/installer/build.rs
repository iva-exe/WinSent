//! Ikona + manifest instalátoru.
//!
//! Manifest s `requireAdministrator` je důležitý: Windows si o práva
//! řeknou samy hned při spuštění (výzva UAC). Tester tak nemusí nic
//! vědět o „spustit jako správce" — jen dvakrát klikne.

fn main() {
    if !cfg!(target_os = "windows") {
        return;
    }
    let ico = "../ui/src-tauri/icons/icon.ico";
    println!("cargo:rerun-if-changed={ico}");
    println!("cargo:rerun-if-changed=installer.manifest");

    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico);
    res.set_manifest_file("installer.manifest");
    res.set("FileDescription", "Winsent — instalace");
    res.set("ProductName", "Winsent");
    res.set("CompanyName", "Winsent");
    // Bez rc.exe se resource nezkompiluje — instalátor pak jen nemá
    // ikonu a manifest; build kvůli tomu padat nesmí.
    if let Err(e) = res.compile() {
        println!("cargo:warning=resource se nepodařilo zabudovat: {e}");
    }
}
