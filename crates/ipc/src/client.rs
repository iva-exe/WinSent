//! Named pipe klient — používá UI (a testy) k dotazům na službu.
//!
//! Klientská strana pipe jde otevřít přes std::fs — server instanci
//! vytvořil, my ji jen otevíráme jako soubor pro čtení+zápis.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::time::Duration;

use core_types::ipc::{Request, Response, PROTOCOL_VERSION};

use crate::{frame, Error, PIPE_NAME};

/// Výsledek úspěšného pingu služby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PongInfo {
    pub protocol_version: u32,
    pub uptime_s: u64,
}

/// Otevře spojení na službu. `Error::NotAvailable` = služba neběží.
/// ERROR_PIPE_BUSY (všechny instance obsazené) řeší krátký retry.
pub fn connect() -> Result<File, Error> {
    const ERROR_PIPE_BUSY: i32 = 231;
    const ATTEMPTS: u32 = 3;

    for attempt in 0..ATTEMPTS {
        match OpenOptions::new().read(true).write(true).open(PIPE_NAME) {
            Ok(f) => return Ok(f),
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(Error::NotAvailable),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempt + 1 < ATTEMPTS => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::NotAvailable)
}

/// Pošle požadavek a přečte jednu odpověď.
pub fn request(stream: &mut File, req: &Request) -> Result<Response, Error> {
    frame::write_msg(stream, req)?;
    frame::read_msg(stream)?.ok_or(Error::NotAvailable)
}

/// „Žiješ?“ — jedno spojení, jeden ping, jedna odpověď.
pub fn ping() -> Result<PongInfo, Error> {
    let mut stream = connect()?;
    match request(
        &mut stream,
        &Request::Ping {
            protocol_version: PROTOCOL_VERSION,
        },
    )? {
        Response::Pong {
            protocol_version,
            uptime_s,
        } => Ok(PongInfo {
            protocol_version,
            uptime_s,
        }),
        Response::Error { message } => Err(Error::Remote { message }),
    }
}
