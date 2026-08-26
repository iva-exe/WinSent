//! Brána: WMI boolean se nesmí porovnávat s řetězcem „true".
//!
//! VARIANT typu VT_BOOL nese pravdu jako −1 (všechny bity nastavené),
//! takže přes převod na text z ní vyleze „-1". Porovnání s „true" tedy
//! nikdy nesedlo — TPM se hlásilo jako vypnuté a Defender jako bez
//! ochrany v reálném čase, přestože obojí bylo zapnuté.
//!
//! Měří se na `Win32_DiskPartition.Bootable`, protože ta je čitelná
//! i bez zvýšených práv a aspoň jeden oddíl zavádí systém vždycky.

fn main() {
    win_sys::wic::init_com_for_thread();
    let rows = win_sys::wmi::query(
        r"root\CIMV2",
        "SELECT Name, Bootable FROM Win32_DiskPartition",
        &["Name", "Bootable"],
    );
    if rows.is_empty() {
        println!("FAIL: Win32_DiskPartition nevrátil nic");
        std::process::exit(1);
    }

    let mut forms = std::collections::BTreeSet::new();
    let mut bootable = 0usize;
    for r in &rows {
        if let Some(v) = r.get("Bootable") {
            forms.insert(v.clone());
        }
        if win_sys::wmi::flag(r, "Bootable") == Some(true) {
            bootable += 1;
        }
    }
    println!("  oddílů {}, tvary hodnoty: {forms:?}", rows.len());
    println!("  zavádějících podle truthy(): {bootable}");

    // Kdyby WMI někdy začalo vracet „true", brána to POZNÁ a projde —
    // smyslem je hlídat, že se pravda pozná v tom tvaru, který přijde.
    if bootable == 0 {
        println!("FAIL: ani jeden oddíl nevyšel jako zavádějící — boolean se nečte správně");
        std::process::exit(1);
    }
    if !win_sys::wmi::truthy("-1") || !win_sys::wmi::truthy("1") || !win_sys::wmi::truthy("True") {
        println!("FAIL: truthy() nepozná některý tvar pravdy");
        std::process::exit(1);
    }
    if win_sys::wmi::truthy("0") || win_sys::wmi::truthy("") {
        println!("FAIL: truthy() bere nepravdu jako pravdu");
        std::process::exit(1);
    }
    println!("OK: WMI boolean se čte správně");
}
