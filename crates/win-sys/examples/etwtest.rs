//! Přímý test ETW cesty (elevovaně!): spustí realtime session, nechá
//! spadnout testovací proces s 0xC0000005 a vypíše zachycené události.
//! `cargo run -p win-sys --example etwtest`

fn main() {
    tracing_subscriber::fmt::init();
    let session = match win_sys::etw::start_realtime("syswatch-etwtest") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("start_realtime selhal: {e} (běžíš elevovaně?)");
            std::process::exit(1);
        }
    };
    let (rx, _consumer) = match win_sys::etw::consume("syswatch-etwtest") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("consume selhal: {e}");
            std::process::exit(1);
        }
    };
    println!("session běží, spouštím padající proces…");

    // cmd žije 2 s a skončí kódem 0xC0000005 (cmd exit umí jen dekadicky).
    let child = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Start-Sleep 2; [Environment]::Exit(-1073741819)",
        ])
        .spawn()
        .expect("spuštění testovacího procesu");
    let child_pid = child.id();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut starts = 0u32;
    let mut stops = 0u32;
    let mut crash_seen = false;
    while std::time::Instant::now() < deadline {
        while let Ok(ev) = rx.try_recv() {
            match ev {
                win_sys::etw::ProcEvent::Start { pid, parent, .. } => {
                    starts += 1;
                    if pid == child_pid {
                        println!("START pid={pid} parent={parent} (testovací proces)");
                    }
                }
                win_sys::etw::ProcEvent::Stop { pid, exit_code, .. } => {
                    stops += 1;
                    if pid == child_pid {
                        println!("STOP pid={pid} exit=0x{exit_code:08X}");
                        crash_seen = exit_code == 0xC000_0005;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        if crash_seen {
            break;
        }
    }
    println!(
        "celkem: {starts} startů, {stops} stopů; pád zachycen: {}",
        if crash_seen { "ANO" } else { "NE" }
    );
    drop(session);
    std::process::exit(if crash_seen { 0 } else { 2 });
}
