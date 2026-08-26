//! Diagnostika GPU čítačů: kolik instancí `\GPU Engine(*)` PDH vrací,
//! kolik jich zahodíme kvůli CStatus a jaké PID/engine v nich jsou.
//!
//! `cargo run -p win-sys --example gpuprobe`
//!
//! Jen čtení. Slouží k porovnání s tím, co ukazuje Správce úloh.

use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA,
};

fn main() {
    // SAFETY: standardní PDH sekvence, query se zavírá na konci.
    unsafe {
        let mut query = PDH_HQUERY::default();
        assert_eq!(PdhOpenQueryW(PCWSTR::null(), 0, &mut query), 0, "PdhOpenQueryW");
        let mut counter = PDH_HCOUNTER::default();
        let rc = PdhAddEnglishCounterW(
            query,
            w!("\\GPU Engine(*)\\Utilization Percentage"),
            0,
            &mut counter,
        );
        if rc != 0 {
            println!("counter GPU Engine nejde přidat: 0x{rc:08x}");
            return;
        }
        let _ = PdhCollectQueryData(query);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let _ = PdhCollectQueryData(query);

        let mut size = 0u32;
        let mut count = 0u32;
        let rc = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
        if rc != PDH_MORE_DATA {
            println!("první dotaz nevrátil PDH_MORE_DATA: 0x{rc:08x}");
            return;
        }
        let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let cap = (size as usize).div_ceil(item);
        let mut buf = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); cap.max(1)];
        let rc = PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut size,
            &mut count,
            Some(buf.as_mut_ptr()),
        );
        if rc != 0 {
            println!("druhý dotaz selhal: 0x{rc:08x}");
            return;
        }

        let mut by_status = std::collections::BTreeMap::<u32, u32>::new();
        let mut nonzero_dropped = 0u32;
        let mut per_pid_all = std::collections::BTreeMap::<u32, f64>::new();
        let mut per_pid_cstatus0 = std::collections::BTreeMap::<u32, f64>::new();

        for it in buf.iter().take(count as usize) {
            let name = it.szName.to_string().unwrap_or_default();
            let st = it.FmtValue.CStatus;
            let v = it.FmtValue.Anonymous.doubleValue;
            *by_status.entry(st).or_default() += 1;
            let pid = name
                .strip_prefix("pid_")
                .and_then(|r| r[..r.find('_').unwrap_or(r.len())].parse::<u32>().ok());
            if let Some(p) = pid {
                *per_pid_all.entry(p).or_default() += v;
                if st == 0 {
                    *per_pid_cstatus0.entry(p).or_default() += v;
                } else if v > 0.0 {
                    nonzero_dropped += 1;
                }
            }
        }

        println!("instancí celkem: {count}");
        println!("rozpad podle CStatus: {by_status:?}");
        println!("zahozeno kvůli CStatus != 0 s nenulovou hodnotou: {nonzero_dropped}");
        println!(
            "procesů s GPU > 0 — všechny instance: {}, jen CStatus==0: {}",
            per_pid_all.values().filter(|v| **v > 0.0).count(),
            per_pid_cstatus0.values().filter(|v| **v > 0.0).count()
        );

        let mut top: Vec<_> = per_pid_all.iter().filter(|(_, v)| **v > 0.0).collect();
        top.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        println!("\nTOP procesy (všechny instance):");
        for (pid, v) in top.iter().take(15) {
            println!("  pid {pid:>6}  {v:>6.2} %");
        }

        // Rozpad podle enginů u nejvytíženějších procesů: Správce úloh
        // ukazuje MAXIMUM přes typy enginů, my zatím součet.
        let mut per_pid_eng: std::collections::BTreeMap<u32, std::collections::BTreeMap<String, f64>> =
            std::collections::BTreeMap::new();
        for it in buf.iter().take(count as usize) {
            let name = it.szName.to_string().unwrap_or_default();
            let v = it.FmtValue.Anonymous.doubleValue;
            if v <= 0.0 { continue; }
            let Some(pid) = name.strip_prefix("pid_").and_then(|r| r[..r.find('_').unwrap_or(r.len())].parse::<u32>().ok()) else { continue };
            let eng = name.rsplit_once("engtype_").map(|(_, t)| t.to_string()).unwrap_or_default();
            *per_pid_eng.entry(pid).or_default().entry(eng).or_default() += v;
        }
        println!("
rozpad podle enginů (jen nenulové):");
        for (pid, m) in &per_pid_eng {
            let sum: f64 = m.values().sum();
            let max = m.values().cloned().fold(0.0f64, f64::max);
            println!("  pid {pid:>6}  součet {sum:>6.2} %  max {max:>6.2} %  {m:?}");
        }

        // Jak vyjde CELKOVÉ GPU podle tří různých metodik. Správce úloh
        // ukazuje v hlavičce jedno číslo — hledáme, která z nich sedí.
        let mut by_engtype: std::collections::BTreeMap<String, f64> =
            std::collections::BTreeMap::new();
        let mut by_engine: std::collections::BTreeMap<String, f64> =
            std::collections::BTreeMap::new();
        let mut grand = 0.0f64;
        for it in buf.iter().take(count as usize) {
            let name = it.szName.to_string().unwrap_or_default();
            let v = it.FmtValue.Anonymous.doubleValue;
            grand += v;
            let eng = name
                .rsplit_once("engtype_")
                .map(|(_, t)| t.to_string())
                .unwrap_or_default();
            *by_engtype.entry(eng).or_default() += v;
            // Fyzický engine = vše kromě pid_ části: luid + phys + eng.
            let phys = name
                .split_once("_luid_")
                .map(|(_, r)| r.to_string())
                .unwrap_or_default();
            *by_engine.entry(phys).or_default() += v;
        }
        let max_engtype = by_engtype.values().cloned().fold(0.0f64, f64::max);
        let max_engine = by_engine.values().cloned().fold(0.0f64, f64::max);
        println!("\ncelkové GPU podle metodik:");
        println!("  součet všeho .................. {grand:>6.2} %");
        println!("  max přes TYPY enginů (naše) ... {max_engtype:>6.2} %");
        println!("  max přes FYZICKÉ enginy ....... {max_engine:>6.2} %");
        let mut top: Vec<_> = by_engine.iter().collect();
        top.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        println!("  nejvytíženější fyzické enginy:");
        for (k, v) in top.iter().take(4) {
            println!("    {v:>6.2} %  {k}");
        }

        // Ukázka jmen instancí — ať je vidět skutečný tvar.
        println!("\nprvních 6 jmen instancí:");
        for it in buf.iter().take(6) {
            println!(
                "  {}  (CStatus 0x{:08x}, {:.2})",
                it.szName.to_string().unwrap_or_default(),
                it.FmtValue.CStatus,
                it.FmtValue.Anonymous.doubleValue
            );
        }

        let _ = PdhCloseQuery(query);
    }
}
