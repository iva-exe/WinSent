//! Výpis auditu: `cargo run -p ipc --example auditcheck`.

fn main() {
    match ipc::client::query_audit(20) {
        Ok(rows) => {
            for r in rows {
                println!(
                    "[{}] {:14} {} | {} | {} | {:?} | deny={:?}",
                    r.ts, r.action, r.class, r.target, r.verdict, r.outcome, r.deny_reason
                );
            }
        }
        Err(e) => eprintln!("query_audit selhal: {e}"),
    }
}
