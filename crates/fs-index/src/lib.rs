//! fs-index — NTFS MFT/USN prohlížeč (SPEC kap. 11.2, čtecí část).
//!
//! In-memory index celého svazku postavený z MFT za sekundy — žádná
//! vlastní databáze souborů, čte se přímo struktura NTFS. Hledání je
//! lineární průchod s podřetězcem bez ohledu na velikost písmen
//! a diakritiku — i milión záznamů se projde v desítkách ms. Mazání
//! sem NEPATŘÍ (v8, přes validační vrstvu).

pub mod snapshot;

use std::collections::HashMap;

/// Chyby této crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("win-sys: {0}")]
    WinSys(#[from] win_sys::Error),
}

/// FILE_ATTRIBUTE_DIRECTORY.
pub const ATTR_DIR: u32 = 0x10;
/// FILE_ATTRIBUTE_HIDDEN.
pub const ATTR_HIDDEN: u32 = 0x2;
/// FILE_ATTRIBUTE_SYSTEM.
pub const ATTR_SYSTEM: u32 = 0x4;

/// Jeden záznam indexu.
struct Node {
    name: Box<str>,
    parent: u64,
    attrs: u32,
}

/// Index jednoho svazku.
pub struct VolumeIndex {
    pub letter: char,
    nodes: HashMap<u64, Node>,
    /// FileReferenceNumber kořene svazku.
    root: u64,
}

/// Nález hledání.
#[derive(Debug, Clone)]
pub struct Hit {
    pub path: String,
    pub name: String,
    pub attrs: u32,
}

impl VolumeIndex {
    /// Postaví index svazku z MFT (sekundy; volat z pozadí/on-demand).
    pub fn build(letter: char) -> Result<VolumeIndex, Error> {
        Self::build_with(letter, |_| {})
    }

    /// Stavba s průběžným hlášením počtu záznamů (progres do UI).
    pub fn build_with(
        letter: char,
        mut on_progress: impl FnMut(u64),
    ) -> Result<VolumeIndex, Error> {
        let mut nodes = HashMap::new();
        let mut n = 0u64;
        win_sys::usn::enum_volume(letter, |e| {
            n += 1;
            if n.is_multiple_of(20_000) {
                on_progress(n);
            }
            nodes.insert(
                e.file_ref,
                Node {
                    name: e.name.into_boxed_str(),
                    parent: e.parent_ref,
                    attrs: e.attrs,
                },
            );
        })?;
        // Kořen: MFT záznam 5 (nízkých 48 bitů reference čísla).
        let root = nodes
            .keys()
            .copied()
            .find(|k| k & 0x0000_FFFF_FFFF_FFFF == 5)
            .unwrap_or(5);
        Ok(VolumeIndex {
            letter,
            nodes,
            root,
        })
    }

    /// Počet záznamů.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Je index prázdný?
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Celá cesta záznamu (rekonstrukce přes rodiče).
    fn path_of(&self, mut file_ref: u64) -> String {
        let mut parts: Vec<&str> = Vec::new();
        let mut guard = 0;
        while let Some(n) = self.nodes.get(&file_ref) {
            if file_ref == self.root || guard > 64 {
                break;
            }
            parts.push(&n.name);
            file_ref = n.parent;
            guard += 1;
        }
        let mut out = format!("{}:", self.letter);
        for p in parts.iter().rev() {
            out.push('\\');
            out.push_str(p);
        }
        out
    }

    /// Hledání podřetězce v názvech bez ohledu na velikost písmen
    /// a diakritiku (viz `slozit`).
    ///
    /// Dřív se porovnávalo jen ASCII bez ohledu na velikost: „škola"
    /// nenašla „Škola.docx" (Š a š se v UTF-8 liší jinde než v ASCII
    /// bitu) a „skola" nenašla ani „škola.docx". Programy přitom UI
    /// filtruje bez diakritiky, takže tentýž dotaz u nich fungoval
    /// a u souborů ne.
    ///
    /// Vrací nejvýš `limit` nálezů, a to TY NEJLEPŠÍ — ne prvních
    /// `limit` nalezených. To je rozdíl, na kterém všechno stojí:
    /// svazek má přes milion záznamů a rozhoduje se tu, kterých dvě stě
    /// z nich uživatel vůbec uvidí. Dřív se braly v pořadí, v jakém
    /// ležely v mapě, takže se ta správná položka nemusela do výsledku
    /// vejít vůbec.
    ///
    /// Řadí se podle toho, KDE v názvu shoda začíná: co začíná hledaným
    /// slovem, jde nahoru. Na dotaz „al" tak vyjde „Aluminium" před
    /// „Zákal". Úplná shoda jména je vždycky první. Při stejné poloze
    /// vyhrává kratší jméno (méně přílepků kolem hledaného) a pak cesta
    /// blíž ke kořeni.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let q = query.trim();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }
        let q_f = slozit(q);
        let q_ascii = q_f.is_ascii();

