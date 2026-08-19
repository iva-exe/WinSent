//! Čtení výpisů paměti (.dmp) do textu, ze kterého jde určit viník.
//!
//! Dva různé formáty pod stejnou příponou:
//!
//! * **MDMP** — výpis padlé aplikace (`%LOCALAPPDATA%\CrashDumps`,
//!   WER). Dokumentovaný formát: hlavička, adresář streamů, a v nich
//!   seznam načtených modulů s bázemi a velikostmi plus záznam
//!   o výjimce. Adresa výjimky se dá proti tomu seznamu porovnat —
//!   a tím padne odpověď na otázku „ve kterém modulu to spadlo".
//! * **PAGEDU64** — jaderný výpis po modré obrazovce
//!   (`C:\Windows\Minidump`). Jiná stavba; bugcheck a jeho parametry
//!   leží v pevné hlavičce.
//!
//! Bez ladicích symbolů se nedostaneme na jména funkcí — na to je
//! potřeba debugger a symboly od Microsoftu. Modul ale stačí: to je
//! přesně ta informace, kterou chce člověk i model na druhé straně.
//!
//! Obrana (INFRA 1.3): soubor je cizí binárka, klidně useknutá nebo
//! poškozená. Každý offset se ověřuje proti délce, počty mají strop
//! a nic se nečte mimo buffer — vadný soubor skončí zkráceným výpisem,
//! nikdy pádem.

use std::path::Path;

/// Kolik modulů má smysl vypsat. Dump s milionem modulů je vadný.
const MAX_MODULES: usize = 4096;
/// Strop na čtení souboru. Plné výpisy mají stovky MB a do textového
/// záznamu stejně nepatří — bereme hlavičku a streamy, ne paměť.
const MAX_READ: u64 = 128 * 1024 * 1024;

/// Přečte výpis a vrátí textový rozbor. Chyby se nevyhazují: do
/// záznamu patří i věta „tenhle soubor přečíst nejde a proč".
pub fn describe_dump(path: &Path) -> String {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return format!("Výpis {} nejde otevřít: {e}", path.display()),
    };
    if meta.len() > MAX_READ {
        return format!(
            "Výpis {} má {:.1} GB — příliš velký na vložení do záznamu. \
             Pošli ho zvlášť; obsahuje celý obraz paměti.",
            path.display(),
            meta.len() as f64 / 1e9
        );
    }
    let buf = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return format!("Výpis {} nejde přečíst: {e}", path.display()),
    };

    let mut out = String::new();
    out.push_str(&format!(
        "Soubor:   {}\nVelikost: {} B\n",
        path.display(),
        buf.len()
    ));

    match buf.get(..4) {
        Some(b"MDMP") => describe_user_dump(&buf, &mut out),
        Some(b"PAGE") => describe_kernel_dump(&buf, &mut out),
        _ => out.push_str("Neznámý formát výpisu — první bajty nesedí na MDMP ani PAGEDU64.\n"),
    }
    out
}

// ── Čtení čísel s kontrolou délky ────────────────────────────────────
fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    b.get(off..off + 8).map(|s| {
        u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]])
    })
}

/// MINIDUMP_STRING: délka v BAJTECH, pak UTF-16 bez ukončovací nuly.
fn mdmp_string(b: &[u8], rva: usize) -> Option<String> {
    let len = u32_at(b, rva)? as usize;
    // Jméno modulu delší než kilobajt je nesmysl — chrání proti
    // vadné délce, která by jinak vzala celý zbytek souboru.
    if len > 4096 || len % 2 != 0 {
        return None;
    }
    let bytes = b.get(rva + 4..rva + 4 + len)?;
    let u16s: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(String::from_utf16_lossy(&u16s))
}

/// Jeden načtený modul z výpisu.
struct Module {
    base: u64,
    size: u32,
    name: String,
    version: String,
}

