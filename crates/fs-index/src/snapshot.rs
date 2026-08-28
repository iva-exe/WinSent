//! Uložený index svazku — aby se nemusel stavět pořád znovu.
//!
//! Stavba z MFT trvá sekundy: naměřeno 8,1 s pro systémový svazek
//! s 1,84 milionu záznamů (15,2 s se studenou cache) a 1,7 s pro
//! datový se 444 tisíci. To by samo o sobě nevadilo, kdyby se dělala
//! jen při startu — jenže index se po nečinnosti uvolňuje z paměti
//! a další hledání ho pak dostavuje PŘÍMO v uživatelově dotazu.
//! Kdo hledal poprvé po pěti minutách, čekal těch osm sekund
//! s kurzorem v liště.
//!
//! Uložený index je proti tomu čtení jednoho souboru. Není to
//! databáze a nemá cenu ho zachraňovat: když se nedá načíst, z jakého
//! důvodu je jedno, prostě se svazek postaví znovu.
//!
//! FORMÁT (little-endian, jedna hlavička, dvě pole, součet na konci):
//!
//! ```text
//!   hlavička 40 B: magie(8) verze(4) písmeno(4) kořen(8) počet(8) délka_arény(8)
//!   záznamy:       počet × 32 B { file_ref(8) rodič(8) offset(4) délka(4) atributy(4) výplň(4) }
//!   aréna:         délka_arény bajtů — jména za sebou v UTF-8
//!   součet:        8 B FNV-1a přes všechno předchozí
//! ```
//!
//! Výplň v záznamu je TAM SCHVÁLNĚ a nuluje se: bez ní by struktura
//! měla čtyři bajty neinicializované paměti, které by se vynesly do
//! souboru a udělaly kontrolní součet nereprodukovatelným.
//!
//! Čtení je záměrně celé v bezpečném Rustu s ručními mezemi. Adresář
//! `%ProgramData%\syswatch` je zapisovatelný i pro běžného uživatele,
//! takže soubor, který služba (běžící pod SYSTEM) čte, nemusí být od
//! nás. Nejhorší, co se poškozeným nebo podvrženým souborem dá
//! způsobit, jsou nesmyslné výsledky hledání — ne pád a ne cizí kód.

use std::collections::HashMap;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;

use crate::{Node, VolumeIndex};

/// „WSIDX" + verze formátu. Změna formátu = změna magie; starý soubor
/// se pak neuzná a svazek se postaví znovu, což je přesně to, co má
/// nekompatibilní změna udělat.
const MAGIE: &[u8; 8] = b"WSIDX\x00\x00\x02";
const VERZE: u32 = 2;
/// magie(8) verze(4) písmeno(4) kořen(8) počet(8) délka_arény(8) uloženo(8)
const HLAVICKA: usize = 48;
const ZAZNAM: usize = 32;
const SOUCET: usize = 8;

/// Nejvyšší velikost souboru, kterou jsme ochotni vůbec přečíst.
///
/// Adresář je zapisovatelný i pro běžného uživatele, takže velikost
/// souboru je nedůvěryhodný údaj — a přitom z něj plyne, kolik se
/// alokuje. Bez stropu stačí podstrčit dvougigabajtový soubor a služba
/// se pokusí sáhnout po několika gigabajtech; selhání alokace v Rustu
/// není chyba k odchycení, ale ABORT procesu (profil release má
/// panic = "abort"). Naměřeno: 512 MB vstupu → 1,4 GB v tabulce.
/// Systémový svazek s 1,85 milionu záznamů má 116 MB, takže 320 MB je
/// s velkou rezervou a zároveň řádově pod tím, čím by šlo ublížit.
const STROP_SOUBORU: u64 = 320 * 1024 * 1024;

