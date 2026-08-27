//! IPC protokol — typy zpráv pro named pipe (SPEC kap. 10).
//!
//! v0 obsahuje jen ping/pong („žiješ?“). Enumy jsou od začátku
//! `#[non_exhaustive]`-friendly tvarem (další varianty přibudou ve v1+),
//! serializace postcard — proto derive serde na všem.

use serde::{Deserialize, Serialize};

/// Verze IPC protokolu. UI a služba si ji vymění při připojení;
/// neshoda znamená „čekám na dokončení aktualizace“ (INFRA kap. 4.3).
///
/// POVYŠUJ JI V TOMTÉŽ COMMITU, KTERÝ MĚNÍ TVAR JAKÉKOLI ZPRÁVY.
/// Postcard není sebepopisný formát: přidané pole posune čtení všech
/// následujících. Když se přidá pole a verze zůstane, obě strany si
/// odsouhlasí shodu a přitom čtou jinak — starý klient si u
/// `HardwareReport` přečte razítko času z bajtu příznaku `Option`,
/// takže vyjde rok 1970 a nikde se nic nezhroutí. Tichý nesmysl je
/// horší než chybová hláška, protože ho nikdo nenahlásí.
pub const PROTOCOL_VERSION: u32 = 48;

/// Požadavek UI → služba.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// „Žiješ?“ — nese verzi protokolu klienta.
    Ping { protocol_version: u32 },
    /// Aktuální snapshot procesů (1 Hz sampler, SPEC kap. 3.1).
    QueryProcs,
    /// Aktuální systémové metriky (CPU, RAM).
    QuerySystem,
    /// Vlastní spotřeba nástroje (SPEC kap. 2.3) — rozpočet musí být
    /// ověřitelný uživatelem, ne slibovaný.
    QuerySelfUsage,
    /// Kde leží databáze a kam by se dala přesunout.
    QueryDbLocation,
    /// Přesune databázi jinam. Prázdný `dir` = zpátky na výchozí místo.
    /// Sama akce jen ověří místo a zapíše přání do configu — přestěhuje
    /// se až při příštím startu služby, kdy databázi nikdo nedrží.
    SetDbDir { dir: String },
    /// Historie systémových metrik ze system_1s (unix rozsah, včetně).
    QuerySystemHistory { from: i64, to: i64 },
    /// Stav procesů v konkrétním čase (nejbližší vzorek ±2 s).
    QueryProcsAt { ts: i64 },
    /// Statické informace o komponentách (CPU/RAM/GPU/disky).
    QuerySysInfo,
    /// Detaily proměnných v čase (jádra, disky, GPU) — pro zámek grafu.
    QueryDetailAt { ts: i64 },
    /// Historie disků pro per-disk grafy.
    QueryDiskHistory { from: i64, to: i64 },
    /// Historie jader CPU (mini grafy při zámku času).
    QueryCoreHistory { from: i64, to: i64 },
    /// Ikona aplikace podle identity_key (extrahovaná z .exe, cache).
    QueryIcon { identity_key: String },
    /// Události (záseky, pády) v rozsahu — markery na časové ose (v3).
    QueryEvents { from: i64, to: i64 },
    /// Poslední incidenty (nejnovější první), max `limit`.
    QueryIncidents { limit: u32 },
    /// Inventář aplikací (v4, SPEC kap. 5).
    QueryApps,
    /// Mapa souborů aplikace.
    QueryAppMap { identity_key: String },
    /// Spočítá velikosti cest aplikace (pomalé — on-demand) a vrátí
    /// čerstvou mapu; výsledky se cachují do DB.
    ComputeAppSizes { identity_key: String },
    /// Vyžádá nový sken inventáře na pozadí.
    RescanApps,
    /// Jak je na tom sken inventáře. Sken trvá přes 20 s — bez tohohle
    /// by „Obnovit" v UI jen mlčelo a seznam by se změnil až někdy.
    QueryInvStatus,
    /// Svazky + zdraví fyzických disků (v4, SPEC kap. 11.1).
    QueryVolumes,
    /// Postaví MFT index svazku (sekundy; index drží služba v paměti
    /// a po 5 min nečinnosti ho uvolní).
    BuildFileIndex { letter: char },
    /// Hledání v MFT indexu svazku.
    SearchFiles {
        letter: char,
        query: String,
        limit: u32,
    },
    /// Duplicity pod kořenem — dvoufázová čtecí analýza (SPEC 11.3).
    FindDuplicates { root: String, min_size: u64 },
    /// Stav auto-indexace + úklidové analýzy (v4E).
    QueryCleanup,
    /// Smaže záznam incidentu (jen náš DB záznam, žádná mutace OS).
    DeleteIncident { id: i64 },
    /// T0 akce: validace + provedení + ověření v jednom (v5, SPEC 17.2).
    ToggleAction { action: crate::action::Action },
    /// T1 fáze 1: sestavit plán (vrací kroky + expires_ts).
    PlanAction { action: crate::action::Action },
    /// T1 fáze 2–4: provést potvrzený plán (expirovaný = zamítnut).
    ExecuteAction { plan_id: u64 },
    /// Poslední auditní záznamy (SPEC 17.6).
    QueryAudit { limit: u32 },
    /// Startup položky — co startuje s Windows (v6, SPEC kap. 7).
    QueryStartup,
    /// Hardwarový přehled: deska, BIOS, baterie, teploty, disky (v9).
    QueryHardware,
    /// Síť: spojení seskupená per aplikace (v9, SPEC kap. 12).
    QueryNetwork,
    /// Připojení: adaptéry, IP konfigurace, WiFi (v9).
    QueryConnection,
    /// Security: stav ochrany + oprávnění aplikací (v9, SPEC kap. 13).
    QuerySecurity,
    /// Users: účty na tomhle počítači a kdo z nich je správce
    /// (v9E, SPEC kap. 14). Čistě čtecí — účty se odsud nespravují.
    QueryUsers,
    /// Ovladače: co v počítači běží, od koho a jak staré (v10, SPEC 6).
    /// Čistě čtecí — instalovat ovladače umí Windows Update, ne my.
    QueryDrivers,
    /// Jak je na tom sběr. Odpovídá na otázku, kterou jinak nejde
    /// položit na dálku: služba běží, ale tabulka je prázdná — proč?
    QueryCollectorHealth,
    /// Hlášení o pádech, která má uložená Windows (SPEC kap. 16),
    /// přeložená do lidské řeči. Čistě čtecí.
    QueryCrashReports { limit: u32 },
    /// Výpisy paměti a hlášení, která patří k jednomu incidentu,
    /// složené do textu pro záznam. Čte je služba — do složek jako
    /// C:\Windows\Minidump a do cizích profilů běžný uživatel nevidí.
    QueryIncidentDumps {
        app: String,
        ts: i64,
        dump_path: String,
    },
    /// Historie použití jedné schopnosti aplikací (v9D): sezení
    /// za posledních `days` dní. ConsentStore drží jen to poslední,
    /// tohle je to, co si služba zapsala sama.
    /// Součty použití VŠECH oprávnění za období najednou (v9D).
    ///
    /// Jeden dotaz místo sedmdesáti: čas u každého řádku má být vidět
    /// rovnou, ne až po kliknutí. Po jednom by to znamenalo dotaz na
    /// každou aplikaci a schopnost zvlášť.
    QueryPermUseTotals { days: u32 },
    QueryPermUse {
        app: String,
        capability: String,
        days: u32,
    },
    /// Kdo drží soubory (v8, Restart Manager) — „proč to nejde smazat".
    QueryHolders { paths: Vec<String> },

    /// Co po aplikaci zbylo na disku (v8, SPEC 5.3) — čistě čtecí.
    QueryLeftovers { identity_key: String },

    /// Odinstalace, krok 2: služba plán ZNOVU validuje, zapíše audit a
    /// vrátí příkaz odinstalátoru — ale NESPOUŠTÍ ho. Spuštění dělá UI
    /// ve své (uživatelské) relaci; služba běží jako SYSTEM v session 0,
    /// kde by odinstalátor neměl viditelnou plochu ani správný HKCU.
    AuthorizeUninstall { plan_id: u64 },
    /// Odinstalace, krok 3: UI hlásí, že odinstalátor doběhl. Služba
    /// ověří registr (fáze 4) a doplní výsledek k auditnímu záznamu.
    ReportUninstall {
        audit_id: i64,
        identity_key: String,
        detail: String,
    },
}

