//! ETW (SPEC kap. 3.2): realtime session pro události procesů a
//! POVINNÝ autologger — černá skříňka ve file módu (.etl, rotující
//! ring), jejíž buffery zapisuje jádro, takže přežije BSOD.
//!
//! Realtime konzumujeme jen Microsoft-Windows-Kernel-Process
//! (ProcessStart/Stop s exit kódem a pravým parent PID — to žádné
//! jiné API nedá). Hard faulty a latence disku jdou levněji z PDH a
//! IOCTL (Kernel-Memory na Win10 hard-fault keyword nemá — ověřeno
//! `logman query providers`). Černá skříňka nahrává navíc Kernel-Disk
//! pro forenzní analýzu okna pádu.
//!
//! Sessions vyžadují admin/SYSTEM — služba běží jako SYSTEM, konzole
//! elevovaně. Kolize jmen (session přežila pád procesu) se řeší
//! stop + retry.

use std::ffi::c_void;
use std::sync::mpsc::{Receiver, Sender};

use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_SUCCESS};
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW,
    CONTROLTRACE_HANDLE, EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_RECORD,
    EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_FILE_MODE_CIRCULAR, EVENT_TRACE_LOGFILEW,
    EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_REAL_TIME, WNODE_FLAG_TRACED_GUID,
};

use crate::Error;

/// Microsoft-Windows-Kernel-Process.
const KERNEL_PROCESS: GUID = GUID::from_u128(0x22FB2CD6_0E7B_422B_A0C7_2FAD1FD0E716);
/// Microsoft-Windows-Kernel-Disk.
const KERNEL_DISK: GUID = GUID::from_u128(0xC7BDE69A_E1E0_4177_B6EF_283AD1525271);
/// WINEVENT_KEYWORD_PROCESS.
const KW_PROCESS: u64 = 0x10;
/// TRACE_LEVEL_INFORMATION.
const LEVEL_INFO: u8 = 4;

/// Událost procesu z ETW.
#[derive(Debug, Clone)]
pub enum ProcEvent {
    /// Nový proces: (ts unix, pid, parent pid).
    Start { ts: i64, pid: u32, parent: u32 },
    /// Konec procesu: (ts unix, pid, exit code).
    Stop { ts: i64, pid: u32, exit_code: u32 },
}

/// Běžící ETW session (controller handle). Drop session zastaví.
pub struct Session {
    handle: CONTROLTRACE_HANDLE,
    name: Vec<u16>,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = stop_session_by_name(&mut self.name.clone(), Some(self.handle));
    }
}

/// Vlastnosti session se jménem (a případně souborem) v jednom bufferu,
/// jak to EVENT_TRACE_PROPERTIES vyžaduje.
fn build_props(file: Option<&str>) -> (Vec<u8>, usize, usize) {
    const MAX_CHARS: usize = 512;
    let head = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
    let name_off = head;
    let file_off = head + MAX_CHARS * 2;
    let total = head + MAX_CHARS * 4;
    let mut buf = vec![0u8; total];

    // SAFETY: buffer je čerstvě alokovaný, dost velký na strukturu.
    let props = unsafe { &mut *(buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES) };
    props.Wnode.BufferSize = total as u32;
    props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
    props.Wnode.ClientContext = 1; // QPC
    props.BufferSize = 64; // KB na buffer
    props.FlushTimer = 1; // s — realtime doručení bez čekání na plný buffer
    props.LoggerNameOffset = name_off as u32;
    match file {
        Some(path) => {
            props.LogFileMode = EVENT_TRACE_FILE_MODE_CIRCULAR;
            props.MaximumFileSize = 64; // MB — rotující ring (SPEC 16.3)
            props.LogFileNameOffset = file_off as u32;
            let wide: Vec<u16> = path.encode_utf16().take(MAX_CHARS - 1).collect();
            let dst = &mut buf[file_off..file_off + wide.len() * 2];
            // SAFETY: kopie u16 → bajty do rezervovaného úseku bufferu.
            dst.copy_from_slice(unsafe {
                std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2)
            });
        }
        None => {
            props.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
            props.LogFileNameOffset = 0;
        }
    }
    (buf, name_off, file_off)
}

