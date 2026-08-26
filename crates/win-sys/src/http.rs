//! Stahování přes WinHttp — součást Windows.
//!
//! Vědomě bez `reqwest`/`ureq`: TLS knihovna by nesla vlastní seznam
//! certifikátů. WinHttp používá systémové úložiště, takže věří přesně
//! tomu, čemu věří Windows.
//!
//! POZOR na dvojníka: `crates/installer/src/http.rs` je totéž. Není to
//! omyl — instalátor je jediný soubor, který dostane tester, a musí
//! zůstat bez závislosti na `win-sys` (ta táhne WMI, ETW, GPU a další
//! moduly, které instalátor nepotřebuje). Když se tady něco opraví,
//! patří to i tam.

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};

/// Chyby stahování — hlášky jdou rovnou testerovi do konzole.
#[derive(Debug)]
pub enum HttpError {
    Connect(String),
    Http { status: u32 },
    Read(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Connect(d) => write!(f, "nepodařilo se spojit se serverem ({d})"),
            HttpError::Http { status: 404 } => {
                write!(
                    f,
                    "soubor na serveru není (404) — vydavatel ho ještě nenahrál"
                )
            }
            HttpError::Http { status } => write!(f, "server odpověděl chybou {status}"),
            HttpError::Read(d) => write!(f, "přenos se přerušil ({d})"),
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Handle WinHttp, který se sám zavře. Bez tohohle by každá chybová
/// cesta musela handle uklízet ručně — a jednou by se zapomnělo.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: handle pochází z WinHttp* a zavírá se právě jednou.
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// Stáhne `https://{host}{path}` do paměti.
///
/// `progress` dostane počet přenesených bajtů — instalátor podle toho
/// kreslí ukazatel, ať tester nekouká na zamrzlé okno.
pub fn get(host: &str, path: &str, mut progress: impl FnMut(usize)) -> Result<Vec<u8>, HttpError> {
    let wagent = wide("Winsent");
    let whost = wide(host);
    let wpath = wide(path);
    let wget = wide("GET");

    // SAFETY: všechny handly drží Handle (uzavře je Drop); buffery
    // mají velikost hlášenou API.
    unsafe {
        let session = Handle(WinHttpOpen(
            PCWSTR(wagent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(HttpError::Connect("WinHttpOpen".into()));
        }

        let conn = Handle(WinHttpConnect(session.0, PCWSTR(whost.as_ptr()), 443, 0));
        if conn.0.is_null() {
            return Err(HttpError::Connect(format!("spojení na {host}")));
        }

        let req = Handle(WinHttpOpenRequest(
            conn.0,
            PCWSTR(wget.as_ptr()),
            PCWSTR(wpath.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null_mut(),
            WINHTTP_FLAG_SECURE,
        ));
        if req.0.is_null() {
            return Err(HttpError::Connect("WinHttpOpenRequest".into()));
        }

        if WinHttpSendRequest(req.0, None, None, 0, 0, 0).is_err() {
            return Err(HttpError::Connect("odeslání požadavku".into()));
        }
        if WinHttpReceiveResponse(req.0, std::ptr::null_mut()).is_err() {
            return Err(HttpError::Connect("čekání na odpověď".into()));
        }

        // Stavový kód: bez téhle kontroly bychom uložili HTML stránku
        // s chybou jako by to byla binárka.
        let mut status: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        let _ = WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut _ as *mut _),
            &mut len,
            std::ptr::null_mut(),
        );
        if status != 200 {
            return Err(HttpError::Http { status });
        }

        let mut out = Vec::new();
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(req.0, &mut avail).is_err() {
                return Err(HttpError::Read("dotaz na data".into()));
            }
            if avail == 0 {
                break;
            }
            let start = out.len();
            out.resize(start + avail as usize, 0);
            let mut read: u32 = 0;
            if WinHttpReadData(req.0, out[start..].as_mut_ptr() as *mut _, avail, &mut read)
                .is_err()
            {
                return Err(HttpError::Read("čtení dat".into()));
            }
            out.truncate(start + read as usize);
            progress(out.len());
        }
        Ok(out)
    }
}
