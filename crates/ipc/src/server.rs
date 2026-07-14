//! Named pipe server — běží ve službě.
//!
//! Model: akceptační smyčka vytváří instance pipe a pro každé připojení
//! spouští obslužné vlákno (request→response, dokud klient nezavře).
//! DACL: SYSTEM a Administrators plný přístup, interaktivní uživatelé
//! čtení+zápis (SPEC kap. 2.1). `PIPE_REJECT_REMOTE_CLIENTS` — pipe je
//! jen lokální útočná plocha, ne síťová (SPEC kap. 21 bod 10).

use std::fs::File;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use core_types::ipc::{Request, Response};

use crate::{frame, Error, MAX_FRAME_LEN, PIPE_NAME};

/// SDDL pro DACL pipe: P = protected (žádná dědičnost), SYSTEM (SY) a
/// Administrators (BA) generic all, interaktivní uživatelé (IU) generic
/// read+write. Nikdo jiný pipe neotevře.
const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

/// Obsluha jednoho požadavku. Služba dodá funkci, server ji volá pro
/// každý přijatý Request — server sám protokolu nerozumí.
pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

/// Security descriptor pro pipe — drží alokaci z LocalAlloc po dobu
/// života serveru, Drop ji uvolní.
struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        // SAFETY: descriptor pochází z ConvertStringSecurityDescriptor…,
        // který alokuje přes LocalAlloc; párové uvolnění je LocalFree.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
        }
    }
}

/// Přeloží SDDL na security descriptor.
fn build_pipe_security() -> Result<PipeSecurity, Error> {
    let sddl: Vec<u16> = PIPE_SDDL.encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: výstupní ukazatel žije po celou dobu volání.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            1, // SDDL_REVISION_1
            &mut descriptor,
            None,
        )
    }
    .map_err(|source| Error::Win32 {
        call: "ConvertStringSecurityDescriptorToSecurityDescriptorW",
        source,
    })?;
    Ok(PipeSecurity { descriptor })
}

/// Vytvoří novou instanci pipe připravenou na jednoho klienta.
fn create_instance(sec: &PipeSecurity) -> Result<HANDLE, Error> {
    let name: Vec<u16> = PIPE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sec.descriptor.0,
        bInheritHandle: false.into(),
    };
    // SAFETY: name i sa žijí po dobu volání; handle vlastníme my.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            MAX_FRAME_LEN,
            MAX_FRAME_LEN,
            0,
            Some(&sa),
        )
    };
    if handle.is_invalid() {
        return Err(Error::Win32 {
            call: "CreateNamedPipeW",
            source: windows::core::Error::from_win32(),
        });
    }
    Ok(handle)
}

/// Akceptační smyčka serveru. Blokuje, dokud `stop` nenastaví služba;
/// probuzení z blokujícího čekání zajistí `wake()`.
pub fn run(handler: Handler, stop: Arc<AtomicBool>) -> Result<(), Error> {
    let sec = build_pipe_security()?;
    tracing::info!(pipe = PIPE_NAME, "IPC server naslouchá");

    while !stop.load(Ordering::SeqCst) {
        let pipe = create_instance(&sec)?;

        // Čekání na klienta. ERROR_PIPE_CONNECTED = klient se stihl
        // připojit mezi vytvořením a čekáním — to je úspěch.
        // SAFETY: pipe je platný handle z create_instance.
        let connected = unsafe { ConnectNamedPipe(pipe, None) };
        if let Err(e) = connected {
            if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                // SAFETY: handle vlastníme, File ho korektně zavře.
                drop(unsafe { File::from_raw_handle(pipe.0 as _) });
                tracing::warn!(error = %e, "ConnectNamedPipe selhal");
                continue;
            }
        }

        // Vlastnictví handle přechází na File (zavře ho při dropu).
        // SAFETY: pipe je platný, nikdo jiný ho nezavírá.
        let stream = unsafe { File::from_raw_handle(pipe.0 as _) };

        if stop.load(Ordering::SeqCst) {
            break; // probuzení dummy klientem při shutdownu
        }

        let handler = Arc::clone(&handler);
        std::thread::Builder::new()
            .name("ipc-conn".into())
            .spawn(move || handle_connection(stream, handler))
            .map_err(Error::Io)?;
    }

    tracing::info!("IPC server ukončen");
    Ok(())
}

/// Probudí akceptační smyčku zablokovanou v ConnectNamedPipe — připojí
/// se jako dummy klient. Volat po nastavení stop flagu.
pub fn wake() {
    let _ = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_NAME);
}

/// Obsluha jednoho spojení: čte rámce, volá handler, odpovídá.
/// Chyba protokolu se klientovi ohlásí (nic neselhává mlčky) a spojení
/// se zavře.
fn handle_connection(mut stream: File, handler: Handler) {
    loop {
        match frame::read_msg::<_, Request>(&mut stream) {
            Ok(Some(req)) => {
                let resp = handler(req);
                if let Err(e) = frame::write_msg(&mut stream, &resp) {
                    tracing::warn!(error = %e, "zápis odpovědi selhal");
                    break;
                }
            }
            Ok(None) => break, // klient čistě zavřel
            Err(e) => {
                tracing::warn!(error = %e, "vadný rámec od klienta");
                let _ = frame::write_msg(
                    &mut stream,
                    &Response::Error {
                        message: e.to_string(),
                    },
                );
                break;
            }
        }
    }

    // Korektní rozloučení: doručit zbylé bajty, odpojit instanci.
    // SAFETY: handle patří File, jen ho flushneme/odpojíme před dropem.
    unsafe {
        let h = HANDLE(stream.as_raw_handle() as _);
        let _ = FlushFileBuffers(h);
        let _ = DisconnectNamedPipe(h);
    }
}