/// Výpis padlé aplikace (MDMP).
fn describe_user_dump(b: &[u8], out: &mut String) {
    out.push_str("Formát:   MDMP (výpis padlé aplikace)\n");

    let streams = match u32_at(b, 8) {
        Some(n) if n as usize <= 256 => n as usize,
        _ => {
            out.push_str("Adresář streamů je vadný.\n");
            return;
        }
    };
    let dir_rva = match u32_at(b, 12) {
        Some(r) => r as usize,
        None => return,
    };
    out.push_str(&format!("Streamů:  {streams}\n"));

    // Adresář: 12 bajtů na položku (typ, velikost, offset).
    let mut modules_loc = None;
    let mut exception_loc = None;
    let mut sysinfo_loc = None;
    for i in 0..streams {
        let e = dir_rva + i * 12;
        let (Some(kind), Some(size), Some(rva)) =
            (u32_at(b, e), u32_at(b, e + 4), u32_at(b, e + 8))
        else {
            break;
        };
        match kind {
            4 => modules_loc = Some((rva as usize, size)),
            6 => exception_loc = Some(rva as usize),
            7 => sysinfo_loc = Some(rva as usize),
            _ => {}
        }
    }

    if let Some(rva) = sysinfo_loc {
        describe_sysinfo(b, rva, out);
    }

    let modules = modules_loc
        .map(|(rva, _)| read_modules(b, rva))
        .unwrap_or_default();

    // Výjimka a hlavně JEJÍ ADRESA — z ní se určuje viník.
    if let Some(rva) = exception_loc {
        describe_exception(b, rva, &modules, out);
    } else {
        out.push_str("Záznam o výjimce ve výpisu není.\n");
    }

    out.push_str(&format!("\nNačtené moduly ({}):\n", modules.len()));
    out.push_str("  báze               velikost  verze            modul\n");
    for m in &modules {
        out.push_str(&format!(
            "  0x{:016x} {:>9}  {:<16} {}\n",
            m.base, m.size, m.version, m.name
        ));
    }
}

/// SystemInfoStream: na čem to běželo.
fn describe_sysinfo(b: &[u8], rva: usize, out: &mut String) {
    let arch = match u32_at(b, rva).map(|v| v & 0xffff) {
        Some(0) => "x86",
        Some(5) => "ARM",
        Some(9) => "x64",
        Some(12) => "ARM64",
        _ => "neznámá",
    };
    // NumberOfProcessors je v MINIDUMP_SYSTEM_INFO až za trojicí
    // USHORT (architektura, úroveň, revize) — tedy na +6, ne +3.
    let cpus = b.get(rva + 6).copied().unwrap_or(0);
    let (Some(major), Some(minor), Some(build)) = (
        u32_at(b, rva + 8),
        u32_at(b, rva + 12),
        u32_at(b, rva + 16),
    ) else {
        return;
    };
    out.push_str(&format!(
        "Systém:   Windows {major}.{minor} build {build}, architektura {arch}, {cpus} procesorů\n"
    ));
}

/// ExceptionStream + přiřazení adresy k modulu.
fn describe_exception(b: &[u8], rva: usize, modules: &[Module], out: &mut String) {
    // MINIDUMP_EXCEPTION_STREAM: thread id, zarovnání, pak výjimka.
    let thread = u32_at(b, rva).unwrap_or(0);
    let er = rva + 8;
    let (Some(code), Some(addr), Some(nparams)) =
        (u32_at(b, er), u64_at(b, er + 16), u32_at(b, er + 24))
    else {
        out.push_str("Záznam o výjimce je zkrácený.\n");
        return;
    };

    out.push_str(&format!("\nVýjimka:\n  vlákno:   {thread}\n"));
    out.push_str(&format!("  kód:      0x{code:08X}"));
    if let Some(h) = crate::report::exception_human(&format!("{code:08x}")) {
        out.push_str(&format!("  ({h})"));
    }
    out.push('\n');
    out.push_str(&format!("  adresa:   0x{addr:016x}\n"));

    // Parametry výjimky (u přístupu do paměti: [0] čtení/zápis,
    // [1] adresa, na kterou se sahalo).
    let n = (nparams as usize).min(15);
    for i in 0..n {
        if let Some(p) = u64_at(b, er + 32 + i * 8) {
            out.push_str(&format!("  param[{i}]: 0x{p:016x}\n"));
        }
    }

    // Tohle je ta odpověď, kvůli které se dump čte.
    match modules
        .iter()
        .find(|m| addr >= m.base && addr < m.base.saturating_add(m.size as u64))
    {
        Some(m) => out.push_str(&format!(
            "  VINÍK:    {} (offset 0x{:x} v modulu, verze {})\n",
            m.name,
            addr - m.base,
            m.version
        )),
        None => out.push_str(
            "  VINÍK:    adresa nepadla do žádného načteného modulu \
             (typicky kód generovaný za běhu nebo poškozený zásobník)\n",
        ),
    }
}