/// Jak starý smí být uložený index, aby se z něj dalo rovnou žít.
///
/// Starší se pořád použije — je to nejrychlejší způsob, jak uživateli
/// hned odpovědět —, ale volající si k tomu má objednat čerstvou
/// stavbu na pozadí. Bez toho by hledání zamrzlo na stavu disku
/// k poslednímu uložení a nové soubory by nikdy nenašlo.
pub const CERSTVOST_S: u64 = 60;

/// Kam se ukládají indexy svazků.
///
/// Vedle databáze, ale ve vlastním podadresáři: je to zahoditelná
/// mezipaměť, ne data. Kdo ji smaže, přijde jen o rychlost.
pub fn adresar() -> Option<PathBuf> {
    let base = std::env::var_os("ProgramData")?;
    Some(PathBuf::from(base).join("syswatch").join("index"))
}

/// Soubor pro jeden svazek.
pub fn soubor(letter: char) -> Option<PathBuf> {
    // Písmeno svazku do jména cesty nepouštíme jinak než jako jediný
    // znak z povolené množiny — jméno souboru se skládá z hodnoty,
    // která k nám může přijít i z pipe.
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    Some(adresar()?.join(format!("{}.idx", letter.to_ascii_uppercase())))
}

/// Sekundy od epochy. Razítko v hlavičce, ne čas souboru: kopírování
/// nebo obnova ze zálohy by čas souboru změnily, obsah ne.
fn ted_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fnv(data: &[u8], mut h: u64) -> u64 {
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const FNV_ZAKLAD: u64 = 0xcbf2_9ce4_8422_2325;

/// Zapíše index do proudu. Oddělené od souboru kvůli testům —
/// jinak by se v nich musel zapisovat druhý, samostatný kus kódu
/// a ten by se s tímhle dřív nebo později rozešel.
///
/// Jde se přes záznamy DVAKRÁT, aby nebylo potřeba žádné mezipole:
/// hlavička potřebuje délku arény dřív, než se aréna zapíše, a
/// posbírat si ji do paměti by u systémového svazku znamenalo přes
/// sto megabajtů navíc — zrovna ve chvíli, kdy služba drží index.
/// Mapa se mezi průchody nemění, takže pořadí sedí.
pub fn zapis(idx: &VolumeIndex, w: &mut impl Write) -> std::io::Result<()> {
    let mut h = FNV_ZAKLAD;
    let poslat = |w: &mut dyn Write, data: &[u8], h: &mut u64| -> std::io::Result<()> {
        *h = fnv(data, *h);
        w.write_all(data)
    };

    let arena_len: usize = idx.nodes.values().map(|n| n.name.len()).sum();
    poslat(w, MAGIE, &mut h)?;
    poslat(w, &VERZE.to_le_bytes(), &mut h)?;
    poslat(w, &(idx.letter as u32).to_le_bytes(), &mut h)?;
    poslat(w, &idx.root.to_le_bytes(), &mut h)?;
    poslat(w, &(idx.nodes.len() as u64).to_le_bytes(), &mut h)?;
    poslat(w, &(arena_len as u64).to_le_bytes(), &mut h)?;
    poslat(w, &ted_s().to_le_bytes(), &mut h)?;

    let mut off: u32 = 0;
    for (file_ref, n) in &idx.nodes {
        let dl = n.name.len() as u32;
        let mut zaznam = [0u8; ZAZNAM];
        zaznam[0..8].copy_from_slice(&file_ref.to_le_bytes());
        zaznam[8..16].copy_from_slice(&n.parent.to_le_bytes());
        zaznam[16..20].copy_from_slice(&off.to_le_bytes());
        zaznam[20..24].copy_from_slice(&dl.to_le_bytes());
        zaznam[24..28].copy_from_slice(&n.attrs.to_le_bytes());
        // Poslední čtyři bajty zůstávají nulové. Výplň je tam
        // schválně: bez ní by měl záznam v paměti neinicializované
        // bajty, které by se vynesly do souboru a udělaly kontrolní
        // součet nereprodukovatelným.
        poslat(w, &zaznam, &mut h)?;
        off += dl;
    }
    for n in idx.nodes.values() {
        poslat(w, n.name.as_bytes(), &mut h)?;
    }
    w.write_all(&h.to_le_bytes())
}

/// Uloží index svazku na disk. Chyba se hlásí, ale volající ji smí
/// ignorovat — bez uloženého indexu aplikace funguje, jen pomaleji.
pub fn uloz(idx: &VolumeIndex) -> std::io::Result<PathBuf> {
    let cil = soubor(idx.letter)
        .ok_or_else(|| std::io::Error::other("neznámé umístění pro uložený index"))?;
    if let Some(d) = cil.parent() {
        std::fs::create_dir_all(d)?;
    }
    // Píše se vedle a přejmenovává až hotové. Kdyby služba spadla
    // uprostřed zápisu, zůstal by jinak useknutý soubor, který by se
    // příště sice zahodil, ale mezitím by vypadal jako platný.
    // Pevné jméno by šlo obsadit: adresář je zapisovatelný pro
    // běžného uživatele, takže by si tam mohl předem založit soubor
    // (nebo adresář) toho jména a ukládání by od té chvíle mlčky
    // selhávalo napořád. Náhodná přípona a `create_new` to vylučují —
    // a zároveň se tím nesrazí dva souběžné zápisy.
    let docasny = cil.with_extension(format!("idx.{}.tmp", ted_s() ^ (idx.nodes.len() as u64)));
    {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&docasny)?;
        let mut w = BufWriter::with_capacity(1 << 20, f);
        zapis(idx, &mut w)?;
        let f = w.into_inner().map_err(|e| e.into_error())?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&docasny, &cil) {
        let _ = std::fs::remove_file(&docasny);
        return Err(e);
    }
    Ok(cil)
}

