//! Spuštění Winsentu při přihlášení uživatele.
//!
//! Zapisuje si to UI samo — ne instalátor a ne služba. Oboje má totiž
//! špatnou větev registru: instalátor běží elevovaně a jeho
//! HKEY_CURRENT_USER nemusí patřit tomu, kdo bude aplikaci používat,
//! a služba je na tom ještě hůř, protože běží pod SYSTEM a její HKCU
//! je hive účtu SYSTEM. Na tuhle past v tomhle projektu dojelo už
//! čtení inventáře: aplikace nainstalované „jen pro mě" se nenašly.
//! UI běží v relaci uživatele, takže jeho HKCU je ten správný.
//!
//! Není to zásah do systému v tom smyslu, jaký hlídá validační vrstva:
//! aplikace si spravuje VLASTNÍ položku ve vlastní větvi uživatele.
//! Cizí startovací položky se pořád mění jen přes službu a audit.
//!
//! Ve výchozím stavu je spouštění zapnuté — bez něj po restartu
//! počítače nefunguje ani vyhledávací lišta na klávesovou zkratku,
//! protože zkratku registruje až běžící proces UI.

use std::path::PathBuf;

/// Kde má Windows seznam programů spouštěných po přihlášení.
const KLIC_RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Kde si Windows (a Správce úloh) pamatují, jestli je položka povolená.
const KLIC_SCHVALENI: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Jméno hodnoty. Musí sedět s tím, co ukazuje Správce úloh.
const JMENO: &str = "Winsent";
/// Přepínač, se kterým se UI schová rovnou do oznamovací oblasti.
pub const PREPINAC_TRAY: &str = "--tray";

/// Soubor, kterým si pamatujeme, že jsme spouštění po přihlášení už
/// jednou nastavovali.
///
/// Bez něj by se volba nedala vypnout: při každém startu bychom ji
/// zase zapnuli. Leží vedle ostatních předvoleb hostitele.
fn priznak() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Winsent").join("autostart-init")
}

/// Co se má po přihlášení spustit.
fn prikaz() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" {PREPINAC_TRAY}", exe.display()))
}

/// Spouští se Winsent po přihlášení?
///
/// Nestačí se podívat do klíče Run: uživatel může položku vypnout ve
/// Správci úloh, který ji nesmaže, jen si její zákaz poznamená vedle.
/// Kdybychom to nečetli, přepínač v Nastavení by hlásil „zapnuto" nad
/// něčím, co se nespouští.
pub fn zapnuto() -> bool {
    if precti_run().is_none() {
        return false;
    }
    !zakazano_spravcem_uloh()
}

/// Co je teď zapsané v klíči Run.
fn precti_run() -> Option<String> {
    win_sys::registry::read_string(win_sys::registry::HKEY_CURRENT_USER, KLIC_RUN, JMENO)
}

/// Má položku zakázanou Správce úloh?
fn zakazano_spravcem_uloh() -> bool {
    win_sys::registry::read_binary(win_sys::registry::HKEY_CURRENT_USER, KLIC_SCHVALENI, JMENO)
        .is_some_and(|v| zaznam_znamena_zakaz(&v))
}

/// Znamená záznam Správce úloh zákaz?
///
/// Rozhoduje nejnižší bit prvního bajtu: sudá hodnota je povoleno
/// (běžně 02 nebo 06), lichá zakázáno (03). Chybějící nebo prázdný
/// záznam znamená, že se položkou nikdo neručně nezabýval — tedy
/// povoleno.
fn zaznam_znamena_zakaz(v: &[u8]) -> bool {
    v.first().is_some_and(|b| b & 1 == 1)
}

/// Zapne nebo vypne spouštění po přihlášení.
pub fn nastav(zapnout: bool) -> Result<(), String> {
    if zapnout {
        let cil = prikaz().ok_or("nepodařilo se zjistit cestu k aplikaci")?;
        zapis_run(&cil)?;
        // Záznam Správce úloh se maže, ne přepisuje: bez něj platí
        // „povoleno", což je přesně to, co uživatel právě zvolil.
        smaz_hodnotu(KLIC_SCHVALENI, JMENO);
    } else {
        smaz_hodnotu(KLIC_RUN, JMENO);
        // Osiřelý zákaz u neexistující položky by po příštím zapnutí
        // tiše vyhrál nad volbou uživatele.
        smaz_hodnotu(KLIC_SCHVALENI, JMENO);
    }
    Ok(())
}

