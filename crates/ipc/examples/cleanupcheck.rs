//! Ruční test v4E: `cargo run -p ipc --example cleanupcheck`.

fn main() {
    match ipc::client::query_cleanup() {
        Ok((indexing, running, report)) => {
            for (l, n, done, err) in &indexing {
                println!("index {l}: {n} zaznamu, hotovo={done}, chyba={err:?}");
            }
            println!("analyza bezi: {running}");
            if let Some(r) = report {
                let waste: u64 = r.dups.iter().map(|(s, p)| s * (p.len() as u64 - 1)).sum();
                println!(
                    "report: {} dup skupin ({:.1} MB navic), {} nulovych, {} junk cest",
                    r.dups.len(),
                    waste as f64 / 1e6,
                    r.zero_byte.len(),
                    r.junk.len()
                );
                for (size, paths) in r.dups.iter().take(3) {
                    println!(
                        "  dup {:.1} MB x{}: {}",
                        *size as f64 / 1e6,
                        paths.len(),
                        paths[0]
                    );
                }
                for (p, s) in &r.junk {
                    println!("  junk {:.1} MB  {}", *s as f64 / 1e6, p);
                }
                println!(
                    "nejvetsi: {} slozek, {} souboru",
                    r.big_dirs.len(),
                    r.big_files.len()
                );
                for (l, p, s) in r.big_dirs.iter().take(4) {
                    println!("  dir  [{l}] {:.1} GB  {}", *s as f64 / 1e9, p);
                }
                for (l, p, s) in r.big_files.iter().take(4) {
                    println!("  file [{l}] {:.1} GB  {}", *s as f64 / 1e9, p);
                }
            } else {
                println!("report: jeste neni");
            }
        }
        Err(e) => {
            eprintln!("query_cleanup selhal: {e}");
            std::process::exit(1);
        }
    }
}
