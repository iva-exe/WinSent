//! fs-index — NTFS MFT/USN prohlížeč (SPEC kap. 11.2, čtecí část).
//!
//! In-memory index celého svazku postavený z MFT za sekundy — žádná
//! vlastní databáze souborů, čte se přímo struktura NTFS. Hledání je
//! lineární průchod s ASCII case-insensitive podřetězcem — i milión
//! záznamů se projde v desítkách ms. Mazání sem NEPATŘÍ (v8, přes
//! validační vrstvu).

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

    /// Hledání podřetězce v názvech (ASCII case-insensitive). Vrací max
    /// `limit` nálezů; kratší cesty (blíž kořeni) první.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let q_lc = q.to_ascii_lowercase();
        let mut out = Vec::new();
        for (file_ref, n) in &self.nodes {
            if contains_ignore_ascii_case(&n.name, &q_lc) {
                out.push(Hit {
                    path: self.path_of(*file_ref),
                    name: n.name.to_string(),
                    attrs: n.attrs,
                });
                if out.len() >= limit {
                    break;
                }
            }
        }
        out.sort_by_key(|h| h.path.len());
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
pub fn find_duplicates(root: &str, min_size: u64, max_files: usize) -> Vec<DupGroup> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

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
    let mut stack: Vec<std::path::PathBuf> = std::fs::read_dir(&users)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
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
    if let Ok(rd) = std::fs::read_dir(&users) {
        for e in rd.flatten() {
            let p = e.path().join("AppData\\Local\\Temp");
            if p.is_dir() {
                junk_paths.push(p.to_string_lossy().into_owned());
            }
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

/// Největší soubory a složky svazku (v4F): jeden průchod stromem,
/// velikosti z directory enumerace (na Windows je `metadata()` na
/// DirEntry zdarma — data pocházejí z FindNextFile). Složkám se
/// přičítá velikost potomků jen do `DIR_DEPTH` úrovní, ať to má
/// vypovídací hodnotu a nežere paměť.
pub struct BigItems {
    /// (cesta, velikost) — největší jednotlivé soubory.
    pub files: Vec<(String, u64)>,
    /// (cesta, velikost) — největší složky (součet obsahu).
    pub dirs: Vec<(String, u64)>,
}

/// Do jaké hloubky se agregují velikosti složek.
const DIR_DEPTH: usize = 4;

/// Projde svazek a vrátí top N souborů a složek dle velikosti.
pub fn largest_items(root: &str, top_n: usize, max_entries: usize) -> BigItems {
    let mut files: Vec<(String, u64)> = Vec::new();
    let mut dirs: HashMap<String, u64> = HashMap::new();
    // Práh pro udržení souboru v kandidátech (roste, ať Vec nepřeteče).
    let mut min_file: u64 = 1_000_000;
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(root.into(), 0)];
    let mut seen = 0usize;

    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen > max_entries {
                stack.clear();
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
                stack.push((path, depth + 1));
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
                // Přičíst do všech předků do DIR_DEPTH úrovní.
                let mut cur = path.parent().map(|p| p.to_path_buf());
                let mut level = depth;
                while let Some(p) = cur {
                    if level == 0 || level > DIR_DEPTH {
                        break;
                    }
                    *dirs.entry(p.to_string_lossy().into_owned()).or_insert(0) += size;
                    cur = p.parent().map(|x| x.to_path_buf());
                    level -= 1;
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
            (p.len() < kp.len() && kp.starts_with(&p) && *ks * 10 > s * 9)
                || (kp.len() < p.len() && p.starts_with(kp.as_str()) && s * 10 > *ks * 9)
        });
        if !redundant {
            kept.push((p, s));
        }
        if kept.len() >= top_n {
            break;
        }
    }
    BigItems { files, dirs: kept }
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
fn contains_ignore_ascii_case(haystack: &str, needle_lc: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle_lc.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    'outer: for start in 0..=(h.len() - n.len()) {
        for (i, &nc) in n.iter().enumerate() {
            if h[start + i].to_ascii_lowercase() != nc {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_matches_case_insensitive() {
        assert!(contains_ignore_ascii_case("Config.SYS", "config"));
        assert!(contains_ignore_ascii_case("abcDEF", "cde"));
        assert!(!contains_ignore_ascii_case("abc", "abcd"));
    }
}