/// ModuleListStream.
fn read_modules(b: &[u8], rva: usize) -> Vec<Module> {
    let Some(count) = u32_at(b, rva) else {
        return Vec::new();
    };
    let count = (count as usize).min(MAX_MODULES);
    let mut out = Vec::with_capacity(count.min(512));
    for i in 0..count {
        // MINIDUMP_MODULE má 108 bajtů.
        let m = rva + 4 + i * 108;
        let (Some(base), Some(size), Some(name_rva)) =
            (u64_at(b, m), u32_at(b, m + 8), u32_at(b, m + 20))
        else {
            break;
        };
        let name = mdmp_string(b, name_rva as usize).unwrap_or_else(|| "(bez jména)".into());
        // VS_FIXEDFILEINFO začíná na +24; verze souboru je na +8 a +12
        // uvnitř něj (nejvyšší a nejnižší dvojice čísel).
        let version = match (u32_at(b, m + 32), u32_at(b, m + 36)) {
            (Some(ms), Some(ls)) if ms != 0 || ls != 0 => format!(
                "{}.{}.{}.{}",
                ms >> 16,
                ms & 0xffff,
                ls >> 16,
                ls & 0xffff
            ),
            _ => "—".into(),
        };
        // Jen jméno souboru; celá cesta je v seznamu stovky modulů šum.
        let short = name.rsplit('\\').next().unwrap_or(&name).to_string();
        out.push(Module {
            base,
            size,
            name: short,
            version,
        });
    }
    out
}

/// Jaderný výpis po modré obrazovce (PAGEDU64).
fn describe_kernel_dump(b: &[u8], out: &mut String) {
    let sig = String::from_utf8_lossy(b.get(..8).unwrap_or_default()).to_string();
    out.push_str(&format!("Formát:   {sig} (jaderný výpis po modré obrazovce)\n"));

    if let (Some(major), Some(minor)) = (u32_at(b, 0x08), u32_at(b, 0x0c)) {
        out.push_str(&format!("Systém:   build {major}.{minor}\n"));
    }
    let Some(bugcheck) = u32_at(b, 0x38) else {
        out.push_str("Hlavička je zkrácená.\n");
        return;
    };
    out.push_str(&format!(
        "Bugcheck: 0x{bugcheck:08X}  ({})\n",
        crate::bugcheck_human(bugcheck)
    ));
    for i in 0..4 {
        if let Some(p) = u64_at(b, 0x40 + i * 8) {
            out.push_str(&format!("  param[{i}]: 0x{p:016x}\n"));
        }
    }
    if let Some(cpus) = u32_at(b, 0x30) {
        out.push_str(&format!("Procesorů: {cpus}\n"));
    }

    // Jména ovladačů leží ve výpisu jako UTF-16 řetězce. Bez rozboru
    // triage struktury se nedá říct, který je viník, ale samotný soupis
    // toho, co bylo načtené, je pro rozbor cenný — a hlavně poctivý:
    // vypisuje se, co v souboru opravdu je, nic se nedopočítává.
    let names = scan_sys_names(b);
    if !names.is_empty() {
        out.push_str(&format!("\nOvladače nalezené ve výpisu ({}):\n", names.len()));
        for n in &names {
            out.push_str(&format!("  {n}\n"));
        }
        out.push_str(
            "\nKterý z nich pád způsobil, z výpisu bez ladicích symbolů neurčíme — \
             porovnej adresu z parametrů s bázemi ovladačů v debuggeru.\n",
        );
    }
}

