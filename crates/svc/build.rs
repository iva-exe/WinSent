//! Zabudování ikony a verzí do binárky služby.
//!
//! Bez ikony v PE resource se Winsent v seznamech (i ve vlastním UI)
//! ukazoval s generickou ikonou Windows — identita aplikace ji čte
//! přímo z resource, protože služba běží v session 0, kde shell
//! vrací default ikonu.

fn main() {
    if !cfg!(target_os = "windows") {
        return;
    }
    let ico = "../ui/src-tauri/icons/icon.ico";
    println!("cargo:rerun-if-changed={ico}");
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico);
    res.set("FileDescription", "Winsent — systémový monitor");
    res.set("ProductName", "Winsent");
    res.set("CompanyName", "Winsent");
    // Selhání kompilace resource nesmí zabít build (např. bez rc.exe) —
    // binárka pak jen nemá ikonu.
    if let Err(e) = res.compile() {
        println!("cargo:warning=ikonu se nepodařilo zabudovat: {e}");
    }
}
