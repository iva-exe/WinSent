//! Ověření, že vlastní procesy patří pod jednu aplikaci „Winsent"
//! a že create_time (identita instance) přichází do UI:
//! `cargo run -p ipc --example winsentcheck`.

fn main() {
    let procs = match ipc::client::query_procs() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("query_procs selhal: {e}");
            std::process::exit(1);
        }
    };
    let own: Vec<_> = procs
        .iter()
        .filter(|p| p.identity_key == "app:winsent")
        .collect();
    println!("procesu pod app:winsent: {}", own.len());
    for p in &own {
        println!(
            "  {} (pid {}, create_time {}) app={}",
            p.name, p.pid, p.create_time, p.app_name
        );
    }
    // create_time musí být nenulový u všech (potřeba pro T1 kill).
    let missing = procs.iter().filter(|p| p.create_time == 0).count();
    println!("procesu bez create_time: {missing}");

    match ipc::client::query_cleanup() {
        Ok((indexing, _, _)) => {
            for (l, n, done, err) in indexing {
                println!("index {l}: {n} zaznamu, hotovo={done}, chyba={err:?}");
            }
        }
        Err(e) => println!("query_cleanup: {e}"),
    }
}
