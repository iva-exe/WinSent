//! identity — rozpoznání aplikace z procesu (SPEC kap. 4).
//!
//! Rozhodovací kaskáda (4.1): první shoda vyhrává. Drahé operace
//! (cesta procesu, ověření podpisu, VERSIONINFO) běží VÝHRADNĚ na
//! background vlákně (BELOW_NORMAL, SPEC kap. 4.2) a cachují se tam.
//! Samplovací cyklus dělá jen lookup v `per_pid` mapě — žádné syscall,
//! žádné metadata, žádné ověřování. Nováček dostane provisional identitu
//! a zařadí se do fronty; hotový výsledek přiteče kanálem.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use core_types::proc::{Confidence, IconData, Protection};

pub mod cascade;

/// Sdílená cache ikon aplikací: identity_key → ikona (None = zkoušeno,
/// ikona není). Plní ji identity worker, čte IPC handler ve službě.
pub type IconStore = Arc<Mutex<HashMap<String, Option<IconData>>>>;

/// Hotová identita jednoho procesu.
#[derive(Debug, Clone)]
pub struct Identity {
    pub identity_key: String,
    pub app_name: String,
    pub publisher: Option<String>,
    pub confidence: Confidence,
}

impl Identity {
    /// Předběžný výsledek, než doběhne kaskáda: seskupí podle názvu
    /// image (bez cesty), confidence guess.
    fn provisional(image_name: &str) -> Identity {
        Identity {
            identity_key: format!("name:{}", image_name.to_ascii_lowercase()),
            app_name: image_name.trim_end_matches(".exe").to_string(),
            publisher: None,
            confidence: Confidence::Guess,
        }
    }
}

/// Statické tabulky pro kaskádu (uninstall záznamy). Zjištěno jednou.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    /// (InstallLocation lowercase, název aplikace), seřazené sestupně
    /// dle délky cesty — nejdelší prefix vyhrává (SPEC 4.1 krok 3).
    pub uninstall: Vec<(String, String)>,
}

/// Ochranná třída procesu (win-sys → serializovatelný core-types typ).
/// Čerstvý dotaz na OS; volá se jen pro nováčky (drží se v cache výše).
pub fn protection(pid: u32, name: &str) -> Protection {
    match win_sys::procinfo::protection(pid, name) {
        win_sys::procinfo::Protection::Critical => Protection::Critical,
        win_sys::procinfo::Protection::Protected => Protection::Protected,
        win_sys::procinfo::Protection::System => Protection::System,
        win_sys::procinfo::Protection::User => Protection::User,
    }
}

/// Klíč cache podpisů (SPEC kap. 4.2): cesta + velikost + mtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SigKey {
    path: String,
    size: u64,
    mtime: i64,
}

impl SigKey {
    fn of(path: &str) -> Option<SigKey> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Some(SigKey {
            path: path.to_string(),
            size: meta.len(),
            mtime,
        })
    }
}

/// Práce pro worker: dořeš identitu procesu.
struct Job {
    pid: u32,
    image_name: String,
}

/// Hotová identita z workeru (mapuje se na pid v hlavním vlákně).
struct Done {
    pid: u32,
    identity: Identity,
    protection: Protection,
}

/// Engine identity: hlavní vlákno drží jen levné mapy, veškerá drahá
/// práce a cache podpisů žijí v background workeru.
pub struct Engine {
    per_pid: HashMap<u32, Identity>,
    prot_pid: HashMap<u32, Protection>,
    tx: Sender<Job>,
    rx_done: Receiver<Done>,
    pending: HashSet<u32>,
    sig_cache_len: usize,
    icons: IconStore,
}

impl Engine {
    /// Spustí engine s background workerem (BELOW_NORMAL).
    pub fn new(tables: Tables) -> Engine {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let (tx_done, rx_done) = std::sync::mpsc::channel::<Done>();
        let icons: IconStore = Arc::new(Mutex::new(HashMap::new()));

        let icons_worker = Arc::clone(&icons);
        std::thread::Builder::new()
            .name("identity".into())
            .spawn(move || worker(rx, tx_done, tables, icons_worker))
            .expect("spuštění identity vlákna");

        Engine {
            per_pid: HashMap::new(),
            prot_pid: HashMap::new(),
            tx,
            rx_done,
            pending: HashSet::new(),
            sig_cache_len: 0,
            icons,
        }
    }

    /// Klon sdílené cache ikon (pro IPC handler ve službě).
    pub fn icons(&self) -> IconStore {
        Arc::clone(&self.icons)
    }

    /// Vezme hotové výsledky z workeru.
    fn drain(&mut self) {
        while let Ok(done) = self.rx_done.try_recv() {
            self.pending.remove(&done.pid);
            self.per_pid.insert(done.pid, done.identity);
            self.prot_pid.insert(done.pid, done.protection);
            self.sig_cache_len = self.sig_cache_len.max(1); // orientační
        }
    }

