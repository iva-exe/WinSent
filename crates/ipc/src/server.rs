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
///
/// Druhý argument říká, kdo je na druhém konci. Pipe smí otevřít každý
/// interaktivní uživatel (DACL výše) a dřív handler o volajícím nevěděl
/// nic — požadavek, který nechá službu jako SYSTEM zapisovat do cesty
/// zvolené klientem (přesun databáze), tak mohl poslat i neprivilegovaný
/// proces. Rozhodnutí, co smí kdo, zůstává na službě; server jen dodá
/// identitu.
pub type Handler = Arc<dyn Fn(Request, &ClientInfo) -> Response + Send + Sync>;

/// Identita klienta jednoho spojení.
///
/// Vyhodnocuje se líně, až když se na ni handler zeptá: drtivá většina
/// požadavků jsou čtení, kterým je jedno, kdo se ptá, a impersonace na
/// každé spojení by byla práce navíc. Pozdní vyhodnocení je zároveň
/// podmínkou: ImpersonateNamedPipeClient funguje až poté, co se z pipe
/// něco přečetlo — a handler dostává řízení vždy až po přečtení požadavku.
pub struct ClientInfo {
    // HANDLE jako isize — spojení patří obslužnému vláknu, tohle je jen
    // výpůjčka na dobu volání handleru.
    pipe: isize,
    admin: std::sync::OnceLock<bool>,
}

impl ClientInfo {
    /// Je klient elevovaný správce?
    ///
    /// CheckTokenMembership s tokenem klienta vrací TRUE jen tehdy, když
    /// je skupina BUILTIN\Administrators v tokenu POVOLENÁ. U správce
    /// s UAC bez elevace je jen „deny only", takže neelevovaný proces
    /// (a s ním malware v relaci uživatele) neprojde; elevovaný proces
    /// nebo správce s vypnutým UAC ano. Cokoli selže → false: pochybnost
    /// nesmí znamenat oprávnění.
    pub fn is_elevated_admin(&self) -> bool {
        *self
            .admin
            .get_or_init(|| client_is_elevated_admin(HANDLE(self.pipe as _)))
    }
}

/// Vrátí vlákno do identity služby, ať se z funkce odchází jakkoli.
///
/// Vlákno, které by zůstalo v identitě klienta, by další požadavky
/// obsluhovalo s jeho právy — a hlavně by to nikdo nepoznal. Když se
/// návrat nepovede (podle dokumentace se to stát nemá), jediná bezpečná
/// cesta je proces ukončit: SCM ho po pádu nastartuje znovu.
struct ZpetDoSebe;

impl Drop for ZpetDoSebe {
    fn drop(&mut self) {
        // SAFETY: bez argumentů; ukončuje impersonaci aktuálního vlákna.
        if unsafe { windows::Win32::Security::RevertToSelf() }.is_err() {
            std::process::abort();
        }
    }
}

/// Impersonuje klienta pipe a zjistí, jestli má povolenou skupinu správců.
fn client_is_elevated_admin(pipe: HANDLE) -> bool {
    use windows::Win32::Security::{
        CheckTokenMembership, CreateWellKnownSid, WinBuiltinAdministratorsSid, PSID,
    };
    use windows::Win32::System::Pipes::ImpersonateNamedPipeClient;
    // SECURITY_MAX_SID_SIZE = 68 B; SID správců je kratší.
    let mut sid = [0u8; 68];
    let mut sid_len = sid.len() as u32;
    // SAFETY: buffer pro SID vlastníme a jeho délku předáváme; SID se
    // tvoří ještě PŘED impersonací, ať v identitě klienta běží co nejméně.
    let sid_ok = unsafe {
        CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            None,
            Some(PSID(sid.as_mut_ptr() as _)),
            &mut sid_len,
        )
    };
    if sid_ok.is_err() {
        return false;
    }
    // SAFETY: pipe je platný handle spojení, ze kterého se už četlo.
    if unsafe { ImpersonateNamedPipeClient(pipe) }.is_err() {
        return false;
    }
    let _zpet = ZpetDoSebe;
    let mut member = windows::core::BOOL(0);
    // SAFETY: None = token impersonace aktuálního vlákna, tedy klienta.
    let ok = unsafe { CheckTokenMembership(None, PSID(sid.as_mut_ptr() as _), &mut member) };
    ok.is_ok() && member.as_bool()
}