        // Nejdřív se sbírají jen klíče a skóre. Skládat cesty (což je
        // chůze po rodičích až ke kořeni) pro každý nález by u obecného
        // dotazu znamenalo statisíce zbytečných řetězců — dělá se to až
        // pro tu dvoustovku, která se opravdu vrátí.
        let mut kandidati: Vec<(u8, u32, u32, u64)> = Vec::new();
        for (file_ref, n) in &self.nodes {
            // Rychlá cesta bez alokace: ASCII jméno se složeným ASCII
            // dotazem stačí porovnat po bajtech. Takových je na svazku
            // drtivá většina; skládat je všechna by u každého hledání
            // alokovalo milion řetězců.
            let (kde, delka) = if n.name.is_ascii() {
                if !q_ascii {
                    // Složený dotaz s ne-ASCII znakem (ł, ø, CJK…)
                    // v čistě ASCII jménu být nemůže.
                    continue;
                }
                match index_ignore_ascii_case(&n.name, &q_f) {
                    Some(k) => (k, n.name.len()),
                    None => continue,
                }
            } else {
                let f = slozit(&n.name);
                match f.find(&q_f) {
                    Some(k) => (k, f.len()),
                    None => continue,
                }
            };
            // Délka i poloha se berou ze složeného tvaru, jinak by
            // „Škola" na dotaz „skola" nevyšla jako úplná shoda.
            let presna = (delka == q_f.len()) as u8;
            kandidati.push((1 - presna, kde as u32, delka as u32, *file_ref));
        }
        if kandidati.len() > limit {
            kandidati.select_nth_unstable(limit - 1);
            kandidati.truncate(limit);
        }
        kandidati.sort_unstable();

        let mut out: Vec<Hit> = kandidati
            .into_iter()
            .filter_map(|(_, _, _, file_ref)| {
                let n = self.nodes.get(&file_ref)?;
                Some(Hit {
                    path: self.path_of(file_ref),
                    name: n.name.to_string(),
                    attrs: n.attrs,
                })
            })
            .collect();
        // Poslední slovo má hloubka cesty, ale jen mezi jinak stejně
        // dobrými nálezy — proto stabilní řazení až tady.
        out.sort_by_key(|h| h.path.matches('\\').count());
        out
    }
}

/// Skupina duplicitních souborů (stejná velikost + stejný obsah).
#[derive(Debug, Clone)]
pub struct DupGroup {
    pub size: u64,
    pub paths: Vec<String>,
}

