//! Snapshot všech procesů přes `NtQuerySystemInformation(SystemProcessInformation)`
//! (SPEC kap. 3.1). Jedno volání vrátí všechny procesy v jednom bufferu;
//! buffer se alokuje jednou a realokuje jen při `STATUS_INFO_LENGTH_MISMATCH`
//! — v horké cestě žádné alokace.
//!
//! Obrana dle INFRA kap. 1.3: nedokumentované API — každý krok chůze
//! bufferem se validuje proti vrácené délce; vadný offset ukončí čtení
//! chybou, nikdy čtením mimo buffer.

use std::ffi::c_void;

use crate::Error;

// NtQuerySystemInformation není ve windows-rs feature sadě, kterou
// používáme — deklarace přímo z ntdll (import lib je součástí SDK).
#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret_len: *mut u32) -> i32;
}

const SYSTEM_PROCESS_INFORMATION_CLASS: u32 = 5;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

/// UNICODE_STRING (winternl).
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

/// SYSTEM_PROCESS_INFORMATION — veřejně zdokumentovaný NT layout
/// (ntdoc/phnt), stabilní od Visty. Čteme jen pole, která potřebujeme;
/// zbytek drží offsety.
#[repr(C)]
struct SystemProcessInformation {
    next_entry_offset: u32,
    number_of_threads: u32,
    working_set_private_size: i64,
    hard_fault_count: u32,
    number_of_threads_high_watermark: u32,
    cycle_time: u64,
    create_time: i64,
    user_time: i64,
    kernel_time: i64,
    image_name: UnicodeString,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
    handle_count: u32,
    session_id: u32,
    unique_process_key: usize,
    peak_virtual_size: usize,
    virtual_size: usize,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_page_count: usize,
    read_operation_count: i64,
    write_operation_count: i64,
    other_operation_count: i64,
    read_transfer_count: i64,
    write_transfer_count: i64,
    other_transfer_count: i64,
}

/// Surová data jednoho procesu ze snapshotu (bez odvozenin typu CPU %).
#[derive(Debug, Clone)]
pub struct RawProc {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
    /// Kumulativní CPU čas (kernel + user) v jednotkách 100 ns.
    pub cpu_time_100ns: u64,
    /// Čas vzniku procesu (FILETIME) — spolu s PID tvoří identitu
    /// odolnou proti recyklaci PID.
    pub create_time: i64,
    pub ws_bytes: u64,
    pub priv_bytes: u64,
    pub threads: u32,
    pub session_id: u32,
    pub hard_faults: u32,
    pub handles: u32,
    /// Kumulativní přenesené bajty I/O (čtení/zápis) — delty počítá
    /// kolektor stejně jako u CPU.
    pub io_read_bytes: u64,
    pub io_write_bytes: u64,
}

/// Naplní snapshot všech procesů. `buf` se znovupoužívá mezi voláními.
pub fn snapshot_processes(buf: &mut Vec<u8>) -> Result<Vec<RawProc>, Error> {
    if buf.capacity() == 0 {
        buf.reserve(512 * 1024);
    }

    // Realokační smyčka: při MISMATCH zvětšit dle ret_len + rezerva
    // (počet procesů se mezi voláními mění).
    let filled = loop {
        let mut ret_len: u32 = 0;
        // SAFETY: předáváme vlastní buffer a jeho skutečnou kapacitu.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESS_INFORMATION_CLASS,
                buf.as_mut_ptr() as *mut c_void,
                buf.capacity() as u32,
                &mut ret_len,
            )
        };
        match status {
            0 => break ret_len as usize,
            STATUS_INFO_LENGTH_MISMATCH => {
                buf.reserve((ret_len as usize + 128 * 1024).saturating_sub(buf.capacity()));
            }
            s => {
                return Err(Error::Win32 {
                    call: "NtQuerySystemInformation(SystemProcessInformation)",
                    code: s,
                })
            }
        }
    };

    let entry_size = std::mem::size_of::<SystemProcessInformation>();
    let mut procs = Vec::with_capacity(256);
    let mut offset = 0usize;

    loop {
        // Validace: celá struktura musí ležet uvnitř vyplněné délky.
        if offset + entry_size > filled {
            return Err(Error::Win32 {
                call: "SystemProcessInformation walk (offset mimo buffer)",
                code: STATUS_INFO_LENGTH_MISMATCH,
            });
        }
        // SAFETY: offset + entry_size ověřeno proti `filled`; buffer žije.
        let p = unsafe { &*(buf.as_ptr().add(offset) as *const SystemProcessInformation) };

        // Jméno: UNICODE_STRING ukazuje dovnitř téhož bufferu. Délka
        // v bajtech; pid 0 (Idle) má prázdný string.
        let name = if p.image_name.buffer.is_null() || p.image_name.length == 0 {
            String::from("System Idle Process")
        } else {
            // SAFETY: buffer + length pochází z jádra a leží v našem bufferu.
            let chars = unsafe {
                std::slice::from_raw_parts(p.image_name.buffer, (p.image_name.length / 2) as usize)
            };
            String::from_utf16_lossy(chars)
        };

        procs.push(RawProc {
            pid: p.unique_process_id as u32,
            parent_pid: p.inherited_from_unique_process_id as u32,
            name,
            cpu_time_100ns: (p.kernel_time.max(0) + p.user_time.max(0)) as u64,
            create_time: p.create_time,
            ws_bytes: p.working_set_size as u64,
            priv_bytes: p.private_page_count as u64,
            threads: p.number_of_threads,
            session_id: p.session_id,
            hard_faults: p.hard_fault_count,
            handles: p.handle_count,
            io_read_bytes: p.read_transfer_count.max(0) as u64,
            io_write_bytes: p.write_transfer_count.max(0) as u64,
        });

        if p.next_entry_offset == 0 {
            break;
        }
        let next = offset + p.next_entry_offset as usize;
        // Validace: offset musí růst a zůstat v bufferu.
        if next <= offset || next > filled {
            return Err(Error::Win32 {
                call: "SystemProcessInformation walk (vadný next_entry_offset)",
                code: STATUS_INFO_LENGTH_MISMATCH,
            });
        }
        offset = next;
    }

    Ok(procs)
}
