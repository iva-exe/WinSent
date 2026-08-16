//! Měření klidu tabulky. `cargo run -p ipc --example sortchurn`
//!
//! Odpovídá na otázku „jak často se přeháže pořadí, když se řadí podle
//! téhle hodnoty" — tedy proč se v Tasks řadí podle vyhlazeného čísla.
//! Řazení podle syrové hodnoty vypadá jako správné řešení, dokud si
//! člověk nezměří, že přehází přes polovinu top10 každou sekundu.
//!
//! Naměřeno na 227 procesech: syrová hodnota 5,1 změn za vzorek,
//! klouzavý průměr α=0,3 jen 1,8 — a pořadí přitom vždy souhlasí
//! s číslem ve sloupci, protože se zobrazuje tatáž vyhlazená hodnota.

/// Přesná kopie sysLoad() z UI (tasks/+page.svelte).
fn sys_load(cpu: f32, mem_share: f32) -> f32 {
    let mean = (cpu + mem_share) / 2.0;
    let max = cpu.max(mem_share);
    let w = (max / 100.0).min(1.0);
    mean * (1.0 - w) + max * w
}

fn main() {
    let total = ipc::client::query_system()
        .map(|s| s.mem_total_mb as f64 * 1048576.0)
        .unwrap_or(0.0);

    // (jméno, hodnota) po každém vzorku
    let mut samples: Vec<Vec<(String, f32)>> = Vec::new();
    for _ in 0..15 {
        let rows = match ipc::client::query_procs() {
            Ok(r) => r,
            Err(e) => {
                println!("!! {e}");
                return;
            }
        };
        let mut agg: std::collections::HashMap<String, (f32, u64)> = std::collections::HashMap::new();
        for r in &rows {
            let e = agg.entry(r.app_name.clone()).or_insert((0.0, 0));
            e.0 += r.cpu_pct;
            e.1 += r.ws_bytes;
        }
        let list: Vec<(String, f32)> = agg
            .into_iter()
            .map(|(n, (cpu, ws))| {
                let share = if total > 0.0 {
                    (ws as f64 / total) as f32 * 100.0
                } else {
                    0.0
                };
                (n, sys_load(cpu, share))
            })
            .collect();
        samples.push(list);
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    // Strategie A: řadit podle syrové hodnoty.
    let churn_raw = churn(&samples, |v| v);
    // Strategie B: řadit podle ZOBRAZENÉ hodnoty (na desetinu) —
    // shodné hodnoty pak drží abecedu, takže se nehýbou.
    let churn_disp = churn(&samples, |v| (v * 10.0).round() / 10.0);
    // Strategie C: zobrazená hodnota zaokrouhlená na celá procenta.
    let churn_int = churn(&samples, |v| v.round());

    println!("  změn pozic v top10 na jeden přechod (menší = klidnější):");
    println!("    A) syrová hodnota            {churn_raw:.1}");
    println!("    B) zobrazená (desetina)      {churn_disp:.1}");
    println!("    C) zaokrouhlená na celá %    {churn_int:.1}");

    // Strategie D: klouzavý průměr — vyhlazená hodnota se zobrazuje
    // i řadí, takže pořadí nikdy neodporuje číslu ve sloupci.
    for alpha in [0.5, 0.3, 0.2, 0.1] {
        let sm = smooth(&samples, alpha);
        println!(
            "    D) klouzavý průměr α={alpha:.1}     {:.1}",
            churn(&sm, |v| v)
        );
    }

    // Kolik je zpoždění: za jak dlouho vyhlazená hodnota dorovná skok.
    println!("\n  doba náběhu na 90 % skokové změny (vzorky po 1 s):");
    for alpha in [0.5, 0.3, 0.2, 0.1] {
        let mut v: f32 = 0.0;
        let mut n = 0;
        while v < 0.9 && n < 100 {
            v += (1.0 - v) * alpha;
            n += 1;
        }
        println!("    α={alpha:.1}  {n} s");
    }
}

/// Vyhladí sérii vzorků klouzavým průměrem. Aplikace, která se objeví
/// poprvé, začíná na své hodnotě (žádný náběh z nuly).
fn smooth(samples: &[Vec<(String, f32)>], alpha: f32) -> Vec<Vec<(String, f32)>> {
    let mut state: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for s in samples {
        let mut cur = Vec::new();
        for (n, v) in s {
            let e = state.entry(n.clone()).or_insert(*v);
            *e += (v - *e) * alpha;
            cur.push((n.clone(), *e));
        }
        out.push(cur);
    }
    out
}

/// Kolik pozic v top10 se v průměru změní mezi vzorky, když se řadí
/// podle `key(hodnota)` a shoda se dorovná abecedně.
fn churn(samples: &[Vec<(String, f32)>], key: impl Fn(f32) -> f32) -> f64 {
    let mut prev: Vec<String> = Vec::new();
    let (mut sum, mut n) = (0usize, 0usize);
    for s in samples {
        let mut list: Vec<(String, f32)> = s.iter().map(|(a, b)| (a.clone(), key(*b))).collect();
        list.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        let top: Vec<String> = list.iter().take(10).map(|(x, _)| x.clone()).collect();
        if !prev.is_empty() {
            sum += top.iter().zip(prev.iter()).filter(|(a, b)| a != b).count();
            n += 1;
        }
        prev = top;
    }
    if n == 0 {
        0.0
    } else {
        sum as f64 / n as f64
    }
}