/// Duplicity pod kořenem — dvoufázově (SPEC 11.3): nejdřív seskupení
/// podle velikosti (zadarmo z metadat), hash obsahu se počítá JEN pro
/// kandidáty se shodnou velikostí. Čtecí analýza, žádné mazání (v8).
/// `max_files` je pojistka proti obřím stromům.
///
/// Kořen přichází z pipe od kteréhokoli přihlášeného uživatele a projde
/// ho služba pod účtem SYSTEM. Proto se bere jen místní cesta s písmenem
/// disku bez přesměrování (viz `mistni_koren`): UNC cesta by službu
/// přiměla přihlásit se účtem počítače k cizímu serveru (NTLM relay)
/// a junction nebo symlink v cestě by ji odvedl jinam, než kam
/// kontrola řetězce viděla.
pub fn find_duplicates(root: &str, min_size: u64, max_files: usize) -> Vec<DupGroup> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    if !mistni_koren(root) {
        return Vec::new();
    }

    // Fáze 1: velikost → cesty.
    let mut by_size: HashMap<u64, Vec<std::path::PathBuf>> = HashMap::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    let mut seen_files = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(e.path());
            } else if meta.len() >= min_size {
                by_size.entry(meta.len()).or_default().push(e.path());
                seen_files += 1;
                if seen_files >= max_files {
                    stack.clear();
                    break;
                }
            }
        }
    }

    // Fáze 2: hash obsahu kandidátů (po 1MB blocích; SipHash stačí na
    // detekci — nejde o kryptografii, jen o „stejný obsah?").
    let hash_file = |path: &std::path::Path| -> Option<u64> {
        use std::io::Read;
        let mut f = std::fs::File::open(path).ok()?;
        let mut h = DefaultHasher::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            h.write(&buf[..n]);
        }
        Some(h.finish())
    };

    let mut out = Vec::new();
    for (size, paths) in by_size {
        if paths.len() < 2 {
            continue;
        }
        let mut by_hash: HashMap<u64, Vec<String>> = HashMap::new();
        for p in paths {
            if let Some(h) = hash_file(&p) {
                by_hash
                    .entry(h)
                    .or_default()
                    .push(p.to_string_lossy().into_owned());
            }
        }
        for (_, group) in by_hash {
            if group.len() >= 2 {
                out.push(DupGroup { size, paths: group });
            }
        }
    }
    // Největší plýtvání první: (počet-1) × velikost.
    out.sort_by_key(|g| std::cmp::Reverse(g.size * (g.paths.len() as u64 - 1)));
    out.truncate(100);
    out
}

/// Výsledek úklidové analýzy (SPEC 11.3 rozšířeno): potvrzené duplicity
/// napříč svazky, soubory s nulovou velikostí a známé junk adresáře.
#[derive(Debug, Clone, Default)]
pub struct CleanupReport {
    /// (velikost, cesty) — stejné jméno + velikost + hash obsahu.
    pub dups: Vec<(u64, Vec<String>)>,
    pub zero_byte: Vec<String>,
    /// (cesta, velikost) — temp/cache adresáře k úklidu.
    pub junk: Vec<(String, u64)>,
}

/// Přípony, u kterých duplicity uživatele zajímají (média, archivy,
/// dokumenty, instalátory) — systémové dll/manifesty jsou šum.
const DUP_EXTS: &[&str] = &[
    "zip", "rar", "7z", "iso", "mp4", "mkv", "avi", "mov", "mp3", "flac", "wav", "jpg", "jpeg",
    "png", "heic", "gif", "pdf", "docx", "xlsx", "pptx", "doc", "exe", "msi", "psd", "blend",
];

