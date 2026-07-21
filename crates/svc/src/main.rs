//! syswatch — démon systémového monitoru.
//!
//! Dva režimy běhu (ROADMAP v0):
//!   `syswatch.exe --service` → Windows služba pod SCM (produkce)
//!   `syswatch.exe --console` → konzolový proces se stdout logy (vývoj)

mod config;
mod console;
mod daemon;
mod incidents;
mod integrity;
mod service;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let code = match args.first().map(String::as_str) {
        Some("--service") => match service::run() {
            Ok(()) => 0,
            Err(e) => {
                // SCM dispatcher selhal ještě před startem služby —
                // stderr je jediné místo, kam to jde říct.
                eprintln!("chyba service dispatcheru: {e}");
                1
            }
        },
        Some("--console") => match console::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("chyba démona: {e}");
                1
            }
        },
        Some("--version") => {
            println!("syswatch {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!(
                "použití: syswatch.exe --service | --console | --version\n\
                 --service  běh jako Windows služba (spouští SCM, ne ručně)\n\
                 --console  vývojový režim, logy na stdout (vyžaduje admin)"
            );
            2
        }
    };
    std::process::exit(code);
}