/// Načte uložený index svazku. `None` = není, nesedí, nebo je vadný;
/// v každém z těch případů je odpověď stejná — postavit ho znovu.
pub fn nacti(letter: char) -> Option<(VolumeIndex, u64)> {
    let cil = soubor(letter)?;
    let f = std::fs::File::open(&cil).ok()?;
    // Strop se dává na ČTENÍ, ne až na velikost z metadat: soubor může
    // mezi zjištěním velikosti a čtením vyrůst a řídkému souboru
    // metadata stejně nic neřeknou.
    let mut data = Vec::new();
    std::io::Read::take(f, STROP_SOUBORU + 1)
        .read_to_end(&mut data)
        .ok()?;
    if data.len() as u64 > STROP_SOUBORU {
        return None;
    }
    rozbal(letter, &data)
}

/// Jak dlouho už uložený index leží — v sekundách. `None` = není.
pub fn stari_s(letter: char) -> Option<u64> {
    let data = precti_hlavicku(letter)?;
    let uloz_v = u64::from_le_bytes(data.get(40..48)?.try_into().ok()?);
    Some(ted_s().saturating_sub(uloz_v))
}

fn precti_hlavicku(letter: char) -> Option<Vec<u8>> {
    let f = std::fs::File::open(soubor(letter)?).ok()?;
    let mut hlava = Vec::new();
    std::io::Read::take(f, HLAVICKA as u64)
        .read_to_end(&mut hlava)
        .ok()?;
    (hlava.len() == HLAVICKA && &hlava[..8] == MAGIE).then_some(hlava)
}

