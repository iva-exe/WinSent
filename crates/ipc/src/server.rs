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

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    FlushFileBuffers, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use core_types::ipc::{Request, Response};

use crate::{frame, Error, MAX_FRAME_LEN, PIPE_NAME};

/// SDDL šablona DACL pipe: P = protected (žádná dědičnost), SYSTEM (SY)
/// a Administrators (BA) plný přístup, `{server}` = SID identity, pod
/// kterou server právě běží (LocalSystem v produkci, vývojář v --console)
/// — server musí sám sobě dovolit vytvářet další instance. Interaktivní
/// uživatelé (IU) jen čtení+zápis BEZ práva FILE_CREATE_PIPE_INSTANCE
/// (0x4) — jinak by si kdokoli mohl vytvořit vlastní instanci naší pipe
/// a odposlouchávat požadavky (pipe je útočná plocha, SPEC kap. 21 bod 10).
/// 0x12019b = (FILE_GENERIC_READ | FILE_GENERIC_WRITE) & ~FILE_CREATE_PIPE_INSTANCE
const PIPE_SDDL_TEMPLATE: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{server})(A;;0x12019b;;;IU)";

/// SID identity aktuálního procesu (string forma pro SDDL).
fn current_process_sid() -> Result<String, Error> {
    // SAFETY: standardní sekvence token → TOKEN_USER → SID string;
    // všechny buffery vlastníme, handle i LocalAlloc řetězec uvolňujeme.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(|source| {
            Error::Win32 {
                call: "OpenProcessToken",
                source,
            }
        })?;

        // První volání jen zjistí potřebnou délku.
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let info = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr() as *mut _),
            len,
            &mut len,
        );
        let _ = CloseHandle(token);
        info.map_err(|source| Error::Win32 {
            call: "GetTokenInformation(TokenUser)",
            source,
        })?;

        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid_str).map_err(|source| Error::Win32 {
            call: "ConvertSidToStringSidW",
            source,
        })?;
        let sid = sid_str.to_string().unwrap_or_default();
        let _ = LocalFree(Some(HLOCAL(sid_str.0 as _)));
        Ok(sid)
    }
}

/// Obsluha jednoho požadavku. Služba dodá funkci, server ji volá pro
/// každý přijatý Request — server sám protokolu nerozumí.
pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

/// Security descriptor pro pipe — drží alokaci z LocalAlloc po dobu
/// života serveru, Drop ji uvolní.
struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
}

// SAFETY: descriptor je vlastněná LocalAlloc paměť bez vazby na vlákno;
// přesun do server vlákna je bezpečný, přístup je výhradně read-only.
unsafe impl Send for PipeSecurity {}

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
    let sddl_string = PIPE_SDDL_TEMPLATE.replace("{server}", &current_process_sid()?);
    let sddl: Vec<u16> = sddl_string
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
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
///
/// První instance nese FILE_FLAG_FIRST_PIPE_INSTANCE: když pipe už
/// existuje (běžící služba vs. vývojový --console démon), start selže
/// s jasnou chybou místo tichého souboje dvou serverů o klienty.
fn create_instance(sec: &PipeSecurity, first: bool) -> Result<HANDLE, Error> {
    let name: Vec<u16> = PIPE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sec.descriptor.0,
        bInheritHandle: false.into(),
    };
    let mut open_mode = PIPE_ACCESS_DUPLEX;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: name i sa žijí po dobu volání; handle vlastníme my.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            MAX_FRAME_LEN,
            MAX_FRAME_LEN,
            0,
            Some(&sa),
        )
    };
    if handle.is_invalid() {
        let source = windows::core::Error::from_win32();
        if first && source.code() == ERROR_ACCESS_DENIED.to_hresult() {
            return Err(Error::PipeAlreadyExists);
        }
        return Err(Error::Win32 {
            call: "CreateNamedPipeW",
            source,
        });
    }
    Ok(handle)
}

/// Server navázaný na pipe: DACL + první instance (exkluzivní vlastnictví
/// jména). Vzniká synchronně přes `bind()`, takže kolize s jiným démonem
/// selže hned při startu, ne až v obslužném vlákně.
pub struct Bound {
    sec: PipeSecurity,
    // HANDLE jako isize, aby byl Bound Send (surový HANDLE není).
    first_instance: isize,
}

/// Naváže server na pipe: vytvoří DACL a první instanci. Když jméno už
/// vlastní jiný proces, vrátí `PipeAlreadyExists`.
pub fn bind() -> Result<Bound, Error> {
    let sec = build_pipe_security()?;
    let first = create_instance(&sec, true)?;
    Ok(Bound {
        sec,
        first_instance: first.0 as isize,
    })
}

/// Akceptační smyčka serveru. Blokuje, dokud `stop` nenastaví služba;
/// probuzení z blokujícího čekání zajistí `wake()`.
pub fn run(bound: Bound, handler: Handler, stop: Arc<AtomicBool>) -> Result<(), Error> {
    let sec = bound.sec;
    let mut pending = Some(HANDLE(bound.first_instance as _));
    tracing::info!(pipe = PIPE_NAME, "IPC server naslouchá");

    while !stop.load(Ordering::SeqCst) {
        // První kolo použije instanci z bind(), další se vytváří průběžně.
        let pipe = match pending.take() {
            Some(h) => h,
            None => create_instance(&sec, false)?,
        };

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