    /// Identita procesu. V samplovacím cyklu jen lookup; nováček dostane
    /// provisional a zařadí se do fronty.
    pub fn identify(&mut self, pid: u32, image_name: &str) -> (Identity, Protection) {
        self.drain();
        if let Some(id) = self.per_pid.get(&pid) {
            let prot = self.prot_pid.get(&pid).copied().unwrap_or_default();
            return (id.clone(), prot);
        }
        // Nováček.
        let prov = Identity::provisional(image_name);
        self.per_pid.insert(pid, prov.clone());
        if self.pending.insert(pid) {
            let _ = self.tx.send(Job {
                pid,
                image_name: image_name.to_string(),
            });
        }
        (prov, Protection::default())
    }

    /// Úklid zaniklých procesů.
    pub fn retain_pids(&mut self, live: &HashSet<u32>) {
        self.per_pid.retain(|pid, _| live.contains(pid));
        self.prot_pid.retain(|pid, _| live.contains(pid));
        self.pending.retain(|pid| live.contains(pid));
    }

    /// Orientační velikost cache (pro měření).
    pub fn sig_cache_len(&self) -> usize {
        self.sig_cache_len
    }
}

/// Background worker: fetch cesty, kaskáda, cache podpisů. Jediné vlákno,
/// takže cache nepotřebuje zámky.
fn worker(rx: Receiver<Job>, tx_done: Sender<Done>, tables: Tables, icons: IconStore) {
    let _ = win_sys::threading::set_current_thread_below_normal();
    let mut sig_cache: HashMap<SigKey, Identity> = HashMap::new();

    while let Ok(job) = rx.recv() {
        let path = win_sys::procinfo::image_path(job.pid);

        // Cache podle (cesta, velikost, mtime) — procesy stejné binárky
        // sdílí výsledek, podpis se ověří jednou.
        let identity = match path.as_deref().and_then(SigKey::of) {
            Some(key) => {
                if let Some(id) = sig_cache.get(&key) {
                    id.clone()
                } else {
                    let id = cascade::resolve(job.pid, &job.image_name, path.as_deref(), &tables);
                    sig_cache.insert(key, id.clone());
                    id
                }
            }
            None => cascade::resolve(job.pid, &job.image_name, path.as_deref(), &tables),
        };

        // Ikona aplikace — jednou na identity_key (drahé GDI, tady na
        // BELOW_NORMAL vlákně). Do cache i None, ať se nezkouší dokola.
        let need_icon = {
            let map = icons.lock().expect("icon cache lock");
            !map.contains_key(&identity.identity_key)
        };
        if need_icon {
            let ico = path
                .as_deref()
                .and_then(win_sys::icon::extract)
                .map(|i| IconData {
                    w: i.w,
                    h: i.h,
                    rgba: i.rgba,
                });
            icons
                .lock()
                .expect("icon cache lock")
                .insert(identity.identity_key.clone(), ico);
        }

        let protection = protection(job.pid, &job.image_name);
        if tx_done
            .send(Done {
                pid: job.pid,
                identity,
                protection,
            })
            .is_err()
        {
            break;
        }
    }
}

/// Načte uninstall záznamy z registru (SPEC kap. 5.1) — jednou při startu.
pub fn load_tables() -> Tables {
    use win_sys::registry::{enum_subkeys, read_string, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let mut uninstall = Vec::new();
    let roots = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ];
    for (root, base) in roots {
        for sub in enum_subkeys(root, base) {
            let key = format!("{base}\\{sub}");
            let Some(loc) = read_string(root, &key, "InstallLocation") else {
                continue;
            };
            if loc.trim().is_empty() {
                continue;
            }
            let name = read_string(root, &key, "DisplayName").unwrap_or_else(|| sub.clone());
            uninstall.push((loc.to_ascii_lowercase(), name));
        }
    }
    // Nejdelší prefix první (nejspecifičtější InstallLocation vyhrává).
    uninstall.sort_by_key(|(loc, _)| std::cmp::Reverse(loc.len()));
    uninstall.dedup_by(|a, b| a.0 == b.0);
    tracing::info!(
        count = uninstall.len(),
        "načteny uninstall záznamy pro identitu"
    );
    Tables { uninstall }
}

/// Je cesta pod systémovým adresářem Windows?
pub(crate) fn under_system_root(path: &str) -> bool {
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    path.to_ascii_lowercase()
        .starts_with(&sysroot.to_ascii_lowercase())
}

/// Parent adresář cesty (pro path fallback).
pub(crate) fn parent_dir(path: &str) -> String {
    Path::new(path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(path)
        .to_string()
}