/// Úklidová analýza nad postavenými indexy. Třífázově: kandidáti podle
/// stejného JMÉNA z MFT (zadarmo), velikost přes metadata (jen
/// kandidáti), potvrzení hashem obsahu. Jen čte.
pub fn cleanup_analysis(indexes: &[&VolumeIndex]) -> CleanupReport {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut report = CleanupReport::default();

    // ── Kandidáti: jméno_lc → cesty (jen zajímavé přípony) ──
    let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
    for idx in indexes {
        for (file_ref, n) in &idx.nodes {
            if n.attrs & ATTR_DIR != 0 || n.name.len() < 6 {
                continue;
            }
            let Some(ext) = n.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()) else {
                continue;
            };
            if !DUP_EXTS.contains(&ext.as_str()) {
                continue;
            }
            by_name
                .entry(n.name.to_ascii_lowercase())
                .or_default()
                .push(idx.path_of(*file_ref));
        }
    }

    // ── Velikosti kandidátů (stat jen kolizí jmen, s pojistkou) ──
    let hash_file = |path: &str| -> Option<u64> {
        use std::io::Read;
        let mut f = std::fs::File::open(path).ok()?;
        let mut h = DefaultHasher::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            h.write(&buf[..n]);
        }
        Some(h.finish())
    };
    let mut stats = 0usize;
    for (_, paths) in by_name {
        if paths.len() < 2 || paths.len() > 10 || stats > 30_000 {
            continue;
        }
        // Recycle bin a WinSxS nejsou úklid uživatele.
        if paths.iter().any(|p| {
            let lc = p.to_ascii_lowercase();
            lc.contains("\\$recycle") || lc.contains("\\winsxs\\")
        }) {
            continue;
        }
        let mut by_size: HashMap<u64, Vec<String>> = HashMap::new();
        for p in paths {
            stats += 1;
            if let Ok(m) = std::fs::metadata(&p) {
                if m.len() >= 1_000_000 {
                    by_size.entry(m.len()).or_default().push(p);
                }
            }
        }
        // ── Potvrzení obsahem ──
        for (size, group) in by_size {
            if group.len() < 2 {
                continue;
            }
            let mut by_hash: HashMap<u64, Vec<String>> = HashMap::new();
            for p in group {
                if let Some(h) = hash_file(&p) {
                    by_hash.entry(h).or_default().push(p);
                }
            }
            for (_, g) in by_hash {
                if g.len() >= 2 {
                    report.dups.push((size, g));
                }
            }
        }
    }
    report
        .dups
        .sort_by_key(|(size, paths)| std::cmp::Reverse(size * (paths.len() as u64 - 1)));
    report.dups.truncate(100);

    // ── 0bajtové soubory v uživatelských profilech ──
    let users = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + r"\Users";
    let profily = uzivatelske_profily(&users);
    let mut stack: Vec<std::path::PathBuf> = profily.clone();
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > 120_000 || report.zero_byte.len() >= 300 {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_symlink() {
                continue;
            }
            if m.is_dir() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                // AppData je plné legitimních 0B zámků/markerů — šum.
                if name != "appdata" && !name.starts_with('.') {
                    stack.push(e.path());
                }
            } else if m.len() == 0 {
                report
                    .zero_byte
                    .push(e.path().to_string_lossy().into_owned());
            }
        }
    }

    // ── Junk adresáře (temp) — velikost = kolik jde uklidit ──
    let mut junk_paths: Vec<String> = vec![format!(
        "{}\\Temp",
        std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())
    )];
    for profil in &profily {
        let p = profil.join("AppData\\Local\\Temp");
        if p.is_dir() {
            junk_paths.push(p.to_string_lossy().into_owned());
        }
    }
    for p in junk_paths {
        let size = dir_size_bounded(&p, 100_000);
        if size > 0 {
            report.junk.push((p, size));
        }
    }
    report
}

/// Profily skutečných uživatelů pod `C:\Users`.
///
/// Kontrola symlinků dřív platila jen pro potomky, ne pro samotné
/// položky `C:\Users`. „All Users" je ale junction na `C:\ProgramData`
/// a „Default User" na `C:\Users\Default`, takže hledání prázdných
/// souborů „v profilech" prošlo i ProgramData — plné legitimních
/// nulových značek a zámků systémového softwaru — a nabízelo je ke
/// smazání pod cestou `C:\Users\All Users\…`. Vzorové a sdílené
/// profily se vynechávají stejně jako v inventáři (collector-inv).
fn uzivatelske_profily(users: &str) -> Vec<std::path::PathBuf> {
    const NE: &[&str] = &["default", "default user", "public", "all users"];
    let Ok(rd) = std::fs::read_dir(users) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| {
            let jmeno = e.file_name().to_string_lossy().to_lowercase();
            // DirEntry::metadata symlink nenásleduje — junction se tu
            // pozná jako symlink, ne jako adresář.
            !NE.contains(&jmeno.as_str())
                && e.metadata().is_ok_and(|m| m.is_dir() && !m.is_symlink())
        })
        .map(|e| e.path())
        .collect()
}

/// Největší soubory a složky svazku (v4F): jeden průchod stromem,
/// velikosti z directory enumerace (na Windows je `metadata()` na
/// DirEntry zdarma — data pocházejí z FindNextFile). Do mapy složek se
/// dávají jen složky do hloubky `DIR_DEPTH`, ať to má vypovídací
/// hodnotu a nežere paměť — jejich součet ale zahrnuje i obsah
/// libovolně hluboko pod nimi.
pub struct BigItems {
    /// (cesta, velikost) — největší jednotlivé soubory.
    pub files: Vec<(String, u64)>,
    /// (cesta, velikost) — největší složky (součet obsahu).
    pub dirs: Vec<(String, u64)>,
    /// Průchod narazil na strop `max_entries` a výsledek je neúplný.
    /// Protokol to zatím do UI nenese; aspoň volající to ví a může to
    /// zalogovat.
    pub truncated: bool,
}