/// Rozbalí obsah souboru na index. Oddělené od čtení kvůli testům —
/// a proto, že tohle je jediné místo, které sahá na cizí data.
fn rozbal(letter: char, data: &[u8]) -> Option<(VolumeIndex, u64)> {
    if data.len() < HLAVICKA + SOUCET || &data[..8] != MAGIE {
        return None;
    }
    // Čtení hlavičky přes `get` — nikde se nekrájí bez kontroly.
    let u32_na = |o: usize| -> Option<u32> {
        Some(u32::from_le_bytes(data.get(o..o + 4)?.try_into().ok()?))
    };
    let u64_na = |o: usize| -> Option<u64> {
        Some(u64::from_le_bytes(data.get(o..o + 8)?.try_into().ok()?))
    };
    if u32_na(8)? != VERZE {
        return None;
    }
    // Písmeno v souboru musí sedět s tím, o který svazek se žádá —
    // jinak by přejmenovaný soubor podstrčil cizí obsah.
    if char::from_u32(u32_na(12)?)?.to_ascii_uppercase() != letter.to_ascii_uppercase() {
        return None;
    }
    let root = u64_na(16)?;
    let pocet = u64_na(24)? as usize;
    let arena_len = u64_na(32)? as usize;
    let ulozeno = u64_na(40)?;
    // Prázdný index je platný soubor, ale k ničemu: svazek by se pak
    // tvářil jako připravený a hledání na něm by vždycky vracelo nic.
    // Lepší je postavit ho znovu.
    if pocet == 0 {
        return None;
    }

    // Délka musí sedět na bajt. Tahle jediná kontrola zaručuje, že
    // všechna krájení níž jsou v mezích.
    let ocekavana = HLAVICKA
        .checked_add(pocet.checked_mul(ZAZNAM)?)?
        .checked_add(arena_len)?
        .checked_add(SOUCET)?;
    if data.len() != ocekavana {
        return None;
    }
    let ulozeny = u64::from_le_bytes(data[data.len() - SOUCET..].try_into().ok()?);
    if fnv(&data[..data.len() - SOUCET], FNV_ZAKLAD) != ulozeny {
        return None;
    }

    let zac_zaznamu = HLAVICKA;
    let zac_areny = zac_zaznamu + pocet * ZAZNAM;
    let arena = &data[zac_areny..zac_areny + arena_len];

    let mut nodes = HashMap::with_capacity(pocet);
    for i in 0..pocet {
        let o = zac_zaznamu + i * ZAZNAM;
        let file_ref = u64::from_le_bytes(data[o..o + 8].try_into().ok()?);
        let parent = u64::from_le_bytes(data[o + 8..o + 16].try_into().ok()?);
        let off = u32::from_le_bytes(data[o + 16..o + 20].try_into().ok()?) as usize;
        let dl = u32::from_le_bytes(data[o + 20..o + 24].try_into().ok()?) as usize;
        let attrs = u32::from_le_bytes(data[o + 24..o + 28].try_into().ok()?);
        let konec = off.checked_add(dl)?;
        if konec > arena.len() {
            return None;
        }
        // Neplatné UTF-8 je důvod zahodit celý soubor, ne jeden řádek:
        // znamená to, že obsah nepochází od nás.
        let name = std::str::from_utf8(&arena[off..konec]).ok()?;
        nodes.insert(
            file_ref,
            Node {
                name: name.into(),
                parent,
                attrs,
            },
        );
    }
    Some((
        VolumeIndex {
            letter,
            nodes,
            root,
        },
        ulozeno,
    ))
}