/// Posbírá jména `.sys` souborů zapsaná ve výpisu jako UTF-16.
fn scan_sys_names(b: &[u8]) -> Vec<String> {
    let mut found = std::collections::BTreeSet::new();
    let mut i = 0usize;
    while i + 2 <= b.len() {
        // Hledá se ".sys" v UTF-16 (tečka, s, y, s po dvou bajtech).
        if b[i] == b'.'
            && b.get(i + 2) == Some(&b's')
            && b.get(i + 4) == Some(&b'y')
            && b.get(i + 6) == Some(&b's')
        {
            // Zpátky po znacích jména, dokud jsou tisknutelné.
            let mut start = i;
            while start >= 2 {
                let c = b[start - 2];
                if b[start - 1] != 0 || !(c.is_ascii_alphanumeric() || c == b'_' || c == b'-') {
                    break;
                }
                start -= 2;
            }
            if start < i {
                let name: String = (start..=i + 6)
                    .step_by(2)
                    .filter_map(|k| b.get(k).map(|&c| c as char))
                    .collect();
                if name.len() > 4 {
                    found.insert(name);
                }
            }
        }
        i += 2;
    }
    found.into_iter().take(512).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vadný a zkrácený soubor nesmí nic shodit.
    #[test]
    fn broken_input_never_panics() {
        let mut out = String::new();
        describe_user_dump(b"MDMP", &mut out);
        describe_user_dump(&[0u8; 16], &mut out);
        describe_kernel_dump(b"PAGEDU64", &mut out);
        describe_kernel_dump(&[0xffu8; 64], &mut out);
        assert!(!out.is_empty());
    }

    // Nesmyslná délka řetězce se odmítne, ne alokuje.
    #[test]
    fn absurd_string_length_is_refused() {
        let mut b = vec![0u8; 32];
        b[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(mdmp_string(&b, 0).is_none());
    }

    // Neexistující soubor se přizná větou, ne pádem.
    #[test]
    fn missing_file_is_explained() {
        let t = describe_dump(Path::new(r"C:\neexistuje-xyz\zadny.dmp"));
        assert!(t.contains("nejde otevřít"), "{t}");
    }
}

/// Najde výpisy a hlášení, která patří k jednomu incidentu, a složí
/// z nich text do záznamu.
///
/// Hledá se na všech místech, kam Windows takové věci ukládají:
/// * `explicit` — cesta z hlášení o modré obrazovce, když ji známe,
/// * `C:\Windows\Minidump` — jaderné výpisy,
/// * `…\AppData\Local\CrashDumps` v profilu KAŽDÉHO uživatele —
///   výpisy padlých aplikací (služba běží jako SYSTEM, takže vlastní
///   `LOCALAPPDATA` je jí k ničemu),
/// * archiv Windows Error Reporting — textová hlášení `Report.wer`.
///
/// `window_s` je tolerance kolem času incidentu; soubor vzniká
/// s malým zpožděním po pádu.
pub fn dumps_for(app: &str, ts: i64, explicit: Option<&str>, window_s: i64) -> String {
    let mut out = String::new();
    let mut seen = std::collections::BTreeSet::new();
    let stem = app
        .rsplit(char::from(92u8))
        .next()
        .unwrap_or(app)
        .trim_end_matches(".exe")
        .to_ascii_lowercase();

    // 1. Výpis, na který ukazuje samo hlášení.
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        if seen.insert(p.to_ascii_lowercase()) {
            out.push_str(&section(&describe_dump(Path::new(p))));
        }
    }

    // 2. Jaderné výpisy v okně kolem incidentu.
    for p in files_in(r"C:\Windows\Minidump", "dmp") {
        if !near(&p, ts, window_s) {
            continue;
        }
        if seen.insert(p.to_string_lossy().to_ascii_lowercase()) {
            out.push_str(&section(&describe_dump(&p)));
        }
    }

    // 3. Výpisy aplikací napříč profily uživatelů.
    for profile in user_profiles() {
        let dir = format!(r"{profile}\AppData\Local\CrashDumps");
        for p in files_in(&dir, "dmp") {
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
            // Jméno souboru začíná jménem aplikace; bez shody by se do
            // záznamu o jednom pádu nasypaly výpisy všeho ostatního.
            if !stem.is_empty() && !name.starts_with(&stem) {
                continue;
            }
            if !near(&p, ts, window_s) {
                continue;
            }
            if seen.insert(p.to_string_lossy().to_ascii_lowercase()) {
                out.push_str(&section(&describe_dump(&p)));
            }
        }
    }

    // 4. Textová hlášení WER. Vkládají se celá — jsou to prosté
    //    dvojice klíč=hodnota a přesně o ně jde.
    for p in wer_reports(&stem, ts, window_s) {
        if !seen.insert(p.to_string_lossy().to_ascii_lowercase()) {
            continue;
        }
        match std::fs::read_to_string(&p) {
            Ok(t) => out.push_str(&section(&format!(
                "Soubor:   {}\nFormát:   Report.wer (hlášení Windows Error Reporting)\n\n{t}",
                p.display()
            ))),
            Err(e) => out.push_str(&section(&format!("{} nejde přečíst: {e}", p.display()))),
        }
    }

    if out.is_empty() {
        out.push_str(
            "K tomuhle incidentu se žádný výpis paměti ani hlášení nenašly.\n\
             Windows je po čase samy mažou (Vyčištění disku, údržba), takže\n\
             u starších pádů to je běžné.\n",
        );
    }
    out
}

