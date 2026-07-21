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
        let mut nodes = HashMap::new();
        win_sys::usn::enum_volume(letter, |e| {
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