/// Kolik spojení smí být obsluhováno současně — celkem.
///
/// Dřív bez limitu: každé spojení dostalo vlastní vlákno, takže kdokoli
/// z interaktivní relace mohl otevřít tisíce spojení a službu vyčerpat.
/// Tenhle strop chrání jen paměť; o férovost se stará MAX_CONNS_NA_PROCES.
const MAX_CONNS: usize = 256;

/// Kolik spojení smí současně držet jeden klientský proces.
///
/// Se samotným globálním stropem (dřív 64) stačilo jednomu procesu
/// otevřít 64 spojení a na každém jednou za < 30 s poslat levný dotaz:
/// nečinnost ho nikdy neodpojila a každé nové spojení UI se hned
/// zavřelo — UI hlásilo „služba neběží". Strop na proces nechá cizímu
/// procesu jen jeho díl. UI posílá souběžně tolik dotazů, kolik má
/// Tauri pracovních vláken (≈ počet jader), takže 64 je rezerva.
/// PID klienta není bezpečnostní hranice (útočník může spustit víc
/// procesů) — jde o to, aby jeden zlobivý proces nevyřadil UI.
const MAX_CONNS_NA_PROCES: usize = 64;

/// Jak dlouho smí spojení čekat na klienta — na další požadavek, nebo
/// na to, až si klient přečte odpověď.
///
/// Bez stropu drželo vlákno klienta, který se připojil a nic neposlal
/// (nebo odpověď nečetl a FlushFileBuffers čekal), napořád. S limitem
/// spojení by pak 64 takových klientů zablokovalo UI úplně. Doba běhu
/// handleru se do stropu nepočítá — dlouhé dotazy (velikosti aplikací)
/// běží mezi čtením a zápisem a nikdo je nepřerušuje.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Spojení, které právě čeká na klienta, a od kdy.
struct Cekajici {
    /// Handle vlákna s právem THREAD_TERMINATE (CancelSynchronousIo).
    vlakno: isize,
    ceka_od: Option<std::time::Instant>,
}

type Hlidane = Arc<std::sync::Mutex<std::collections::HashMap<u64, Cekajici>>>;

/// Registrace obslužného vlákna u hlídače nečinnosti; Drop ji zruší.
struct Hlidani {
    hlidane: Hlidane,
    id: u64,
}

impl Hlidani {
    fn new(hlidane: &Hlidane, id: u64) -> Hlidani {
        use windows::Win32::System::Threading::{GetCurrentThreadId, OpenThread, THREAD_TERMINATE};
        // SAFETY: otevření handle vlastního vlákna; zavírá ho Drop.
        let vlakno = unsafe { OpenThread(THREAD_TERMINATE, false, GetCurrentThreadId()) }
            .map(|h| h.0 as isize)
            .unwrap_or(0);
        if vlakno != 0 {
            hlidane.lock().expect("ipc hlidane lock").insert(
                id,
                Cekajici {
                    vlakno,
                    ceka_od: None,
                },
            );
        } else {
            tracing::warn!("spojení bez hlídání nečinnosti (OpenThread selhal)");
        }
        Hlidani {
            hlidane: Arc::clone(hlidane),
            id,
        }
    }

    /// Provede I/O s klientem pod dohledem hlídače.
    ///
    /// Stav se přepíná pod zámkem, který hlídač drží i při volání
    /// CancelSynchronousIo. Mezi koncem I/O a vynulováním stavu se
    /// žádné jiné I/O nedělá, takže zrušení nikdy nezasáhne práci
    /// handleru (čtení souborů, zápis logu) — jen čekání na klienta.
    fn io<T>(&self, f: impl FnOnce() -> T) -> T {
        self.nastav(Some(std::time::Instant::now()));
        let out = f();
        self.nastav(None);
        out
    }

    fn nastav(&self, ceka_od: Option<std::time::Instant>) {
        if let Some(c) = self
            .hlidane
            .lock()
            .expect("ipc hlidane lock")
            .get_mut(&self.id)
        {
            c.ceka_od = ceka_od;
        }
    }
}

impl Drop for Hlidani {
    fn drop(&mut self) {
        let zaznam = self
            .hlidane
            .lock()
            .expect("ipc hlidane lock")
            .remove(&self.id);
        if let Some(c) = zaznam {
            // SAFETY: handle z OpenThread v new(), nikdo jiný ho nezavírá;
            // ze seznamu je už pryč, hlídač na něj nesáhne.
            unsafe {
                let _ = CloseHandle(HANDLE(c.vlakno as _));
            }
        }
    }
}