/// Odpověď služba → UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    /// Odpověď na Ping — verze protokolu a uptime služby v sekundách.
    Pong {
        protocol_version: u32,
        uptime_s: u64,
    },
    Procs(Vec<crate::proc::ProcRow>),
    System(crate::proc::SystemSnapshot),
    /// Vlastní spotřeba: CPU %, working set a velikost databáze.
    SelfUsage {
        cpu_pct: f32,
        ws_bytes: u64,
        db_bytes: u64,
    },
    /// Umístění databáze.
    DbLocation {
        /// Kde databáze leží PRÁVĚ TEĎ.
        current_path: String,
        /// Kam si ji uživatel přeje; prázdno = výchozí místo.
        wanted_dir: String,
        /// Výchozí adresář (`%ProgramData%\syswatch`).
        default_dir: String,
        /// Velikost databáze včetně WAL.
        bytes: u64,
        /// Volné místo na svazku, kde teď leží.
        free_bytes: u64,
        /// Čeká se na restart služby, aby se přesun provedl?
        pending: bool,
        /// Proč se poslední přesun nepovedl. Prázdno = nic se nepokazilo.
        move_error: String,
    },
    SystemHistory(Vec<crate::proc::SystemPoint>),
    /// Stav procesů z historie; `ts` = skutečný čas nalezeného vzorku.
    ProcsAt {
        ts: i64,
        rows: Vec<crate::proc::HistProcRow>,
    },
    SysInfo(crate::proc::StaticInfo),
    /// Detaily v čase: jádra, disky, GPU (co historie má).
    DetailAt {
        ts: i64,
        cores: Vec<f32>,
        disks: Vec<crate::proc::DiskRate>,
        gpu: Option<crate::proc::GpuInfo>,
    },
    /// Body historie disků: (ts, index, r_bps, w_bps).
    DiskHistory(Vec<(i64, u32, u64, u64)>),
    /// Body historie jader: (ts, jádro, pct).
    CoreHistory(Vec<(i64, u32, f32)>),
    /// Ikona aplikace (None = zkoušeno, ikona není).
    Icon(Option<crate::proc::IconData>),
    /// Události v rozsahu (v3).
    Events(Vec<crate::proc::EventRow>),
    /// Incidenty (v3).
    Incidents(Vec<crate::proc::IncidentRow>),
    /// Inventář aplikací (v4).
    Apps(Vec<crate::proc::AppRow>),
    /// Mapa souborů aplikace (v4).
    AppMap(Vec<crate::proc::AppPathRow>),
    /// Potvrzení požadavku bez dat (RescanApps).
    Ack,
    /// Stav skenu inventáře: běží zrovna, a kdy naposledy dopadl zápis
    /// do databáze (unix; 0 = od startu služby zatím ani jednou).
    InvStatus {
        scanning: bool,
        last_scan_ts: i64,
    },
    /// Svazky + zdraví disků (v4).
    Volumes {
        volumes: Vec<crate::proc::VolumeRow>,
        health: Vec<crate::proc::DiskHealthRow>,
    },
    /// Index svazku postaven: (písmeno, počet záznamů).
    IndexInfo {
        letter: char,
        entries: u64,
    },
    /// Nálezy hledání (v4).
    Files(Vec<crate::proc::FileHit>),
    /// Výsledek akce (T0 i T1 Execute) — s auditní stopou (v5).
    ActionResult(crate::action::ActionResult),
    /// Plán T1 akce k potvrzení (v5).
    PlanReady(crate::action::ActionPlan),
    /// Auditní záznamy (v5).
    Audit(Vec<crate::action::AuditRow>),
    /// Startup položky (v6).
    Startup(Vec<crate::proc::StartupRow>),
    /// Držitelé souborů (v8).
    Holders(Vec<crate::proc::HolderRow>),
    /// Hardwarový přehled (v9).
    Hardware(crate::proc::HardwareReport),
    /// Spojení per aplikace (v9).
    Network(Vec<crate::proc::AppNetRow>),
    /// Stav připojení (v9).
    Connection(crate::proc::ConnectionReport),
    /// Security: ochrana + oprávnění (v9).
    Security(crate::proc::SecurityReport),
    /// Účty a správci (v9E).
    Users(crate::proc::UsersReport),
    /// Přehled ovladačů (v10).
    Drivers(crate::proc::DriversReport),
    /// Stav sběru (diagnostika prázdné tabulky).
    CollectorHealth(crate::proc::CollectorHealth),
    /// Přeložená hlášení o pádech.
    CrashReports(Vec<crate::proc::CrashReportRow>),
    /// Text výpisů k incidentu.
    IncidentDumps(String),
    /// Součty použití za období: (aplikace, schopnost, sekundy).
    PermUseTotals(Vec<(String, String, i64)>),
    /// Sezení použití oprávnění + součet sekund za období (v9D).
    PermUse {
        sessions: Vec<crate::proc::PermUseRow>,
        total_s: i64,
    },

    /// Zbytky po odinstalaci: cesty, které na disku pořád jsou.
    Leftovers(Vec<String>),
    /// Odinstalace schválena: příkaz ke spuštění v relaci uživatele
    /// a id auditního záznamu, ke kterému se pak hlásí výsledek.
    UninstallAuthorized {
        command: String,
        audit_id: i64,
    },
    /// Skupiny duplicit: (velikost, cesty).
    Duplicates(Vec<(u64, Vec<String>)>),
    /// Stav úklidu: indexace svazků (písmeno, záznamů, hotovo),
    /// běží-li analýza a případný výsledek.
    Cleanup {
        /// (svazek, záznamů, hotovo, chyba — proč nešel indexovat).
        indexing: Vec<(char, u64, bool, Option<String>)>,
        running: bool,
        report: Option<crate::proc::CleanupReport>,
    },
    /// Hotovo, není co vracet.
    Ok,
    /// Chyba zpracování požadavku. Nic neselhává mlčky (SPEC kap. 22).
    Error {
        message: String,
    },
}

