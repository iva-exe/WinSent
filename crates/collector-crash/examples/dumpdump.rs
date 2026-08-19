//! Rozbor výpisu paměti do textu. `cargo run -p collector-crash --example dumpdump -- <cesta>`
fn main() {
    let path = std::env::args().nth(1).unwrap_or_default();
    if path.is_empty() {
        println!("použití: dumpdump <cesta k .dmp>");
        return;
    }
    println!("{}", collector_crash::dump::describe_dump(std::path::Path::new(&path)));
}
