//! Ruční test pipe: `cargo run -p ipc --example ping`.
//! Vytiskne odpověď služby, nebo chybu, když neběží.

fn main() {
    match ipc::client::ping() {
        Ok(pong) => println!(
            "služba běží: protokol v{}, uptime {} s",
            pong.protocol_version, pong.uptime_s
        ),
        Err(e) => {
            eprintln!("ping selhal: {e}");
            std::process::exit(1);
        }
    }
}