/// Hlídač nečinnosti: spojení, které čeká na klienta déle než `limit`
/// (v provozu IDLE_TIMEOUT), přeruší — čtení/zápis skončí chybou
/// a vlákno spojení zavře.
fn hlidac(hlidane: Hlidane, stop: Arc<AtomicBool>, limit: std::time::Duration) {
    use windows::Win32::System::IO::CancelSynchronousIo;
    while !stop.load(Ordering::SeqCst) {
        pockej(&stop, std::time::Duration::from_secs(1));
        let mut h = hlidane.lock().expect("ipc hlidane lock");
        for c in h.values_mut() {
            if c.ceka_od.is_some_and(|t| t.elapsed() > limit) {
                // SAFETY: handle je platný, dokud je záznam v mapě —
                // a ta je po celou dobu zamčená.
                let _ = unsafe { CancelSynchronousIo(HANDLE(c.vlakno as _)) };
                // Další pokus nejdřív za další timeout, ne každou sekundu.
                c.ceka_od = Some(std::time::Instant::now());
            }
        }
    }
}

/// Spí po daný interval, ale na stop reaguje do ~50 ms.
fn pockej(stop: &AtomicBool, total: std::time::Duration) {
    let deadline = std::time::Instant::now() + total;
    while std::time::Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Počty obsluhovaných spojení — celkem a po klientských procesech.
#[derive(Default)]
struct SlotyStav {
    celkem: usize,
    na_proces: std::collections::HashMap<u32, usize>,
}

/// Přidělování slotů spojením podle MAX_CONNS a MAX_CONNS_NA_PROCES.
#[derive(Clone)]
struct Sloty {
    stav: Arc<std::sync::Mutex<SlotyStav>>,
    max_celkem: usize,
    max_na_proces: usize,
}

impl Sloty {
    fn new(max_celkem: usize, max_na_proces: usize) -> Sloty {
        Sloty {
            stav: Arc::default(),
            max_celkem,
            max_na_proces,
        }
    }

    /// Slot pro spojení procesu `pid`, nebo None, když je plno celkem
    /// nebo pro tento proces.
    fn zabrat(&self, pid: u32) -> Option<Slot> {
        let mut s = self.stav.lock().expect("ipc sloty lock");
        if s.celkem >= self.max_celkem {
            return None;
        }
        let n = s.na_proces.entry(pid).or_insert(0);
        if *n >= self.max_na_proces {
            return None;
        }
        *n += 1;
        s.celkem += 1;
        Some(Slot {
            sloty: self.clone(),
            pid,
        })
    }
}

/// Vrací slot spojení, ať vlákno skončí jakkoli.
struct Slot {
    sloty: Sloty,
    pid: u32,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut s = self.sloty.stav.lock().expect("ipc sloty lock");
        s.celkem -= 1;
        // Záznam procesu bez spojení pryč — jinak by mapa rostla
        // s každým PID, který se kdy připojil.
        if let Some(n) = s.na_proces.get_mut(&self.pid) {
            *n -= 1;
            if *n == 0 {
                s.na_proces.remove(&self.pid);
            }
        }
    }
}

