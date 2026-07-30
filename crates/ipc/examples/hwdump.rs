//! Dump hardwarového přehledu jako JSON — pro náhled UI mimo Tauri.
//! `cargo run -p ipc --example hwdump > hw.json`

fn main() {
    match ipc::client::query_hardware() {
        Ok(h) => println!("{}", serde_json::to_string(&h).expect("json")),
        Err(e) => eprintln!("query_hardware selhal: {e}"),
    }
}
