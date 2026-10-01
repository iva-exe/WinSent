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
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Tabulky pro kaskádu (uninstall záznamy). Načtou se při startu a znovu
/// po změně přihlášených uživatelů (viz `worker`).
#[derive(Debug, Clone, Default)]
pub struct Tables {
    /// Instalační adresáře, seřazené sestupně dle délky cesty —
    /// nejdelší prefix vyhrává (SPEC 4.1 krok 3).
    pub uninstall: Vec<UninstallEntry>,
    /// identity_key („app:…“) → DisplayIcon spec z uninstall registru
    /// („cesta,index“) — fallback, když .exe procesu ikonu nemá.
    pub icons: HashMap<String, String>,
    /// Hive uživatelů, ze kterých se tabulky četly (viz `user_sids`).
    sids: Vec<String>,
}

/// Jeden instalační adresář z uninstall registru.
#[derive(Debug, Clone)]
pub struct UninstallEntry {
    /// InstallLocation malými písmeny, bez koncového „\".
    pub loc: String,
    /// DisplayName aplikace.
    pub name: String,
    /// Leží pod tímhle adresářem instalace jiné aplikace? Pak to není
    /// bydliště jedné aplikace, ale sběrný adresář, a platí jen pro
    /// binárky přímo v něm (viz `mark_collection_dirs`).
    pub collection: bool,
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
    birth: i64,
    image_name: String,
}

/// Hotová identita z workeru (mapuje se na pid v hlavním vlákně).
struct Done {
    pid: u32,
    birth: i64,
    identity: Identity,
    protection: Protection,
}

/// Engine identity: hlavní vlákno drží jen levné mapy, veškerá drahá
/// práce a cache podpisů žijí v background workeru.
pub struct Engine {
    per_pid: HashMap<u32, Identity>,
    prot_pid: HashMap<u32, Protection>,
    birth: HashMap<u32, i64>,
    tx: Sender<Job>,
    rx_done: Receiver<Done>,
    pending: HashSet<u32>,
    sig_cache_len: usize,
    icons: IconStore,
    /// Kolikrát worker přenačetl tabulky (sdílené s workerem).
    generation: Arc<AtomicU64>,
    /// Generace, ke které patří identity v `per_pid`.
    seen_generation: u64,
    /// PIDy, jejichž identita pochází ze starých tabulek a má se
    /// dopočítat znovu. Do té doby platí ta stará — kdyby se zahodila,
    /// celé Procesy by po přihlášení na chvíli spadly na provisional
    /// seskupení podle jména.
    stale: HashSet<u32>,
}

impl Engine {
    /// Spustí engine s background workerem (BELOW_NORMAL).
    pub fn new(tables: Tables) -> Engine {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let (tx_done, rx_done) = std::sync::mpsc::channel::<Done>();
        let icons: IconStore = Arc::new(Mutex::new(HashMap::new()));
        let generation = Arc::new(AtomicU64::new(0));

        let icons_worker = Arc::clone(&icons);
        let generation_worker = Arc::clone(&generation);
        std::thread::Builder::new()
            .name("identity".into())
            .spawn(move || worker(rx, tx_done, tables, icons_worker, generation_worker))
            .expect("spuštění identity vlákna");

        Engine {
            per_pid: HashMap::new(),
            prot_pid: HashMap::new(),
            birth: HashMap::new(),
            tx,
            rx_done,
            pending: HashSet::new(),
            sig_cache_len: 0,
            icons,
            generation,
            seen_generation: 0,
            stale: HashSet::new(),
        }
    }

    /// Klon sdílené cache ikon (pro IPC handler ve službě).
    pub fn icons(&self) -> IconStore {
        Arc::clone(&self.icons)
    }

    /// Vezme hotové výsledky z workeru.
    fn drain(&mut self) {
        while let Ok(done) = self.rx_done.try_recv() {
            // Zatímco worker počítal, mohl PID zaniknout a Windows ho
            // přidělit jinému procesu. Výsledek se přijme jen tehdy,
            // když pořád patří tomu procesu, pro který se zadával.
            if self.birth.get(&done.pid) != Some(&done.birth) {
                self.pending.remove(&done.pid);
                continue;
            }
            self.pending.remove(&done.pid);
            self.per_pid.insert(done.pid, done.identity);
            self.prot_pid.insert(done.pid, done.protection);
            self.sig_cache_len = self.sig_cache_len.max(1); // orientační
        }
    }