/// Do jaké hloubky se agregují velikosti složek.
const DIR_DEPTH: usize = 4;

/// Projde svazek a vrátí top N souborů a složek dle velikosti.
pub fn largest_items(root: &str, top_n: usize, max_entries: usize) -> BigItems {
    largest_items_until(root, top_n, max_entries, &|| false)
}

/// Totéž, ale s možností práci přerušit.
///
/// Průchod stromem umí trvat minuty — naměřeno 290 s na zaplněném disku.
/// Bez téhle možnosti drží celý ten čas službu při životě: signál
/// „zastav se" dorazí, ale démon ho nemá jak předat dovnitř smyčky.
/// Instalátor pak marně čeká na zastavení a ohlásí chybu, přestože je
/// všechno v pořádku — jen se zrovna počítá.
pub fn largest_items_until(
    root: &str,
    top_n: usize,
    max_entries: usize,
    cancelled: &dyn Fn() -> bool,
) -> BigItems {
    let mut files: Vec<(String, u64)> = Vec::new();
    let mut dirs: HashMap<String, u64> = HashMap::new();
    // Práh pro udržení souboru v kandidátech (roste, ať Vec nepřeteče).
    let mut min_file: u64 = 1_000_000;
    // Do šířky, ne do hloubky. Zásobník (LIFO) s abecedně řazeným
    // read_dir bral kořenové složky od konce: na systémovém svazku
    // s 1,8 milionu záznamů došel strop `max_entries` uvnitř Windows
    // a Users a Program Files ani ProgramData se vůbec neprošly —
    // v „Největších" chyběly úplně, aniž by to cokoli prozradilo.
    // Fronta projde nejdřív všechny mělké úrovně, takže strop ořeže
    // jen hluboké větve rovnoměrně napříč svazkem.
    let mut fronta: std::collections::VecDeque<(std::path::PathBuf, usize)> =
        std::collections::VecDeque::from([(root.into(), 0)]);
    let mut seen = 0usize;
    let mut truncated = false;

    while let Some((dir, depth)) = fronta.pop_front() {
        if cancelled() {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            // Kontrola po tisícovkách položek: čtení atomického příznaku
            // je levné, ale v nejvnitřnější smyčce se to sčítá.
            if seen % 4096 == 0 && cancelled() {
                fronta.clear();
                break;
            }
            if seen > max_entries {
                truncated = true;
                fronta.clear();
                break;
            }
            let Ok(m) = e.metadata() else { continue };
            if m.is_symlink() {
                continue;
            }
            let path = e.path();
            if m.is_dir() {
                // Systémové/servisní stromy do „největších" nepatří.
                let lc = path.to_string_lossy().to_ascii_lowercase();
                if lc.contains("\\$recycle") || lc.contains("\\windows\\winsxs") {
                    continue;
                }
                fronta.push_back((path, depth + 1));
            } else {
                let size = m.len();
                if size >= min_file {
                    files.push((path.to_string_lossy().into_owned(), size));
                    if files.len() > top_n * 4 {
                        files.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
                        files.truncate(top_n);
                        min_file = files.last().map(|(_, s)| *s).unwrap_or(min_file);
                    }
                }
                // Přičíst do předků v hloubce 1..=DIR_DEPTH. Hlubší
                // předkové se do mapy nedávají, ale obsah se do mělčích
                // složek započítat musí. Dřív smyčka u souboru hlouběji
                // než DIR_DEPTH skončila hned na první úrovni a soubor
                // se nepřičetl nikomu: C:\Users ukazoval 12 GB, přestože
                // jen AppData jednoho profilu má přes 190 GB.
                if let Some(rodic) = path.parent() {
                    for p in predkove_do_mapy(rodic, depth) {
                        *dirs.entry(p.to_string_lossy().into_owned()).or_insert(0) += size;
                    }
                }
            }
        }
    }

    files.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
    files.truncate(top_n);
    let mut dirs: Vec<(String, u64)> = dirs.into_iter().collect();
    dirs.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
    // Vyhodit složky, které jsou jen předkem už uvedené větší složky
    // se stejnou velikostí (jinak by seznam byl řetěz jedné cesty).
    let mut kept: Vec<(String, u64)> = Vec::new();
    for (p, s) in dirs {
        let redundant = kept.iter().any(|(kp, ks)| {
            (je_predek(&p, kp) && *ks * 10 > s * 9) || (je_predek(kp, &p) && s * 10 > *ks * 9)
        });
        if !redundant {
            kept.push((p, s));
        }
        if kept.len() >= top_n {
            break;
        }
    }
    BigItems {
        files,
        dirs: kept,
        truncated,
    }
}