/// Smaže po sobě zapomenuté rozpracované soubory.
///
/// Zůstávají po pádu uprostřed zápisu. Samy o sobě nevadí, ale nikdo
/// jiný je neuklidí — `zahod` sahá jen na hotový index.
pub fn uklid_rozpracovane() {
    let Some(dir) = adresar() else { return };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().ends_with(".tmp") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Smaže uložený index svazku (po nepovedeném načtení nemá co dělat).
pub fn zahod(letter: char) {
    if let Some(p) = soubor(letter) {
        let _ = std::fs::remove_file(p);
    }
}

/// Cesta k uloženému indexu jen pro výpis do UI/protokolu.
pub fn velikost(letter: char) -> Option<u64> {
    std::fs::metadata(soubor(letter)?).ok().map(|m| m.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vzorek() -> VolumeIndex {
        let mut nodes = HashMap::new();
        nodes.insert(
            5u64,
            Node {
                name: "".into(),
                parent: 5,
                attrs: crate::ATTR_DIR,
            },
        );
        nodes.insert(
            10u64,
            Node {
                name: "Dokumenty".into(),
                parent: 5,
                attrs: crate::ATTR_DIR,
            },
        );
        nodes.insert(
            11u64,
            Node {
                name: "účtenka.pdf".into(),
                parent: 10,
                attrs: 0,
            },
        );
        VolumeIndex {
            letter: 'C',
            nodes,
            root: 5,
        }
    }

    /// Uložení a načtení musí dát tentýž index — včetně diakritiky
    /// v názvech, kvůli které je aréna v UTF-8, ne v ASCII.
    #[test]
    fn kolecko_zachova_obsah() {
        let idx = vzorek();
        let mut buf = Vec::new();
        zapis(&idx, &mut buf).expect("zápis do paměti nemůže selhat");
        let (zpet, _kdy) = rozbal('C', &buf).expect("platný soubor se musí načíst");
        assert_eq!(zpet.len(), idx.len());
        assert_eq!(zpet.root, idx.root);
        let hits = zpet.search("účtenka", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, r"C:\Dokumenty\účtenka.pdf");
    }

    #[test]
    fn poskozeny_soubor_se_neuzna() {
        let idx = vzorek();
        let mut buf = Vec::new();
        zapis(&idx, &mut buf).expect("zápis do paměti nemůže selhat");

        // Jeden přehozený bajt uvnitř dat — kontrolní součet to chytí.
        let mut zmena = buf.clone();
        let i = HLAVICKA + 3;
        zmena[i] ^= 0xff;
        assert!(rozbal('C', &zmena).is_none(), "změněná data musí propadnout");

        // Useknutý soubor.
        assert!(rozbal('C', &buf[..buf.len() - 5]).is_none());
        // Cizí magie.
        let mut cizi = buf.clone();
        cizi[0] = b'X';
        assert!(rozbal('C', &cizi).is_none());
        // Soubor jiného svazku (přejmenovaný).
        assert!(rozbal('D', &buf).is_none());
        // Prázdno.
        assert!(rozbal('C', &[]).is_none());
    }

    /// Nesmysly v hlavičce nesmí vést k sáhnutí mimo data. Kontrola
    /// délky je jediná pojistka, na které to celé stojí.
    /// Prázdný index je platný soubor, ale znamenal by svazek, který
    /// se tváří připravený a nikdy nic nenajde.
    #[test]
    fn prazdny_index_se_neuzna() {
        let idx = VolumeIndex {
            letter: 'C',
            nodes: HashMap::new(),
            root: 5,
        };
        let mut buf = Vec::new();
        zapis(&idx, &mut buf).expect("zápis do paměti nemůže selhat");
        assert!(rozbal('C', &buf).is_none());
    }

    /// Razítko v hlavičce je to, podle čeho se pozná zastaralý index.
    /// Bez něj by hledání zamrzlo na stavu disku k poslednímu uložení.
    #[test]
    fn hlavicka_nese_cas_ulozeni() {
        let idx = vzorek();
        let mut buf = Vec::new();
        zapis(&idx, &mut buf).expect("zápis do paměti nemůže selhat");
        let (_, kdy) = rozbal('C', &buf).expect("platný soubor");
        let ted = ted_s();
        assert!(kdy <= ted && ted - kdy < 5, "kdy={kdy} ted={ted}");
    }

    #[test]
    fn vymysleny_pocet_zaznamu_neprojde() {
        let idx = vzorek();
        let mut buf = Vec::new();
        zapis(&idx, &mut buf).expect("zápis do paměti nemůže selhat");
        // Počet záznamů na absurdní hodnotu.
        buf[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(rozbal('C', &buf).is_none());
    }
}
