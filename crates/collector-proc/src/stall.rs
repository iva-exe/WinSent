//! Detekce záseku systému (SPEC kap. 3.3): heartbeat vlákno na
//! TIME_CRITICAL prioritě tikne každých 100 ms a měří skutečný odstup
//! (QueryUnbiasedInterruptTime — bez spánku, viz `unbiased_100ns`).
//! Když je odstup > 3× očekávání (tj. > 300 ms), systém se zasekl
//! — vlákno samo nic nepočítá ani
//! nezapisuje (na téhle prioritě NIC drahého), jen pošle hit kanálem.
//!
//! Klasifikaci příčiny dělá daemon z metrik sampleru; hity kratší než
//! `MIN_GAP_S` od předchozího se slučují do jednoho záseku.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Interval heartbeatu.
const TICK_MS: u64 = 100;
/// Násobek intervalu, od kterého jde o zásek.
const STALL_FACTOR: u32 = 3;

/// Jeden zaznamenaný zásek.
#[derive(Debug, Clone, Copy)]
pub struct StallHit {
    /// Unix čas, kdy zásek skončil (heartbeat se probral).
    pub ts: i64,
    /// Jak dlouho heartbeat neběžel (ms).
    pub lag_ms: u64,
}

/// Běžící detektor. Drop zastaví vlákno.
pub struct Detector {
    rx: Receiver<StallHit>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Detector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Detector {
    /// Spustí heartbeat vlákno.
    pub fn start() -> std::io::Result<Detector> {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let stop_thread = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("stall-heartbeat".into())
            .spawn(move || heartbeat(stop_thread, tx))?;
        Ok(Detector {
            rx,
            stop,
            thread: Some(thread),
        })
    }

    /// Vybere hity od minulého volání (neblokuje).
    pub fn drain(&mut self) -> Vec<StallHit> {
        let mut out = Vec::new();
        while let Ok(hit) = self.rx.try_recv() {
            out.push(hit);
        }
        out
    }
}

// QueryUnbiasedInterruptTime není ve windows-rs feature sadě, kterou
// collector-proc má (žádnou) — deklarace přímo z kernel32.
#[link(name = "kernel32")]
extern "system" {
    fn QueryUnbiasedInterruptTime(unbiased_time: *mut u64) -> i32;
}

/// Čas běhu systému BEZ spánku a hibernace (jednotky 100 ns).
///
/// `Instant` (QueryPerformanceCounter) na Windows běží i během uspání.
/// Heartbeat pak po probuzení viděl odstup rovný délce spánku a hlásil
/// ho jako zásek: v historii přibýval incident „zásek systému" s lagem
/// v hodinách (naměřeno 26 326 s a 75 803 s), viníkem podle metrik po
/// probuzení a forenzním oknem přes celou noc. Unbiased interrupt time
/// se během spánku zastaví, takže měří jen dobu, kdy systém opravdu běžel.
fn unbiased_100ns() -> u64 {
    let mut t = 0u64;
    // SAFETY: platný ukazatel na u64; funkce jen zapisuje výstup.
    unsafe {
        QueryUnbiasedInterruptTime(&mut t);
    }
    t
}

/// Odstup dvou čtení unbiased času. Couvnutí (nemá se stát) = nula,
/// ne obří lag z přetečení.
fn odstup(pred: u64, ted: u64) -> Duration {
    Duration::from_nanos(ted.saturating_sub(pred).saturating_mul(100))
}

/// Rozhodne o jednom tiku podle odstupu BEZ spánku: `Some(lag_ms)` =
/// zásek. I kdyby do záseku spadl začátek uspání, lag je jen doba, po
/// kterou systém běžel a heartbeat stál. Rozlišení unbiased času je
/// ~15,6 ms, proti prahu 300 ms zanedbatelné.
fn posoud_tik(odstup_bez_spanku: Duration, threshold: Duration) -> Option<u64> {
    (odstup_bez_spanku > threshold).then_some(odstup_bez_spanku.as_millis() as u64)
}

/// Smyčka heartbeatu — na TIME_CRITICAL, aby ji zátěž nemohla vytlačit;
/// dělá výhradně sleep + porovnání času.
fn heartbeat(stop: Arc<AtomicBool>, tx: Sender<StallHit>) {
    if let Err(e) = win_sys::threading::set_current_thread_time_critical() {
        tracing::warn!(error = %e, "heartbeat bez TIME_CRITICAL priority");
    }
    let expected = Duration::from_millis(TICK_MS);
    let threshold = expected * STALL_FACTOR;
    let mut last = unbiased_100ns();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(expected);
        let now = unbiased_100ns();
        let delta = odstup(last, now);
        last = now;
        if let Some(lag_ms) = posoud_tik(delta, threshold) {
            let hit = StallHit {
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                lag_ms,
            };
            if tx.send(hit).is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRAH: Duration = Duration::from_millis(300);

    // Mezera přes uspání se v unbiased čase neprojeví (ten během spánku
    // stojí), takže z ní zásek nevznikne; skutečný zásek ano.
    #[test]
    fn zasek_jen_z_casu_bez_spanku() {
        assert_eq!(posoud_tik(Duration::from_millis(100), PRAH), None);
        assert_eq!(posoud_tik(Duration::from_millis(300), PRAH), None);
        assert_eq!(posoud_tik(Duration::from_millis(512), PRAH), Some(512));
    }

    // 100ns jednotky → Duration; couvnutí nesmí dát obří lag.
    #[test]
    fn odstup_v_100ns() {
        assert_eq!(odstup(10_000_000, 15_000_000), Duration::from_millis(500));
        assert_eq!(odstup(15_000_000, 10_000_000), Duration::ZERO);
    }

    // Unbiased čas jde dopředu (a funkce se dá volat — kernel32 export).
    #[test]
    fn unbiased_cas_roste() {
        let a = unbiased_100ns();
        std::thread::sleep(Duration::from_millis(40));
        let b = unbiased_100ns();
        assert!(b > a);
    }
}