/// Při úplně prvním spuštění zapne spouštění po přihlášení.
///
/// Volá se při každém startu, ale zabere jen jednou — pak už rozhoduje
/// uživatel. Chyba se nikam nehlásí: kvůli tomu, že se nepovedl zápis
/// jedné hodnoty do registru, nemá smysl aplikaci nespustit.
pub fn zajisti_vychozi() {
    let p = priznak();
    if !p.exists() {
        let _ = nastav(true);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&p, b"1");
        return;
    }
    // Aplikace se mezitím mohla přeinstalovat jinam. Když je spouštění
    // zapnuté, ale míří na jiný soubor, srovná se na ten současný —
    // jinak by po přesunu startovala stará nebo už neexistující kopie.
    // Zakázanou položky se to netýká: `zapnuto()` je u ní false, takže
    // se volba uživatele nepřepíše.
    if zapnuto() {
        if let (Some(chteny), Some(soucasny)) = (prikaz(), precti_run()) {
            if !chteny.eq_ignore_ascii_case(&soucasny) {
                let _ = zapis_run(&chteny);
            }
        }
    }
}

/// Startuje tenhle proces rovnou do oznamovací oblasti?
pub fn tichy_start() -> bool {
    std::env::args().any(|a| a == PREPINAC_TRAY)
}

// ── Zápis do registru ──────────────────────────────────────────────
//
// Vlastní, ne přes `win_sys::registry`: tamní zapisovací funkce je
// schválně jediná a patří výhradně exekutorům za validační vrstvou
// (SPEC kap. 2, oddělené cesty). Tohle je vlastní předvolba aplikace
// ve větvi jejího uživatele, ne zásah do cizího nastavení, takže tou
// cestou nepatří — a ani by jí projít nemohla, viz komentář nahoře.

fn zapis_run(hodnota: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    let wsub = HSTRING::from(KLIC_RUN);
    let wval = HSTRING::from(JMENO);
    // REG_SZ chce data v UTF-16 VČETNĚ zakončující nuly, a v bajtech.
    let mut sirokz: Vec<u16> = hodnota.encode_utf16().collect();
    sirokz.push(0);
    let bajty: &[u8] = unsafe {
        std::slice::from_raw_parts(sirokz.as_ptr() as *const u8, std::mem::size_of_val(&sirokz[..]))
    };
    let mut hkey = HKEY::default();
    // SAFETY: klíč se vždy zavírá; data žijí přes celé volání.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &wsub,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        )
        .ok()
        .map_err(|e| format!("klíč Run nejde otevřít: {e}"))?;
        let r = RegSetValueExW(hkey, &wval, None, REG_SZ, Some(bajty));
        let _ = RegCloseKey(hkey);
        r.ok()
            .map_err(|e| format!("zápis do klíče Run selhal: {e}"))
    }
}

/// Smaže hodnotu, pokud existuje. Chybějící hodnota není chyba.
fn smaz_hodnotu(subkey: &str, value: &str) {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    };
    let wsub = HSTRING::from(subkey);
    let wval = HSTRING::from(value);
    let mut hkey = HKEY::default();
    // SAFETY: klíč se zavírá, jen když se ho podařilo otevřít.
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, &wsub, None, KEY_SET_VALUE, &mut hkey).is_err() {
            return;
        }
        let _ = RegDeleteValueW(hkey, &wval);
        let _ = RegCloseKey(hkey);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zakaz_pozna_spravce_uloh() {
        // 02 = povoleno rukou, 06 = povoleno systémem, 03 = zakázáno.
        assert!(!zaznam_znamena_zakaz(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(!zaznam_znamena_zakaz(&[0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(zaznam_znamena_zakaz(&[0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        // Prázdný nebo chybějící záznam nikomu nic nezakazuje.
        assert!(!zaznam_znamena_zakaz(&[]));
    }

    #[test]
    fn prikaz_ma_uvozovky_a_prepinac() {
        // Cesta s mezerou („C:\\Program Files\\…") musí být v uvozovkách,
        // jinak Windows spustí „C:\\Program" a zbytek vezme jako argument.
        let p = prikaz().expect("cesta k vlastní binárce");
        assert!(p.starts_with('"'), "{p}");
        assert!(p.ends_with(PREPINAC_TRAY), "{p}");
    }
}