#[cfg(test)]
mod wire_shape {
    use crate::proc::*;

    /// Otisk tvaru jedné zprávy.
    ///
    /// Skládá se ze dvou nezávislých měření, protože ani jedno samo
    /// nestačí:
    ///
    /// 1. Délka prázdné struktury v postcardu. Chytí přidané a odebrané
    ///    pole i změnu šířky čísla (`u32` → `u64` u nenulové hodnoty),
    ///    ale je slepá k pořadí a ke jménům.
    /// 2. Tvar téže struktury v JSONu. Ten nese JMÉNA polí v pořadí,
    ///    v jakém jsou deklarovaná, a názvy variant enumů — takže
    ///    prohození dvou polí téhož typu, přejmenování pole nebo
    ///    varianty se projeví, přestože bajtová délka sedí.
    ///
    /// Co ani jedno nechytí: záměnu dvou čísel stejné šířky se stejným
    /// jménem (`u32` → `i32`). Postcard je kóduje jinak (zigzag), takže
    /// je to skutečná změna drátu — kdo takovou úpravu dělá, musí verzi
    /// povýšit sám.
    fn otisk<T: serde::Serialize + Default>() -> (usize, u64) {
        let v = T::default();
        let bajtu = postcard::to_allocvec(&v)
            .expect("prázdná struktura musí jít serializovat")
            .len();
        let tvar = serde_json::to_string(&v).expect("prázdná struktura musí jít do JSONu");
        // FNV-1a — nepotřebujeme kryptografii, jen stabilní číslo.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in tvar.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        (bajtu, h)
    }