/// PID klientského procesu spojení; 0, když ho systém neřekne (pak
/// všechna taková spojení sdílejí jeden díl).
fn pid_klienta(pipe: HANDLE) -> u32 {
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    let mut pid = 0u32;
    // SAFETY: pipe je platný handle připojené instance.
    match unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } {
        Ok(()) => pid,
        Err(_) => 0,
    }
}

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
    let hlidane: Hlidane = Arc::default();
    // Hlídač se spouští dřív, než první instanci převezme smyčka: když
    // spawn selže, instance se musí zavřít. Dřív se tu vracelo Err s
    // instancí jako holým HANDLE — unikla, a záchranná síť v démonu
    // pak při každém novém bind() narazila na PipeAlreadyExists (jméno
    // držela uniklá instance) a UI se připojovalo k instanci, kterou
    // nikdo neobsluhoval.
    let hlidac_handle = {
        let hlidane = Arc::clone(&hlidane);
        let stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("ipc-hlidac".into())
            .spawn(move || hlidac(hlidane, stop, IDLE_TIMEOUT))
    };
    let hlidac_handle = match hlidac_handle {
        Ok(h) => h,
        Err(e) => {
            // SAFETY: instance z bind(), zatím nikomu nepředaná.
            drop(unsafe { File::from_raw_handle(bound.first_instance as _) });
            return Err(Error::Io(e));
        }
    };
    let mut pending = Some(HANDLE(bound.first_instance as _));
    tracing::info!(pipe = PIPE_NAME, "IPC server naslouchá");

    let sloty = Sloty::new(MAX_CONNS, MAX_CONNS_NA_PROCES);
    let mut dalsi_id: u64 = 0;
    // Přechodné chyby (CreateNamedPipeW, spawn vlákna) smyčku neukončí.
    // Dřív je `?` vyneslo ven, démon jen zalogoval „IPC server spadl"
    // a služba dál sbírala data bez rozhraní, dokud ji někdo ručně
    // nerestartoval — z chvilkového nedostatku prostředků byl trvalý
    // výpadek UI. Čeká se s rostoucí pauzou, ať trvalá chyba netočí CPU.
    const BACKOFF_MIN: std::time::Duration = std::time::Duration::from_millis(100);
    const BACKOFF_MAX: std::time::Duration = std::time::Duration::from_secs(2);
    let mut backoff = BACKOFF_MIN;
    // Odmítnutá spojení nad limitem se logují souhrnně — jinak by je
    // útočník mohl použít k přepsání logu (rotace drží jen ~16 MB).
    let mut odmitnuto: u64 = 0;
    let mut odmitnuto_log = std::time::Instant::now();

    while !stop.load(Ordering::SeqCst) {
        // První kolo použije instanci z bind(), další se vytváří průběžně.
        let pipe = match pending.take() {
            Some(h) => h,
            None => match create_instance(&sec, false) {
                Ok(h) => {
                    backoff = BACKOFF_MIN;
                    h
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        pauza_ms = backoff.as_millis() as u64,
                        "CreateNamedPipeW selhal, zkusím znovu"
                    );
                    pockej(&stop, backoff);
                    backoff = (backoff * 2).min(BACKOFF_MAX);
                    continue;
                }
            },
        };

        // Čekání na klienta. ERROR_PIPE_CONNECTED = klient se stihl
        // připojit mezi vytvořením a čekáním — to je úspěch.
        // SAFETY: pipe je platný handle z create_instance.
        let connected = unsafe { ConnectNamedPipe(pipe, None) };
        if let Err(e) = connected {
            if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                tracing::warn!(error = %e, "ConnectNamedPipe selhal");
                // Instance se nezavírá, ale odpojí a použije znovu.
                // Typicky jde o ERROR_NO_DATA (klient se připojil a hned
                // odpojil). Kdyby se zavřela, a byla to zrovna jediná,
                // jméno pipe by na okamžik zaniklo a založit si ho mohl
                // kdokoli z interaktivní relace — UI by pak mluvilo s ním.
                // SAFETY: pipe je platný handle z create_instance.
                if unsafe { DisconnectNamedPipe(pipe) }.is_ok() {
                    pending = Some(pipe);
                } else {
                    // Nejdřív náhradní instance, teprve pak zavřít starou.
                    if let Ok(h) = create_instance(&sec, false) {
                        pending = Some(h);
                    }
                    // SAFETY: handle vlastníme, File ho korektně zavře.
                    drop(unsafe { File::from_raw_handle(pipe.0 as _) });
                }
                continue;
            }
        }

        // Vlastnictví handle přechází na File (zavře ho při dropu).
        // SAFETY: pipe je platný, nikdo jiný ho nezavírá.
        let stream = unsafe { File::from_raw_handle(pipe.0 as _) };

        if stop.load(Ordering::SeqCst) {
            break; // probuzení dummy klientem při shutdownu
        }

        // Další instanci hned, ještě před obsluhou tohohle spojení: jméno
        // pipe tak drží služba pořád. Kdyby se zavřela poslední instance,
        // jméno by zaniklo a mohl by si ho založit kdokoli jiný — UI by
        // pak mluvilo s cizím serverem. Když to teď nevyjde, zkusí se to
        // znovu (s pauzou) na začátku smyčky.
        if let Ok(h) = create_instance(&sec, false) {
            pending = Some(h);
        }

        let pid = pid_klienta(HANDLE(stream.as_raw_handle() as _));
        let Some(slot) = sloty.zabrat(pid) else {
            // Přes limit: spojení se rovnou zavře (klient dostane chybu
            // a UI to zkusí znovu). Legitimní UI na limit nedosáhne.
            drop(stream);
            odmitnuto += 1;
            if odmitnuto_log.elapsed() >= std::time::Duration::from_secs(10) {
                tracing::warn!(
                    odmitnuto,
                    pid,
                    limit = MAX_CONNS,
                    limit_na_proces = MAX_CONNS_NA_PROCES,
                    "příliš mnoho souběžných spojení — odmítám"
                );
                odmitnuto = 0;
                odmitnuto_log = std::time::Instant::now();
            }
            continue;
        };
        let handler = Arc::clone(&handler);
        let hlidane_conn = Arc::clone(&hlidane);
        let id = dalsi_id;
        dalsi_id += 1;
        let spawn = std::thread::Builder::new()
            .name("ipc-conn".into())
            .spawn(move || {
                let _slot = slot;
                handle_connection(stream, handler, &hlidane_conn, id)
            });
        if let Err(e) = spawn {
            // Closure se i se streamem a slotem zahodila: spojení je
            // zavřené a počet spojení vrácený. Klient dostane EOF.
            tracing::warn!(error = %e, "spawn obslužného vlákna selhal");
            pockej(&stop, backoff);
            backoff = (backoff * 2).min(BACKOFF_MAX);
        } else {
            backoff = BACKOFF_MIN;
        }
    }

    if let Some(h) = pending.take() {
        // SAFETY: instance z create_instance, nikomu nepředaná.
        drop(unsafe { File::from_raw_handle(h.0 as _) });
    }
    let _ = hlidac_handle.join();
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
fn handle_connection(mut stream: File, handler: Handler, hlidane: &Hlidane, id: u64) {
    let hlidani = Hlidani::new(hlidane, id);
    let klient = ClientInfo {
        pipe: stream.as_raw_handle() as isize,
        admin: std::sync::OnceLock::new(),
    };
    loop {
        let precteno = hlidani.io(|| frame::read_msg::<_, Request>(&mut stream));
        match precteno {
            Ok(Some(req)) => {
                let resp = handler(req, &klient);
                let zapsano = hlidani.io(|| frame::write_msg(&mut stream, &resp));
                if let Err(e) = zapsano {
                    tracing::warn!(error = %e, "zápis odpovědi selhal");
                    break;
                }
            }
            Ok(None) => break, // klient čistě zavřel
            // I/O chyba = spojení je pryč nebo ho hlídač přerušil pro
            // nečinnost; psát do něj odpověď nemá smysl.
            Err(Error::Io(e)) => {
                tracing::debug!(error = %e, "spojení s klientem skončilo");
                break;
            }
            Err(e) => {
                tracing::warn!(error = %e, "vadný rámec od klienta");
                let _ = hlidani.io(|| {
                    frame::write_msg(
                        &mut stream,
                        &Response::Error {
                            message: e.to_string(),
                        },
                    )
                });
                break;
            }
        }
    }

    // Korektní rozloučení: doručit zbylé bajty, odpojit instanci.
    // FlushFileBuffers čeká, až si klient data přečte — proto taky pod
    // hlídačem, jinak by klient, který nečte, držel vlákno napořád.
    // SAFETY: handle patří File, jen ho flushneme/odpojíme před dropem.
    let h = HANDLE(stream.as_raw_handle() as _);
    hlidani.io(|| unsafe {
        let _ = FlushFileBuffers(h);
    });
    unsafe {
        let _ = DisconnectNamedPipe(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Soukromá pipe pro test — skutečné jméno drží nainstalovaná služba.
    fn testovaci_pipe(pripona: &str) -> (String, HANDLE) {
        let name = format!(r"\\.\pipe\winsent-test-{}-{pripona}", std::process::id());
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: jméno žije po dobu volání; handle převezme File.
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                0,
                None,
            )
        };
        assert!(
            !h.is_invalid(),
            "CreateNamedPipeW {name}: {}",
            windows::core::Error::from_win32()
        );
        (name, h)
    }

    fn prijmi(h: HANDLE) -> File {
        // SAFETY: platný handle z testovaci_pipe.
        if let Err(e) = unsafe { ConnectNamedPipe(h, None) } {
            assert_eq!(
                e.code(),
                ERROR_PIPE_CONNECTED.to_hresult(),
                "ConnectNamedPipe"
            );
        }
        // SAFETY: handle vlastníme, File ho zavře.
        unsafe { File::from_raw_handle(h.0 as _) }
    }

    /// Členství ve správcích pro tento proces, bez impersonace.
    fn proces_je_elevovany_spravce() -> bool {
        use windows::Win32::Security::{
            CheckTokenMembership, CreateWellKnownSid, WinBuiltinAdministratorsSid, PSID,
        };
        let mut sid = [0u8; 68];
        let mut len = sid.len() as u32;
        let mut member = windows::core::BOOL(0);
        // SAFETY: buffery vlastníme; None = token vlákna (zde primární).
        unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                None,
                Some(PSID(sid.as_mut_ptr() as _)),
                &mut len,
            )
            .unwrap();
            CheckTokenMembership(None, PSID(sid.as_mut_ptr() as _), &mut member).unwrap();
        }
        member.as_bool()
    }

    // Klient je tentýž proces, takže identita musí vyjít stejně jako
    // pro proces sám — a vlákno se pak musí vrátit do vlastní identity.
    #[test]
    fn identita_klienta_a_navrat_z_impersonace() {
        use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
        let (name, h) = testovaci_pipe("identita");
        let klient = std::thread::spawn(move || {
            let mut f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
                .unwrap();
            f.write_all(b"x").unwrap();
            let mut b = [0u8; 1];
            let _ = f.read(&mut b);
        });
        let mut server = prijmi(h);
        let mut b = [0u8; 1];
        server.read_exact(&mut b).unwrap();
        let info = ClientInfo {
            pipe: server.as_raw_handle() as isize,
            admin: std::sync::OnceLock::new(),
        };
        assert_eq!(info.is_elevated_admin(), proces_je_elevovany_spravce());
        // Bez impersonace vlákno žádný vlastní token nemá.
        let mut token = HANDLE::default();
        // SAFETY: pseudo-handle vlákna; případný token hned zavřeme.
        let ma_token =
            unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) }.is_ok();
        if ma_token {
            unsafe {
                let _ = CloseHandle(token);
            }
        }
        assert!(!ma_token, "vlákno zůstalo v identitě klienta");
        server.write_all(b"y").unwrap();
        klient.join().unwrap();
    }

    // Jeden proces nesmí zabrat všechna spojení: když vyčerpá svůj díl,
    // jiný proces (UI) se pořád dostane na řadu. A vrácené sloty se
    // musí dát použít znovu, jinak by se server časem ucpal sám.
    #[test]
    fn sloty_na_proces_a_celkem() {
        let sloty = Sloty::new(5, 2);
        let a1 = sloty.zabrat(10).expect("první spojení procesu");
        let a2 = sloty.zabrat(10).expect("druhé spojení procesu");
        assert!(sloty.zabrat(10).is_none(), "proces přes svůj díl");
        let b1 = sloty.zabrat(20).expect("jiný proces se musí dostat dál");
        let b2 = sloty.zabrat(20).unwrap();
        let c1 = sloty.zabrat(30).unwrap();
        assert!(sloty.zabrat(40).is_none(), "globální strop");
        drop(a1);
        assert!(sloty.zabrat(10).is_some(), "uvolněný slot znovu volný");
        drop((a2, b1, b2, c1));
        let s = sloty.stav.lock().unwrap();
        assert_eq!(s.celkem, 0);
        assert!(s.na_proces.is_empty(), "záznamy procesů zůstaly");
    }

    // Klient, který se připojí a nic nepošle, nesmí držet vlákno
    // napořád: hlídač čtení po limitu přeruší.
    #[test]
    fn hlidac_prerusi_necinne_spojeni() {
        let (name, h) = testovaci_pipe("necinnost");
        let klient = std::thread::spawn(move || {
            let mut f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
                .unwrap();
            // Čeká, až server spojení zavře — sám nic neposílá.
            let mut b = [0u8; 1];
            let _ = f.read(&mut b);
        });
        let mut server = prijmi(h);
        let hlidane: Hlidane = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let hl = {
            let hlidane = Arc::clone(&hlidane);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || hlidac(hlidane, stop, std::time::Duration::from_millis(500)))
        };
        let t0 = std::time::Instant::now();
        let vysledek = {
            let hlidani = Hlidani::new(&hlidane, 1);
            hlidani.io(|| frame::read_msg::<_, Request>(&mut server))
        };
        assert!(
            matches!(vysledek, Err(Error::Io(_))),
            "čtení mělo skončit chybou"
        );
        assert!(t0.elapsed() < std::time::Duration::from_secs(10));
        assert!(hlidane.lock().unwrap().is_empty(), "registrace zůstala");
        drop(server);
        stop.store(true, Ordering::SeqCst);
        hl.join().unwrap();
        klient.join().unwrap();
    }
}
