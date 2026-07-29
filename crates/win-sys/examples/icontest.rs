//! Porovnání zdrojů ikon: PE resource vs. shell.
//! `cargo run -p win-sys --example icontest`

fn main() {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let candidates = [
        format!(r"{root}\explorer.exe"),
        format!(r"{root}\System32\notepad.exe"),
        r"C:\Users\IVA\Desktop\Projekty\WinSent\target\debug\syswatch.exe".to_string(),
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".to_string(),
    ];
    for path in candidates {
        if !std::path::Path::new(&path).exists() {
            println!("— {path} (neexistuje)");
            continue;
        }
        let pe = win_sys::icon::extract_pe(&path).map(|i| (i.w, i.h, checksum(&i.rgba)));
        let all = win_sys::icon::extract(&path).map(|i| (i.w, i.h, checksum(&i.rgba)));
        println!("{path}\n   PE: {pe:?}\n  vše: {all:?}");
    }
}

/// Hrubý kontrolní součet pixelů — dvě různé ikony dají různá čísla,
/// takže poznáme, jestli shell vracel jednu generickou pro všechno.
fn checksum(rgba: &[u8]) -> u64 {
    rgba.iter().enumerate().fold(0u64, |acc, (i, b)| {
        acc.wrapping_add((*b as u64).wrapping_mul(i as u64 % 977 + 1))
    })
}
