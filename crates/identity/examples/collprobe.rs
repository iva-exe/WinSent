//! Diagnostika: co dělá kaskáda s cestou uvnitř sběrného adresáře.
//!
//! Naměřeno na vývojovém stroji: Minecraft Launcher má
//! `InstallLocation = D:\hry\` a v témž adresáři leží Genshin Impact,
//! Star Rail a Star Stable. Bez zahození nadřazených prefixů se všechny
//! hlásily jako aplikace „Minecraft Launcher" s confidence Exact.

fn main() {
    let tables = identity::load_tables();

    println!("nejkratší instalační adresáře v tabulce:");
    let mut podle_delky: Vec<&identity::UninstallEntry> = tables.uninstall.iter().collect();
    podle_delky.sort_by_key(|e| e.loc.len());
    for e in podle_delky.iter().take(8) {
        println!("  {:<40} {}{}", e.loc, e.name, if e.collection { "  [sběrný]" } else { "" });
    }

    println!("\nkontrolní cesty:");
    for cesta in [
        r"D:\hry\Star Rail Games\StarRail.exe",
        r"D:\hry\Genshin Impact\crashreport.exe",
        r"D:\hry\Star Stable Online\PXStudioRuntimeMMO.exe",
        r"D:\hry\Launcher\Launcher.exe",
        r"D:\hry\MinecraftLauncher.exe",
    ] {
        let id = identity::cascade::resolve(0, "test.exe", Some(cesta), &tables);
        println!("  {cesta:<52} → {} [{}]", id.app_name, id.identity_key);
    }
}
