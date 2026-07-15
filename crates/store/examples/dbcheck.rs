//! Kontrola obsahu DB: `cargo run -p store --example dbcheck`.
//! Vypíše počty řádků vzorků a rozsah časů (readonly přístup).

use rusqlite::OpenFlags;

fn main() {
    let path = store::db_path().expect("cesta k DB");
    let conn = rusqlite::Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("otevření DB readonly");

    let ver: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let sys: i64 = conn
        .query_row("SELECT COUNT(*) FROM system_1s", [], |r| r.get(0))
        .unwrap_or(-1);
    let smp: i64 = conn
        .query_row("SELECT COUNT(*) FROM sample_1s", [], |r| r.get(0))
        .unwrap_or(-1);
    let span: (Option<i64>, Option<i64>) = conn
        .query_row("SELECT MIN(ts), MAX(ts) FROM system_1s", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap_or((None, None));

    println!(
        "schema v{ver}  system_1s={sys}  sample_1s={smp}  ts {}–{}",
        span.0.unwrap_or(0),
        span.1.unwrap_or(0)
    );
}