/// Předkové souboru, kterým se přičítá jeho velikost: `rodic` leží
/// v hloubce `depth` a vrací se úrovně od min(depth, DIR_DEPTH) nahoru
/// po hloubku 1 (kořen svazku ne). Hlubší úrovně se přeskočí.
fn predkove_do_mapy(
    rodic: &std::path::Path,
    depth: usize,
) -> impl Iterator<Item = &std::path::Path> {
    rodic
        .ancestors()
        .skip(depth.saturating_sub(DIR_DEPTH))
        .take(depth.min(DIR_DEPTH))
}

/// Je `predek` nadřazená složka `cesta`? Na hranici komponenty: prostý
/// prefix řetězce bral „C:\Program Files" jako předka „C:\Program Files
/// (x86)" a „C:\Users\IVA" jako předka „C:\Users\IVAN" — filtr
/// redundance pak tiše vyhodil úplně jinou, skutečnou složku.
fn je_predek(predek: &str, cesta: &str) -> bool {
    let predek = predek.trim_end_matches('\\');
    cesta.len() > predek.len()
        && cesta.starts_with(predek)
        && cesta.as_bytes()[predek.len()] == b'\\'
}

/// Velikost adresáře s pojistkou na počet položek.
fn dir_size_bounded(path: &str, max_entries: usize) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![std::path::PathBuf::from(path)];
    let mut n = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            n += 1;
            if n > max_entries {
                return total;
            }
            let Ok(m) = e.metadata() else { continue };
            if m.is_symlink() {
                continue;
            }
            if m.is_dir() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    total
}

/// Podřetězec bez alokace: `needle_lc` už je lowercase.
/// Kde v `haystack` začíná `needle_lc` (ASCII case-insensitive)?
///
/// Poloha je to, podle čeho se výsledky řadí: co začíná hledaným
/// slovem, patří výš než to, co ho má někde uvnitř.
fn index_ignore_ascii_case(haystack: &str, needle_lc: &str) -> Option<usize> {
    if needle_lc.is_empty() {
        return Some(0);
    }
    let h = haystack.as_bytes();
    let n = needle_lc.as_bytes();
    if h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| {
        h[i..i + n.len()]
            .iter()
            .zip(n)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

/// Tvar jména pro porovnání: malá písmena a bez diakritiky.
///
/// Má dávat totéž co `bezDiakritiky` v UI (toLowerCase + NFD + pryč
/// s combining znaky), jinak by hledání souborů a programů v liště
/// vracelo pro stejný dotaz různé věci. Bez nové závislosti: tabulka
/// pokrývá písmena Latin-1 a Latin Extended-A, která mají kanonický
/// rozklad (ł, đ, ø ho nemají a NFD je nechá být — tady také).
/// Samostatné combining znaky se zahazují, takže sedí i jména uložená
/// v rozloženém tvaru.
pub fn slozit(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        if ('\u{300}'..='\u{36f}').contains(&c) {
            continue;
        }
        out.push(bez_diakritiky(c));
    }
    out
}

/// Základní písmeno malého písmena s diakritikou (viz `slozit`).
fn bez_diakritiky(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        _ => c,
    }
}

