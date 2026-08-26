//! Diagnostika: v jakém tvaru chodí WMI booleany přes náš get_prop.
//!
//! Vzniklo kvůli tomu, že TPM se hlásilo jako vypnuté a Defender jako
//! bez ochrany v reálném čase, přestože obojí bylo zapnuté. Podezření
//! padlo na porovnání s řetězcem „true".

fn main() {
    win_sys::wic::init_com_for_thread();

    for (ns, q, props) in [
        (
            r"root\CIMV2\Security\MicrosoftTpm",
            "SELECT IsEnabled_InitialValue, IsActivated_InitialValue, SpecVersion FROM Win32_Tpm",
            &[
                "IsEnabled_InitialValue",
                "IsActivated_InitialValue",
                "SpecVersion",
            ][..],
        ),
        (
            r"root\Microsoft\Windows\Defender",
            "SELECT RealTimeProtectionEnabled, AntispywareEnabled, AntivirusSignatureAge FROM MSFT_MpComputerStatus",
            &[
                "RealTimeProtectionEnabled",
                "AntispywareEnabled",
                "AntivirusSignatureAge",
            ][..],
        ),
        (
            r"root\CIMV2",
            "SELECT Name, Bootable, BootPartition FROM Win32_DiskPartition",
            &["Name", "Bootable", "BootPartition"][..],
        ),
    ] {
        println!("\n{ns}");
        let rows = win_sys::wmi::query(ns, q, props);
        if rows.is_empty() {
            println!("  (bez řádků — jmenný prostor není nebo dotaz selhal)");
        }
        for r in rows.iter().take(3) {
            for p in props {
                println!("  {p:<32} = {:?}", r.get(*p));
            }
            println!("  --");
        }
    }
}