/// Zastaví session daného jména (úklid po pádu / před restartem).
fn stop_session_by_name(
    name: &mut [u16],
    handle: Option<CONTROLTRACE_HANDLE>,
) -> Result<(), Error> {
    let (mut buf, _, _) = build_props(None);
    // SAFETY: props buffer má správnou velikost; jméno je nul-ukončené.
    let rc = unsafe {
        ControlTraceW(
            handle.unwrap_or_default(),
            PWSTR(name.as_mut_ptr()),
            buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            EVENT_TRACE_CONTROL_STOP,
        )
    };
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(Error::Win32 {
            call: "ControlTraceW(stop)",
            code: rc.0 as i32,
        })
    }
}

/// Spustí session (realtime když `file` je None, jinak circular file)
/// a zapne na ní dané providery (guid, matchanykeyword).
pub fn start_session(
    name: &str,
    file: Option<&str>,
    providers: &[(GUID, u64)],
) -> Result<Session, Error> {
    let mut wname: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let (mut buf, _, _) = build_props(file);

    // SAFETY: props buffer drží jméno i soubor uvnitř; při kolizi
    // starou session zastavíme a zkusíme znovu.
    let mut handle = CONTROLTRACE_HANDLE::default();
    unsafe {
        let mut rc = StartTraceW(
            &mut handle,
            PWSTR(wname.as_mut_ptr()),
            buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
        );
        if rc == ERROR_ALREADY_EXISTS {
            let _ = stop_session_by_name(&mut wname, None);
            let (mut buf2, _, _) = build_props(file);
            buf = buf2.clone();
            rc = StartTraceW(
                &mut handle,
                PWSTR(wname.as_mut_ptr()),
                buf2.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            );
        }
        if rc != ERROR_SUCCESS {
            return Err(Error::Win32 {
                call: "StartTraceW",
                code: rc.0 as i32,
            });
        }
        for (guid, keywords) in providers {
            let rc = EnableTraceEx2(
                handle,
                guid,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                LEVEL_INFO,
                *keywords,
                0,
                0,
                None,
            );
            if rc != ERROR_SUCCESS {
                let _ = stop_session_by_name(&mut wname, Some(handle));
                return Err(Error::Win32 {
                    call: "EnableTraceEx2",
                    code: rc.0 as i32,
                });
            }
        }
    }
    let _ = buf; // props musí žít přes StartTraceW, dál ne
    Ok(Session {
        handle,
        name: wname,
    })
}

/// Realtime session pro daemon: Kernel-Process (start/stop procesů).
pub fn start_realtime(name: &str) -> Result<Session, Error> {
    start_session(name, None, &[(KERNEL_PROCESS, KW_PROCESS)])
}

/// Černá skříňka: circular .etl, Kernel-Process + Kernel-Disk.
pub fn start_blackbox(name: &str, etl_path: &str) -> Result<Session, Error> {
    start_session(
        name,
        Some(etl_path),
        &[(KERNEL_PROCESS, KW_PROCESS), (KERNEL_DISK, 0)],
    )
}

/// Kontext konzumenta předávaný do C callbacku.
struct ConsumerCtx {
    tx: Sender<ProcEvent>,
}