/// Je `root` místní cesta, kterou smí služba (SYSTEM) projít na žádost
/// z pipe? Jen absolutní cesta s písmenem disku („X:\…") bez `.`/`..`,
/// bez alternativních proudů a bez reparse pointů v kterékoli složce
/// cesty.
///
/// UNC (`\\server\share`) a cesty zařízení (`\\?\`, `\\.\`) se odmítají:
/// pipe smí volat každý přihlášený uživatel a služba by se k cizímu
/// serveru přihlásila účtem počítače. Junction nebo symlink po cestě by
/// totéž udělal oklikou (symlink smí mířit i na UNC), proto se každá
/// složka kontroluje bez následování (`symlink_metadata`) dřív, než se
/// do ní vstoupí.
fn mistni_koren(root: &str) -> bool {
    if !mistni_tvar(root) {
        return false;
    }
    // Odmítá se jen odkaz, který vede jinam (symlink, junction — std je
    // na Windows hlásí jako is_symlink). Obecný příznak reparse pointu
    // nesou i cloudové složky iCloud a OneDrive; ty jsou místní a dřív se
    // kvůli němu odmítly celé, s tichým prázdným výsledkem.
    let mut cesta = std::path::PathBuf::from(&root[..3]);
    for kus in root[3..].split('\\').filter(|k| !k.is_empty()) {
        cesta.push(kus);
        match std::fs::symlink_metadata(&cesta) {
            Ok(m) if !m.file_type().is_symlink() => {}
            _ => return false,
        }
    }
    true
}

