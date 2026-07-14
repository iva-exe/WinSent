//! Vývojový režim `--console` — démon jako obyčejný (elevovaný)
//! konzolový proces. Logy jdou rovnou na stdout, Ctrl+C ukončuje čistě.
//! Bez tohoto režimu by každá změna kódu znamenala přeinstalaci služby.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::daemon;

/// Chyby konzolového režimu.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Daemon(#[from] daemon::Error),
    #[error("nelze registrovat Ctrl+C handler: {0}")]
    CtrlC(#[from] ctrlc::Error),
}

/// Spustí démona v popředí a blokuje do Ctrl+C.
pub fn run() -> Result<(), Error> {
    // Logy na stdout; úroveň řídí RUST_LOG, default info.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let stop = Arc::new(AtomicBool::new(false));
    let stop_handler = Arc::clone(&stop);
    ctrlc::set_handler(move || {
        // Druhé Ctrl+C během úklidu nechá proces doběhnout — flag už je nastavený.
        stop_handler.store(true, Ordering::SeqCst);
        ipc::server::wake();
    })?;

    tracing::info!("konzolový režim — ukončení Ctrl+C");
    daemon::run(stop)?;
    Ok(())
}