/// SAFETY: volá ETW runtime; record je platný po dobu callbacku.
unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    let Some(record) = record.as_ref() else {
        return;
    };
    let ctx = record.UserContext as *const ConsumerCtx;
    let Some(ctx) = ctx.as_ref() else { return };
    if record.EventHeader.ProviderId != KERNEL_PROCESS {
        return;
    }

    let data =
        std::slice::from_raw_parts(record.UserData as *const u8, record.UserDataLength as usize);
    let ts = now_unix();
    // Payloady jsou pevný prefix; offsety ale závisí na VERZI eventu:
    // Win10 1809+ (ProcessStart v3, ProcessStop v2) vkládá za ProcessID
    // ještě ProcessSequenceNumber (u64) — ověřeno diagnostikou etwtest,
    // bez posunu vychází z polí smetí. ImageName a spol. jsou dál a mění
    // se častěji — nečteme je.
    let version = record.EventHeader.EventDescriptor.Version;
    match record.EventHeader.EventDescriptor.Id {
        // ProcessStart: ProcessID u32, [SequenceNumber u64 (v3+)],
        // CreateTime FILETIME, ParentProcessID u32.
        1 => {
            let off = if version >= 3 { 20 } else { 12 };
            if data.len() >= off + 4 {
                let pid = u32::from_le_bytes(data[0..4].try_into().unwrap());
                let parent = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
                let _ = ctx.tx.send(ProcEvent::Start { ts, pid, parent });
            }
        }
        // ProcessStop: ProcessID u32, [SequenceNumber u64 (v2+)],
        // CreateTime FILETIME, ExitTime FILETIME, ExitCode u32.
        2 => {
            let off = if version >= 2 { 28 } else { 20 };
            if data.len() >= off + 4 {
                let pid = u32::from_le_bytes(data[0..4].try_into().unwrap());
                let exit_code = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
                let _ = ctx.tx.send(ProcEvent::Stop { ts, pid, exit_code });
            }
        }
        _ => {}
    }
}

/// Běžící konzument realtime session. Drop odpojí (ProcessTrace vlákno
/// pak samo skončí).
pub struct Consumer {
    handle: u64,
    /// Kontext musí žít, dokud běží callbacky.
    _ctx: Box<ConsumerCtx>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        // SAFETY: handle je z OpenTraceW; CloseTrace odblokuje ProcessTrace.
        unsafe {
            let h = windows::Win32::System::Diagnostics::Etw::PROCESSTRACE_HANDLE {
                Value: self.handle,
            };
            let _ = CloseTrace(h);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Připojí konzumenta na realtime session; události tečou do kanálu.
pub fn consume(session_name: &str) -> Result<(Receiver<ProcEvent>, Consumer), Error> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctx = Box::new(ConsumerCtx { tx });

    let mut wname: Vec<u16> = session_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut logfile = EVENT_TRACE_LOGFILEW {
        LoggerName: PWSTR(wname.as_mut_ptr()),
        Context: ctx.as_mut() as *mut ConsumerCtx as *mut c_void,
        ..Default::default()
    };
    logfile.Anonymous1.ProcessTraceMode =
        PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    logfile.Anonymous2.EventRecordCallback = Some(on_event);

    // SAFETY: logfile drží platné ukazatele (wname, ctx) po dobu volání;
    // ctx žije v Consumer po celou dobu běhu vlákna.
    let handle = unsafe { OpenTraceW(&mut logfile) };
    if handle.Value == u64::MAX {
        return Err(Error::Win32 {
            call: "OpenTraceW",
            code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
        });
    }

    let hval = handle.Value;
    let thread = std::thread::Builder::new()
        .name("etw-consume".into())
        .spawn(move || {
            // SAFETY: handle je platný do CloseTrace; ProcessTrace blokuje
            // a vrací se po zavření handle. Chybový návrat = konzument
            // nikdy neběžel — to se MUSÍ objevit v logu.
            unsafe {
                let h =
                    windows::Win32::System::Diagnostics::Etw::PROCESSTRACE_HANDLE { Value: hval };
                let rc = ProcessTrace(&[h], None, None);
                if rc.0 != 0 {
                    tracing::error!(code = rc.0, "ProcessTrace skončil s chybou");
                }
            }
        })
        .map_err(|_| Error::Win32 {
            call: "spawn(etw-consume)",
            code: -1,
        })?;

    Ok((
        rx,
        Consumer {
            handle: hval,
            _ctx: ctx,
            thread: Some(thread),
        },
    ))
}

/// Unix čas.
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