/// Čistě textová část `mistni_koren` (testovatelná bez disku).
fn mistni_tvar(root: &str) -> bool {
    let b = root.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
        return false;
    }
    // Lomítko dopředu Windows berou jako oddělovač, `:` dál v cestě
    // je alternativní datový proud — obojí by obcházelo kontrolu níž.
    if root.contains('/') || root[2..].contains(':') {
        return false;
    }
    // Složka jen z teček a mezer: Windows koncové tečky a mezery
    // zahazují, takže i „.. " je ve skutečnosti návrat o patro výš.
    root[3..]
        .split('\\')
        .filter(|k| !k.is_empty())
        .all(|k| !k.trim_end_matches([' ', '.']).is_empty())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_matches_case_insensitive() {
        assert_eq!(index_ignore_ascii_case("Config.SYS", "config"), Some(0));
        assert_eq!(index_ignore_ascii_case("abcDEF", "cde"), Some(2));
        assert_eq!(index_ignore_ascii_case("abc", "abcd"), None);
    }

    /// Poloha shody je to, podle čeho se řadí — na dotaz „al" musí
    /// „Aluminium" (poloha 0) vyjít před „Zakal" (poloha 3).
    #[test]
    fn poloha_shody_urcuje_poradi() {
        let a = index_ignore_ascii_case("Aluminium", "al").unwrap();
        let z = index_ignore_ascii_case("Zakal", "al").unwrap();
        assert!(a < z, "a={a} z={z}");
        // Rychlá cesta pro ASCII jména zůstává bez ohledu na velikost.
        assert_eq!(index_ignore_ascii_case("ALUMINIUM", "al"), Some(0));
    }

    fn index_se_jmeny(jmena: &[&str]) -> VolumeIndex {
        let mut nodes = HashMap::new();
        nodes.insert(
            5u64,
            Node {
                name: "".into(),
                parent: 5,
                attrs: ATTR_DIR,
            },
        );
        for (i, j) in jmena.iter().enumerate() {
            nodes.insert(
                100 + i as u64,
                Node {
                    name: (*j).into(),
                    parent: 5,
                    attrs: 0,
                },
            );
        }
        VolumeIndex {
            letter: 'C',
            nodes,
            root: 5,
        }
    }

    /// Česká jména s velkým písmenem s háčkem se musí najít malými
    /// písmeny i bez diakritiky — dřív vracel index pro „škola"
    /// u „Škola.docx" prázdno.
    #[test]
    fn hledani_ignoruje_velikost_i_diakritiku() {
        let idx = index_se_jmeny(&["Škola.docx", "ÚČTENKA.pdf", "skola", "Config.SYS"]);
        let jmena = |q: &str| -> Vec<String> {
            let mut v: Vec<String> = idx.search(q, 10).into_iter().map(|h| h.name).collect();
            v.sort();
            v
        };
        assert_eq!(jmena("škola"), vec!["skola", "Škola.docx"]);
        assert_eq!(jmena("skola"), vec!["skola", "Škola.docx"]);
        assert_eq!(jmena("účtenka"), vec!["ÚČTENKA.pdf"]);
        assert_eq!(jmena("CONFIG"), vec!["Config.SYS"]);
        // Úplná shoda se pozná i přes diakritiku: „skola" je první.
        assert_eq!(idx.search("Škola", 10)[0].name, "skola");
    }

    #[test]
    fn slozeni_odpovida_ui() {
        assert_eq!(slozit("Žluťoučký KŮŇ"), "zlutoucky kun");
        // Rozložený tvar (NFD): písmeno + samostatný háček.
        assert_eq!(slozit("S\u{30c}kola"), "skola");
        // Bez kanonického rozkladu zůstává jako v NFD.
        assert_eq!(slozit("Łódź"), "łodz");
    }

    /// Soubor hlouběji než DIR_DEPTH se musí přičíst mělkým předkům —
    /// dřív se nepřičetl nikomu.
    #[test]
    fn hluboky_soubor_se_pricte_melkym_predkum() {
        let rodic = std::path::Path::new(r"C:\a\b\c\d\e\f");
        let p: Vec<_> = predkove_do_mapy(rodic, 6)
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert_eq!(p, vec![r"C:\a\b\c\d", r"C:\a\b\c", r"C:\a\b", r"C:\a"]);
        let p: Vec<_> = predkove_do_mapy(std::path::Path::new(r"C:\a"), 1)
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert_eq!(p, vec![r"C:\a"]);
        assert_eq!(predkove_do_mapy(std::path::Path::new(r"C:\"), 0).count(), 0);
    }

    #[test]
    fn soucet_slozky_zahrne_hluboke_soubory() {
        let koren = std::env::temp_dir().join(format!("wsidx-hloubka-{}", std::process::id()));
        let hluboko = koren.join(r"a\b\c\d\e\f");
        std::fs::create_dir_all(&hluboko).unwrap();
        std::fs::write(koren.join(r"a\small.bin"), vec![0u8; 2_000_000]).unwrap();
        std::fs::write(hluboko.join("big.bin"), vec![0u8; 5_000_000]).unwrap();
        let b = largest_items(&koren.to_string_lossy(), 10, 10_000);
        let a = koren.join("a").to_string_lossy().into_owned();
        let _ = std::fs::remove_dir_all(&koren);
        let velikost = b.dirs.iter().find(|(p, _)| *p == a).map(|(_, s)| *s);
        assert_eq!(velikost, Some(7_000_000), "{:?}", b.dirs);
        assert!(!b.truncated);
    }

    #[test]
    fn predek_jen_na_hranici_slozky() {
        assert!(je_predek(r"C:\Users\IVA", r"C:\Users\IVA\AppData"));
        assert!(!je_predek(r"C:\Users\IVA", r"C:\Users\IVAN"));
        assert!(!je_predek(r"C:\Program Files", r"C:\Program Files (x86)\x"));
        assert!(!je_predek(r"C:\a", r"C:\a"));
    }

    #[test]
    fn koren_duplicit_jen_mistni() {
        assert!(mistni_tvar(r"C:\"));
        assert!(mistni_tvar(r"d:\Fotky\2024"));
        assert!(!mistni_tvar(r"\\server\share\x"));
        assert!(!mistni_tvar(r"\\?\C:\x"));
        assert!(!mistni_tvar(r"\\.\PhysicalDrive0"));
        assert!(!mistni_tvar(r"C:relativni"));
        assert!(!mistni_tvar(r"C:\a\..\..\b"));
        assert!(!mistni_tvar(r"C:\a\.. \b"));
        assert!(!mistni_tvar(r"C:/a"));
        assert!(!mistni_tvar(r"C:\a:proud"));
        assert!(!mistni_tvar(""));
        // Existující místní adresář bez přesměrování projde celou
        // kontrolou, UNC ani neexistující cesta ne.
        let tmp = std::env::temp_dir();
        let tmp = tmp.to_string_lossy();
        if mistni_tvar(&tmp) {
            assert!(mistni_koren(&tmp));
        }
        assert!(find_duplicates(r"\\127.0.0.1\c$", 1, 10).is_empty());
    }
}