    /// Změna tvaru zprávy musí povýšit verzi protokolu.
    ///
    /// Postcard není sebepopisný: přidané pole posune čtení všeho za
    /// ním. Když se pole přidá a `PROTOCOL_VERSION` zůstane, obě strany
    /// si odsouhlasí shodu a přitom čtou jinak — a nemusí to spadnout.
    /// Starý klient si u `HardwareReport` přečte razítko času z bajtu
    /// příznaku `Option` a zobrazí rok 1970. Takovou chybu nikdo
    /// nenahlásí, protože nevypadá jako chyba.
    ///
    /// Když tenhle test spadne: zkontroluj, že jsi povýšil
    /// `PROTOCOL_VERSION`, a teprve pak sem přepiš nové otisky.
    /// NIKDY nepřepisuj jen čísla tady.
    #[test]
    fn zmena_tvaru_zpravy_vyzaduje_povyseni_protokolu() {
        // (jméno, naměřeno, zaznamenáno při PROTOCOL_VERSION 47)
        let mereno: Vec<(&str, (usize, u64), (usize, u64))> = vec![
            ("ProcRow", otisk::<ProcRow>(), (23, 2959247389591744881)),
            ("SystemSnapshot", otisk::<SystemSnapshot>(), (31, 1630485381821562429)),
            ("StaticInfo", otisk::<StaticInfo>(), (11, 16331367557955489898)),
            ("StartupRow", otisk::<StartupRow>(), (12, 5603128357851775481)),
            ("ProtectionReport", otisk::<ProtectionReport>(), (20, 7503998234196962424)),
            ("HardwareReport", otisk::<HardwareReport>(), (19, 16128689981868478521)),
            ("OsInfoRow", otisk::<OsInfoRow>(), (10, 15116235607699317519)),
            ("PagefileRow", otisk::<PagefileRow>(), (4, 9219782284099688547)),
            ("DiskHealthRow", otisk::<DiskHealthRow>(), (7, 6026345417622266159)),
            ("PermissionRow", otisk::<PermissionRow>(), (9, 15657882981265654498)),
            ("ConnectionReport", otisk::<ConnectionReport>(), (4, 11340516523186696761)),
            ("DriversReport", otisk::<DriversReport>(), (3, 2119242957881570790)),
            ("CollectorHealth", otisk::<CollectorHealth>(), (5, 5942219211452029580)),
            ("AppRow", otisk::<AppRow>(), (9, 17305535340744830704)),
            ("IncidentRow", otisk::<IncidentRow>(), (8, 11433128703909839783)),
            ("EventRow", otisk::<EventRow>(), (5, 15093540666795501623)),
            ("HistProcRow", otisk::<HistProcRow>(), (13, 1874953404327417601)),
            ("VolumeRow", otisk::<VolumeRow>(), (8, 12562810835843001531)),
            ("DeviceRow", otisk::<DeviceRow>(), (10, 6588135713776308766)),
            ("DriverRow", otisk::<DriverRow>(), (10, 10978326375890790080)),
            ("NetAdapterRow", otisk::<NetAdapterRow>(), (10, 12005896299422780646)),
            ("AppNetRow", otisk::<AppNetRow>(), (9, 3665581879689679179)),
            ("PermUseRow", otisk::<PermUseRow>(), (2, 4410355863787390850)),
            ("CrashReportRow", otisk::<CrashReportRow>(), (7, 6217742025496804370)),
            ("AuditRow", otisk::<crate::action::AuditRow>(), (9, 12115489750143912336)),
        ];
        let rozdily: Vec<String> = mereno
            .iter()
            .filter(|(_, je, ma)| je != ma)
            .map(|(jmeno, je, _)| format!("(\"{jmeno}\", otisk::<{jmeno}>(), ({}, {})),", je.0, je.1))
            .collect();
        assert!(
            rozdily.is_empty(),
            "tvar zpráv se změnil. Povyš PROTOCOL_VERSION (teď {}) a teprve \
             potom sem přepiš nové otisky:\n{}",
            super::PROTOCOL_VERSION,
            rozdily.join("\n")
        );
    }
}