    /// Identita procesu. V samplovacím cyklu jen lookup; nováček dostane
    /// provisional a zařadí se do fronty.
    pub fn identify(
        &mut self,
        pid: u32,
        image_name: &str,
        create_time: i64,
    ) -> (Identity, Protection) {
        self.drain();
        // Worker přenačetl tabulky (přihlásil se uživatel, jehož hive
        // při startu služby ještě nebyla načtená). Všechno, co se
        // spočítalo se starými, se dopočítá znovu.
        let generation = self.generation.load(Ordering::Relaxed);
        if generation != self.seen_generation {
            self.seen_generation = generation;
            // Pokrývá i rozpracované PIDy, jejichž výsledek ze starých
            // tabulek ještě visí v kanálu: nováček dostane provisional do
            // `per_pid` dřív, než se zařadí do `pending`, a oba se mažou
            // vždy spolu — `pending` je tak podmnožinou klíčů `per_pid`.
            self.stale = self.per_pid.keys().copied().collect();
        }
        // Cache drží PID, ale PID Windows recykluje. Když se čas vzniku
        // liší, je za tím číslem jiný proces a stará identita by mu
        // podstrčila cizí aplikaci — záznam se zahodí a začne se znovu.
        if self.birth.insert(pid, create_time) != Some(create_time) {
            self.per_pid.remove(&pid);
            self.prot_pid.remove(&pid);
            self.pending.remove(&pid);
            self.stale.remove(&pid);
        }
        if let Some(id) = self.per_pid.get(&pid) {
            // Zastaralý výsledek se znovu zařadí, jen když na tenhle PID
            // zrovna nic nečeká: rozpracovaný job mohl začít ještě se
            // starými tabulkami, takže PID zůstane `stale`, dokud jeho
            // výsledek nedorazí, a teprve pak se pošle znovu.
            if !self.pending.contains(&pid) && self.stale.remove(&pid) {
                self.pending.insert(pid);
                let _ = self.tx.send(Job {
                    pid,
                    birth: create_time,
                    image_name: image_name.to_string(),
                });
            }
            let prot = self.prot_pid.get(&pid).copied().unwrap_or_default();
            return (id.clone(), prot);
        }
        // Nováček.
        let prov = Identity::provisional(image_name);
        self.per_pid.insert(pid, prov.clone());
        if self.pending.insert(pid) {
            let _ = self.tx.send(Job {
                pid,
                birth: create_time,
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
        self.birth.retain(|pid, _| live.contains(pid));
        self.stale.retain(|pid| live.contains(pid));
    }

    /// Orientační velikost cache (pro měření).
    pub fn sig_cache_len(&self) -> usize {
        self.sig_cache_len
    }
}

/// Background worker: fetch cesty, kaskáda, cache podpisů. Jediné vlákno,
/// takže cache nepotřebuje zámky.
fn worker(
    rx: Receiver<Job>,
    tx_done: Sender<Done>,
    mut tables: Tables,
    icons: IconStore,
    generation: Arc<AtomicU64>,
) {
    let _ = win_sys::threading::set_current_thread_below_normal();
    let mut sig_cache: HashMap<SigKey, Identity> = HashMap::new();
    // Služba startuje při bootu dřív, než se kdokoli přihlásí, takže
    // hive uživatele v HKEY_USERS ještě není a jeho aplikace „jen pro
    // mě" v tabulkách chybí. Jednou za minutu se levně podívá, jestli
    // se množina přihlášených nezměnila, a pak tabulky načte znovu.
    const KONTROLA: std::time::Duration = std::time::Duration::from_secs(60);
    let mut posledni = std::time::Instant::now();

    loop {
        let job = match rx.recv_timeout(KONTROLA) {
            Ok(job) => Some(job),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if posledni.elapsed() >= KONTROLA {
            posledni = std::time::Instant::now();
            if user_sids() != tables.sids {
                tables = load_tables();
                // Cache drží hotové identity — se starými tabulkami by
                // přežil i `sig:` výsledek aplikace, která teď má `app:`.
                sig_cache.clear();
                generation.fetch_add(1, Ordering::Relaxed);
            }
        }
        let Some(job) = job else { continue };
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

        // Ikona aplikace (drahé GDI, tady na BELOW_NORMAL vlákně).
        // Klíč bez ikony se zkouší znovu s každým dalším procesem téže
        // aplikace (jiné .exe může ikonu mít — např. os:windows ji dodá
        // explorer.exe); fallback je DisplayIcon z uninstall registru.
        let need_icon = {
            let map = icons.lock().expect("icon cache lock");
            !matches!(map.get(&identity.identity_key), Some(Some(_)))
        };
        if need_icon {
            let mut ico = path.as_deref().and_then(win_sys::icon::extract);
            if ico.is_none() {
                if let Some(spec) = tables.icons.get(&identity.identity_key) {
                    ico = win_sys::icon::extract_spec(spec);
                }
            }
            let mut map = icons.lock().expect("icon cache lock");
            // None nepřepisovat přes dřívější úspěch (souběh s retry).
            if ico.is_some() || !matches!(map.get(&identity.identity_key), Some(Some(_))) {
                map.insert(
                    identity.identity_key.clone(),
                    ico.map(|i| IconData {
                        w: i.w,
                        h: i.h,
                        rgba: i.rgba,
                    }),
                );
            }
        }

        let protection = protection(job.pid, &job.image_name);
        if tx_done
            .send(Done {
                pid: job.pid,
                birth: job.birth,
                identity,
                protection,
            })
            .is_err()
        {
            break;
        }
    }
}

/// Hive lidských účtů načtené v HKEY_USERS (S-1-5-21-*, bez `_Classes`).
/// Mění se přihlášením a odhlášením — podle toho worker pozná, že má
/// tabulky načíst znovu.
fn user_sids() -> Vec<String> {
    let mut sids: Vec<String> = win_sys::registry::enum_subkeys(win_sys::registry::HKEY_USERS, "")
        .into_iter()
        .filter(|s| s.starts_with("S-1-5-21") && !s.ends_with("_Classes"))
        .collect();
    sids.sort();
    sids
}

/// Načte uninstall záznamy z registru (SPEC kap. 5.1). Při startu
/// a znovu, když se změní přihlášení uživatelé (viz `worker`).
pub fn load_tables() -> Tables {
    use win_sys::registry::{
        enum_subkeys, read_string, read_u64, RegKey, HKEY_LOCAL_MACHINE, HKEY_USERS,
    };
    let mut uninstall = Vec::new();
    // HKEY_CURRENT_USER tu být NESMÍ: identitu počítá služba pod účtem
    // SYSTEM a její HKCU je hive SYSTEMU s prázdnou větví Uninstall.
    // Aplikace instalované „jen pro mě" (Discord, VS Code User,
    // Modrinth, Roblox…) pak v kaskádě nenašly InstallLocation, dostaly
    // klíč `sig:`/`path:` a nespárovaly se s řádkem `app:` v Programech —
    // bez ikony z DisplayIcon a bez akcí, které chtějí `app:`. Kořeny
    // jsou stejné jako v inventáři (collector-inv), aby klíče seděly.
    let mut roots: Vec<(RegKey, String)> = vec![
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall".to_string(),
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall".to_string(),
        ),
    ];
    let sids = user_sids();
    for sid in &sids {
        for w in ["", r"\WOW6432Node"] {
            roots.push((
                HKEY_USERS,
                format!(r"{sid}\SOFTWARE{w}\Microsoft\Windows\CurrentVersion\Uninstall"),
            ));
        }
    }
    let mut icons = HashMap::new();
    for (root, base) in roots {
        let base = base.as_str();
        for sub in enum_subkeys(root, base) {
            let key = format!("{base}\\{sub}");
            // Bez DisplayName to není aplikace pro uživatele, jen stub.
            //
            // Windows si u vestavěných aplikací (Malování, Výstřižky,
            // Připojení ke vzdálené ploše) zakládá záznam, který jméno
            // drží jen v MUI hodnotě DisplayName_Localized. Jméno
            // podklíče použité místo něj vyrobilo aplikaci
            // „mspaint-b330ad9e-f80b-4c96-9949-4b4228be9a6e" a sebralo
            // pod ni každou nemicrosoftí binárku pod System32.
            // Inventář takové záznamy přeskakuje odjakživa
            // (collector-inv), tady to chybělo.
            let Some(name) = read_string(root, &key, "DisplayName") else {
                continue;
            };
            if name.trim().is_empty() || read_u64(root, &key, "SystemComponent") == Some(1) {
                continue;
            }
            // DisplayIcon jako fallback zdroj ikony (SPEC 5.2).
            if let Some(ico) = read_string(root, &key, "DisplayIcon") {
                if !ico.trim().is_empty() {
                    icons.insert(format!("app:{}", name.to_ascii_lowercase()), ico);
                }
            }
            let Some(loc) = read_string(root, &key, "InstallLocation")
                .as_deref()
                .and_then(install_prefix)
            else {
                continue;
            };
            uninstall.push(UninstallEntry { loc, name, collection: false });
        }
    }
    // Nejdelší prefix první (nejspecifičtější InstallLocation vyhrává).
    uninstall.sort_by_key(|e| std::cmp::Reverse(e.loc.len()));
    uninstall.dedup_by(|a, b| a.loc == b.loc);
    mark_collection_dirs(&mut uninstall);
    tracing::info!(
        count = uninstall.len(),
        icons = icons.len(),
        "načteny uninstall záznamy pro identitu"
    );
    Tables {
        uninstall,
        icons,
        sids,
    }
}

/// `InstallLocation` → porovnatelný prefix cesty, nebo `None`, když je
/// záznam k identifikaci nepoužitelný.
///
/// Do registru si leccos zapíše kořen disku nebo systémový adresář:
/// Blender 2.93 má `InstallLocation` „D:\", stuby vestavěných aplikací
/// „C:\WINDOWS\System32\". Takový prefix sedne na tisíce cizích cest,
/// a protože vyhrává nejdelší shoda, přebije všechno ostatní —
/// `D:\steam\steam.exe` pak vyšel jako aplikace „blender" a
/// `NVDisplay.Container.exe` z DriverStore jako „mspaint-…".
///
/// Uvozovky kolem cesty a lomítka dopředu zapisuje část instalátorů.
fn install_prefix(raw: &str) -> Option<String> {
    let cleaned = raw.trim().trim_matches('"').trim().replace('/', "\\");
    let lc = cleaned.trim_end_matches('\\').to_ascii_lowercase();
    // „d:" — celý disk, žádná aplikace.
    if lc.len() <= 2 || !lc.contains('\\') {
        return None;
    }
    let env = |k: &str| std::env::var(k).ok().map(|v| v.to_ascii_lowercase());
    let sysroot = env("SystemRoot").unwrap_or_else(|| r"c:\windows".into());
    let sysroot = sysroot.trim_end_matches('\\');
    // Sdílené kontejnery — nejsou to instalační adresáře jedné aplikace.
    let generic = [
        sysroot.to_string(),
        format!(r"{sysroot}\system32"),
        format!(r"{sysroot}\syswow64"),
        env("ProgramFiles").unwrap_or_else(|| r"c:\program files".into()),
        env("ProgramFiles(x86)").unwrap_or_else(|| r"c:\program files (x86)".into()),
        env("ProgramData").unwrap_or_else(|| r"c:\programdata".into()),
        env("PUBLIC").unwrap_or_else(|| r"c:\users\public".into()),
    ];
    if generic.iter().any(|g| g.trim_end_matches('\\') == lc) || profile_container(&lc) {
        return None;
    }
    Some(lc)
}

/// Je cesta sdílený kontejner uvnitř profilu — profil sám, AppData,
/// Local, Roaming nebo Local\Programs? Záznamy „jen pro mě" občas
/// zapíšou jako InstallLocation právě tohle a jako nejdelší shoda by
/// pak přebily podpisy všeho, co v profilu běží. Tvar se pozná vzorem,
/// ne z proměnných prostředí: služba běží pod SYSTEM a její %APPDATA%
/// je profil SYSTEMU.
fn profile_container(lc: &str) -> bool {
    let env = |k: &str| std::env::var(k).ok().map(|v| v.to_ascii_lowercase());
    let drive = env("SystemDrive").unwrap_or_else(|| "c:".into());
    let Some(rest) = lc.strip_prefix(&format!(r"{}\users\", drive.trim_end_matches('\\'))) else {
        return false;
    };
    let parts: Vec<&str> = rest.split('\\').collect();
    matches!(
        parts.as_slice(),
        [_] | [_, "appdata"]
            | [_, "appdata", "local"]
            | [_, "appdata", "roaming"]
            | [_, "appdata", "locallow"]
            | [_, "appdata", "local", "programs"]
    )
}

/// Označí instalační adresáře, které jsou nadřazené jiné instalaci.
///
/// Sběrné adresáře seznam pevných jmen nezachytí — jsou to úplně
/// legitimní záznamy. Naměřeno: Minecraft Launcher má
/// `InstallLocation = D:\hry\` a jeho binárka tam opravdu leží, jenže
/// v témž adresáři jsou i Genshin Impact, Star Rail a Star Stable.
/// Prefixová shoda pak `D:\hry\Star Rail Games\StarRail.exe` ohlásila
/// jako aplikaci „Minecraft Launcher" s confidence Exact — tedy přesně
/// to, co u „Blender má D:\" tenhle modul zavíral, jen o patro níž.
///
/// Poznává se to tvarem dat, ne jmény: pod prefixem leží jiný prefix
/// z téhle tabulky, takže to není adresář jedné aplikace.
///
/// Záznam se ale NESMÍ zahodit. `D:\hry` je zároveň skutečné bydliště
/// Minecraft Launcheru (leží tam `MinecraftLauncher.exe` i jeho vlastní
/// `game\`, `runtime\`, `tools\`) — po zahození spadly jeho binárky
/// o krok níž na podpis, tedy do sdíleného klíče
/// `sig:microsoft corporation`, kde už bydlí WebView2, GameInput
/// a PowerShell. Aplikace by v Procesech přišla o vlastní řádek i ikonu
/// a jméno skupiny by určil ten proces, co dorazí první.
///
/// Označený adresář proto platí dál, ale jen pro binárky ležící PŘÍMO
/// v něm (viz `cascade`, krok 3). To zachová Minecraft Launcher
/// a zároveň nepustí sousedy v podadresářích.
///
/// Hranice tohohle pravidla: spustí ho jen existence JINÉHO uninstall
/// záznamu pod prefixem. Kdyby uživatel Rockstar Games Launcher
/// odinstaloval, `D:\hry` se přestane považovat za sběrný adresář
/// a hry v podadresářích se zase začnou hlásit jeho jménem. Poctivější
/// signál (třeba sourozenecké stromy na disku) by ale odstřelil
/// i normální instalace jako `C:\Program Files\Git`, kde binárky
/// v podadresářích leží úplně legitimně.
fn mark_collection_dirs(uninstall: &mut [UninstallEntry]) {
    let vsechny: Vec<String> = uninstall.iter().map(|e| e.loc.clone()).collect();
    for e in uninstall.iter_mut() {
        e.collection = vsechny.iter().any(|jiny| under_dir(jiny, &e.loc));
    }
}

/// Leží cesta uvnitř adresáře? Obojí malými písmeny, `dir` bez koncového
/// „\". Porovnává se na hranici komponenty, ne po znacích — jinak
/// „…\zen browser" sedne i na „…\zen browser nightly\zen.exe".
pub(crate) fn under_dir(path_lc: &str, dir_lc: &str) -> bool {
    path_lc.len() > dir_lc.len()
        && path_lc.starts_with(dir_lc)
        && path_lc.as_bytes()[dir_lc.len()] == b'\\'
}

/// Je cesta pod systémovým adresářem Windows? Na hranici komponenty —
/// prostý prefix bral i `C:\Windows.old\…` nebo `C:\WindowsApps2\…`.
pub(crate) fn under_system_root(path: &str) -> bool {
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    under_dir(
        &path.to_ascii_lowercase(),
        sysroot.trim_end_matches('\\').to_ascii_lowercase().as_str(),
    )
}

/// Parent adresář cesty (pro path fallback).
pub(crate) fn parent_dir(path: &str) -> String {
    Path::new(path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prefix systémového adresáře na hranici komponenty.
    #[test]
    fn system_root_is_matched_on_component_boundary() {
        let sr = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        assert!(under_system_root(&format!(r"{sr}\System32\svchost.exe")));
        assert!(!under_system_root(&format!(r"{sr}.old\System32\evil.exe")));
        assert!(!under_system_root(&format!(r"{sr}Apps2\evil.exe")));
    }

    // Kontejnery v profilu nejsou bydliště jedné aplikace; konkrétní
    // instalace pod nimi ano.
    #[test]
    fn profile_containers_are_not_install_dirs() {
        let d = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        for kontejner in [
            r"\Users\IVA",
            r"\Users\IVA\AppData\Local",
            r"\Users\IVA\AppData\Roaming\",
            r"\Users\IVA\AppData\Local\Programs",
        ] {
            assert!(
                install_prefix(&format!("{d}{kontejner}")).is_none(),
                "{kontejner}"
            );
        }
        let discord = install_prefix(&format!(r"{d}\Users\IVA\AppData\Local\Discord"));
        let ocekavano = format!(
            r"{}\users\iva\appdata\local\discord",
            d.to_ascii_lowercase()
        );
        assert_eq!(discord, Some(ocekavano));
        let code = format!(r"{d}\Users\IVA\AppData\Local\Programs\Microsoft VS Code");
        assert!(install_prefix(&code).is_some());
    }
}