/// Oddělovač mezi jednotlivými výpisy.
fn section(body: &str) -> String {
    format!("\n{}\n{body}\n", "=".repeat(60))
}

/// Soubory dané přípony v adresáři (bez rekurze).
fn files_in(dir: &str, ext: &str) -> Vec<std::path::PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|x| x.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
        })
        .take(64)
        .collect()
}

/// Vznikl soubor blízko času incidentu?
fn near(p: &Path, ts: i64, window_s: i64) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    let Ok(m) = meta.modified() else { return false };
    let Ok(d) = m.duration_since(std::time::UNIX_EPOCH) else {
        return false;
    };
    (d.as_secs() as i64 - ts).abs() <= window_s
}

/// Profily uživatelů — služba běží jako SYSTEM a výpisy aplikací leží
/// v profilu toho, komu spadly.
fn user_profiles() -> Vec<String> {
    let root = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + r"\Users";
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path().to_string_lossy().into_owned())
        .take(32)
        .collect()
}

/// Hlášení WER, která patří k aplikaci a času.
fn wer_reports(stem: &str, ts: i64, window_s: i64) -> Vec<std::path::PathBuf> {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    let mut out = Vec::new();
    for sub in ["ReportArchive", "ReportQueue"] {
        let dir = format!(r"{base}\Microsoft\Windows\WER\{sub}");
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten().take(2048) {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            // Jméno složky nese jméno aplikace (AppCrash_<app>_…),
            // takže se dá filtrovat bez otevírání každého souboru.
            let n = p.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
            if !stem.is_empty() && !n.contains(stem) {
                continue;
            }
            let report = p.join("Report.wer");
            if report.is_file() && near(&report, ts, window_s) {
                out.push(report);
            }
        }
    }
    out.truncate(8);
    out
}
