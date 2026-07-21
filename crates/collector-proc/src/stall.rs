//! Detekce záseku systému (SPEC kap. 3.3): heartbeat vlákno na
//! TIME_CRITICAL prioritě tikne každých 100 ms a měří skutečný odstup
//! (`Instant` = QueryPerformanceCounter). Když je odstup > 3× očekávání
//! (tj. > 300 ms), systém se zasekl — vlákno samo nic nepočítá ani
//! nezapisuje (na téhle prioritě NIC drahého), jen pošle hit kanálem.
//!
//! Klasifikaci příčiny dělá daemon z metrik sampleru; hity kratší než
//! `MIN_GAP_S` od předchozího se slučují do jednoho záseku.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// Smyčka heartbeatu — na TIME_CRITICAL, aby ji zátěž nemohla vytlačit;
/// dělá výhradně sleep + porovnání času.
fn heartbeat(stop: Arc<AtomicBool>, tx: Sender<StallHit>) {
    if let Err(e) = win_sys::threading::set_current_thread_time_critical() {
        tracing::warn!(error = %e, "heartbeat bez TIME_CRITICAL priority");
    }
    let expected = Duration::from_millis(TICK_MS);
    let threshold = expected * STALL_FACTOR;
    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(expected);
        let now = Instant::now();
        let delta = now.duration_since(last);
        last = now;
        if delta > threshold {
            let hit = StallHit {
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                lag_ms: delta.as_millis() as u64,
            };
            if tx.send(hit).is_err() {
                break;
            }
        }
    }
}
