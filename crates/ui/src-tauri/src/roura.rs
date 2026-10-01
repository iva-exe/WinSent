//! Volání služby přes pipe s čekáním na volnou instanci.
//!
//! Server drží vždy jen jednu čekající instanci pipe a další založí až
//! po připojení klienta. Dokud se příkazy UI vykonávaly na hlavním
//! vlákně, šlo na pipe jedno spojení za druhým. Od přesunu na pracovní
//! vlákna (`#[tauri::command(async)]`) jich jde víc naráz — dávka
//! `query_icon` z Tasks, `Promise.all` vedle pingu — a ostatní dostanou
//! ERROR_PIPE_BUSY. `ipc::client::connect` to zkusí jen 3× po pevných
//! 50 ms, takže se všichni poražení probudí ve stejnou chvíli a projde
//! zase jen jeden. Naměřeno proti běžící službě: z 16 souběžných pingů
//! prošly 3, z 32 jen 4; zbytek skončil chybou a indikátor služby
//! zčervenal, i když služba běžela.
//!
//! Tady se proto obsazená pipe zkouší znovu s náhodnou pauzou, aby se
//! čekající rozprostřeli v čase (stejné měření: 128 souběžných, všech
//! 128 prošlo do 0,4 s). Opakovat je bezpečné i u akcí (kill, mazání):
//! ERROR_PIPE_BUSY vrací jen otevření pipe, tedy dřív, než se službě
//! cokoli poslalo. Globální zámek místo toho by sice spojení seřadil,
//! ale dlouhé dotazy (indexace disku, duplicity) by pak minuty
//! blokovaly ping a celé UI.

use std::time::{Duration, Instant};

/// Jak dlouho nejvýš čekat na volnou instanci. Server instanci
/// zakládá hned po připojení předchozího klienta, takže i stovka
/// souběžných dotazů projde zlomkem; víc už znamená, že služba
/// nepřijímá vůbec, a to má UI hlásit.
const LIMIT: Duration = Duration::from_secs(3);

/// Horní mez náhodné pauzy mezi pokusy.
const PAUZA_MAX_MS: u64 = 40;

/// ERROR_PIPE_BUSY — všechny instance pipe jsou obsazené.
const ERROR_PIPE_BUSY: i32 = 231;

/// Selhalo jen otevření pipe kvůli obsazeným instancím?
fn obsazeno(e: &ipc::Error) -> bool {
    matches!(e, ipc::Error::Io(io) if io.raw_os_error() == Some(ERROR_PIPE_BUSY))
}

/// Náhodná pauza 1..=`max_ms`. Bez závislosti na `rand`: `RandomState`
/// má klíče náhodně osolené pro každou instanci, to na rozptyl stačí.
fn pauza(max_ms: u64) -> Duration {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(std::process::id() as u64);
    Duration::from_millis(1 + h.finish() % max_ms.max(1))
}

/// Zavolá `f`, a dokud selhává na obsazené pipe, zkouší to znovu
/// (nejdéle [`LIMIT`]). Jiné chyby — služba neběží, vadný rámec —
/// vrací hned.
pub fn volej<T>(mut f: impl FnMut() -> Result<T, ipc::Error>) -> Result<T, ipc::Error> {
    let start = Instant::now();
    loop {
        match f() {
            Err(e) if obsazeno(&e) && start.elapsed() < LIMIT => {
                std::thread::sleep(pauza(PAUZA_MAX_MS));
            }
            r => return r,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn busy() -> ipc::Error {
        ipc::Error::Io(std::io::Error::from_raw_os_error(ERROR_PIPE_BUSY))
    }

    #[test]
    fn obsazenou_pipe_zkousi_znovu() {
        let mut pokusy = 0;
        let r = volej(|| {
            pokusy += 1;
            if pokusy < 4 {
                Err(busy())
            } else {
                Ok(pokusy)
            }
        });
        assert_eq!(r.unwrap(), 4);
    }

    #[test]
    fn jine_chyby_vraci_hned() {
        let mut pokusy = 0;
        let r: Result<(), _> = volej(|| {
            pokusy += 1;
            Err(ipc::Error::NotAvailable)
        });
        assert!(matches!(r, Err(ipc::Error::NotAvailable)));
        assert_eq!(pokusy, 1);
        // Jiná I/O chyba (přerušené spojení uprostřed dotazu) se také
        // neopakuje — dotaz už mohl ke službě dorazit.
        let mut pokusy = 0;
        let r: Result<(), _> = volej(|| {
            pokusy += 1;
            Err(ipc::Error::Io(std::io::Error::from_raw_os_error(109)))
        });
        assert!(r.is_err());
        assert_eq!(pokusy, 1);
    }

    #[test]
    fn pauza_je_v_mezich() {
        for _ in 0..200 {
            let p = pauza(PAUZA_MAX_MS).as_millis() as u64;
            assert!((1..=PAUZA_MAX_MS).contains(&p), "{p}");
        }
    }
}
