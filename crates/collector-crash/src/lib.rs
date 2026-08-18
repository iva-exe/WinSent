//! collector-crash — incidenty po tvrdém pádu PC (SPEC kap. 16).
//!
//! v3: sken minidumpů po BSOD. Kernel dump v `%SystemRoot%\Minidump\`
//! má dokumentovanou hlavičku (`PAGEDU64`) s bugcheck kódem a parametry
//! na pevných offsetech — čte se BEZ debuggeru. Kód se překládá na
//! lidskou příčinu statickou tabulkou (SPEC 16.1).
//!
//! Pády a hangy běžících aplikací řeší ETW ProcessStop (collector-etw);
//! tahle crate se stará o to, co se dozvíme až po restartu. Jen čte —
//! nález zapisuje svc přes store (oddělené cesty, SPEC kap. 2).

use core_types::config::Config;

/// Chyby této crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {}

/// Stav kolektoru (v3: bez průběžného stavu — sken je jednorázový).
pub mod report;

pub struct State;

/// Inicializace kolektoru při startu služby.
pub fn init(_cfg: &Config) -> Result<State, Error> {
    Ok(State)
}

/// Jeden krok sběru (v3: nic průběžného).
pub fn tick(_state: &mut State) -> Result<(), Error> {
    Ok(())
}

/// Korektní ukončení kolektoru.
pub fn shutdown(_state: State) {}

/// Nález BSOD z minidumpu.
#[derive(Debug, Clone)]
pub struct BsodFinding {
    /// Čas pádu (mtime dump souboru).
    pub ts: i64,
    pub bugcheck: u32,
    pub params: [u64; 4],
    pub dump_path: String,
    /// Lidský překlad bugcheck kódu.
    pub human: &'static str,
}

/// Projde `%SystemRoot%\Minidump` a vrátí dumpy novější než `since`.
/// Nečitelné/cizí soubory se přeskočí — sken nikdy neselže celý.
pub fn scan_minidumps(since: i64) -> Vec<BsodFinding> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let dir = std::path::Path::new(&root).join("Minidump");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new(); // adresář nemusí existovat (žádný BSOD nikdy)
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("dmp"))
        {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let ts = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if ts <= since {
            continue;
        }
        if let Some((bugcheck, params)) = parse_dump_header(&path) {
            out.push(BsodFinding {
                ts,
                bugcheck,
                params,
                dump_path: path.to_string_lossy().into_owned(),
                human: bugcheck_human(bugcheck),
            });
        }
    }
    out.sort_by_key(|f| f.ts);
    out
}

/// Přečte bugcheck kód + parametry z hlavičky kernel dumpu.
/// Layout DUMP_HEADER64: signature `PAGE` @0, `DU64` @4,
/// BugCheckCode u32 @0x38, BugCheckParameter1–4 u64 @0x40.
fn parse_dump_header(path: &std::path::Path) -> Option<(u32, [u64; 4])> {
    use std::io::Read;
    let mut buf = [0u8; 0x60];
    let mut f = std::fs::File::open(path).ok()?;
    f.read_exact(&mut buf).ok()?;
    if &buf[0..4] != b"PAGE" || &buf[4..8] != b"DU64" {
        return None;
    }
    let bugcheck = u32::from_le_bytes(buf[0x38..0x3C].try_into().ok()?);
    let mut params = [0u64; 4];
    for (i, p) in params.iter_mut().enumerate() {
        let off = 0x40 + i * 8;
        *p = u64::from_le_bytes(buf[off..off + 8].try_into().ok()?);
    }
    Some((bugcheck, params))
}

/// Statická tabulka nejčastějších bugcheck kódů → lidská příčina.
pub fn bugcheck_human(code: u32) -> &'static str {
    match code {
        0x0A => "IRQL_NOT_LESS_OR_EQUAL — ovladač sáhl do neplatné paměti",
        0x1A => "MEMORY_MANAGEMENT — chyba správy paměti (často vadná RAM)",
        0x1E => "KMODE_EXCEPTION_NOT_HANDLED — neošetřená výjimka v jádře",
        0x24 => "NTFS_FILE_SYSTEM — chyba souborového systému NTFS",
        0x3B => "SYSTEM_SERVICE_EXCEPTION — výjimka v systémové službě",
        0x50 => "PAGE_FAULT_IN_NONPAGED_AREA — přístup do neexistující paměti",
        0x7A => "KERNEL_DATA_INPAGE_ERROR — čtení stránky z disku selhalo",
        0x7E => "SYSTEM_THREAD_EXCEPTION_NOT_HANDLED — pád vlákna ovladače",
        0x9F => "DRIVER_POWER_STATE_FAILURE — ovladač uvízl při uspávání",
        0xA0 => "INTERNAL_POWER_ERROR — chyba správy napájení",
        0xC2 => "BAD_POOL_CALLER — ovladač poškodil paměťový pool",
        0xD1 => "DRIVER_IRQL_NOT_LESS_OR_EQUAL — chybný přístup ovladače",
        0xEF => "CRITICAL_PROCESS_DIED — zemřel kritický proces Windows",
        0xF5 => "FLTMGR_FILE_SYSTEM — pád filtru souborového systému",
        0x116 => "VIDEO_TDR_FAILURE — GPU ovladač neodpověděl (TDR)",
        0x124 => "WHEA_UNCORRECTABLE_ERROR — hardwarová chyba (CPU/RAM/deska)",
        0x133 => "DPC_WATCHDOG_VIOLATION — ovladač blokoval systém příliš dlouho",
        0x139 => "KERNEL_SECURITY_CHECK_FAILURE — poškozená kernel struktura",
        0x154 => "UNEXPECTED_STORE_EXCEPTION — chyba paměťového store (disk?)",
        0x1CA => "SYNTHETIC_WATCHDOG_TIMEOUT — systém přestal odpovídat",
        _ => "neznámý bugcheck — viz kód a parametry",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hlavička se syntetickým PAGEDU64 dumpem se musí naparsovat.
    #[test]
    fn parses_synthetic_dump_header() {
        let dir = std::env::temp_dir().join("syswatch-test-dump");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.dmp");
        let mut buf = vec![0u8; 0x1000];
        buf[0..4].copy_from_slice(b"PAGE");
        buf[4..8].copy_from_slice(b"DU64");
        buf[0x38..0x3C].copy_from_slice(&0x9Fu32.to_le_bytes());
        buf[0x40..0x48].copy_from_slice(&3u64.to_le_bytes());
        std::fs::write(&path, &buf).unwrap();

        let (code, params) = parse_dump_header(&path).unwrap();
        assert_eq!(code, 0x9F);
        assert_eq!(params[0], 3);
        assert!(bugcheck_human(code).contains("DRIVER_POWER_STATE_FAILURE"));
        let _ = std::fs::remove_file(&path);
    }
}
