//! Obecné registry helpery (čtení hodnot, enumerace podklíčů).
//! Jen čtení — zápisy do registru patří výhradně za validační vrstvu.

use windows::core::HSTRING;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, KEY_ENUMERATE_SUB_KEYS,
    KEY_READ, RRF_RT_ANY, RRF_RT_REG_SZ,
};

pub use windows::Win32::System::Registry::HKEY as RegKey;
pub use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS};

/// Přečte REG_SZ hodnotu (RegGetValueW zvládá i REG_EXPAND_SZ expanzi).
pub fn read_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    let subkey = HSTRING::from(subkey);
    let value = HSTRING::from(value);
    let mut len = 0u32;
    // SAFETY: dvoufázové čtení dle kontraktu RegGetValueW.
    unsafe {
        if RegGetValueW(
            root,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        if RegGetValueW(
            root,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// Přečte číselnou hodnotu — REG_QWORD, REG_DWORD i 8bajtový REG_BINARY
/// (`HardwareInformation.qwMemorySize` je podle vendora cokoliv z toho).
pub fn read_u64(root: HKEY, subkey: &str, value: &str) -> Option<u64> {
    let subkey = HSTRING::from(subkey);
    let value = HSTRING::from(value);
    let mut buf = [0u8; 8];
    let mut len = buf.len() as u32;
    // SAFETY: buffer má pevných 8 B, len říká API skutečnou velikost.
    unsafe {
        if RegGetValueW(
            root,
            &subkey,
            &value,
            RRF_RT_ANY,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
    }
    match len {
        4 => Some(u32::from_le_bytes(buf[..4].try_into().ok()?) as u64),
        8 => Some(u64::from_le_bytes(buf)),
        _ => None,
    }
}

/// Vyjmenuje hodnoty klíče jako (název, data jako string). Nečíselné
/// typy se přeskočí — startup Run klíče drží REG_SZ/EXPAND_SZ.
pub fn enum_values(root: HKEY, subkey: &str) -> Vec<(String, String)> {
    use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS};
    use windows::Win32::System::Registry::{RegEnumValueW, RegQueryInfoKeyW};
    // Strop jména hodnoty dle registru (16 383 znaků + nula).
    const MAX_NAME: usize = 16_384;
    let mut out = Vec::new();
    let wsub = HSTRING::from(subkey);
    let mut hkey = HKEY::default();
    // SAFETY: klíč se vždy zavírá; buffery jsou Vec a délky se před
    // každým voláním nastavují na jejich skutečnou kapacitu.
    unsafe {
        if RegOpenKeyExW(root, &wsub, None, KEY_READ, &mut hkey).is_err() {
            return out;
        }
        // Buffery podle největší hodnoty v klíči. Dřív byly pevné
        // (512 znaků / 2 KiB) a první větší hodnota — i REG_BINARY, který
        // se stejně přeskakuje — vrátila ERROR_MORE_DATA, což se bralo
        // jako konec výčtu: zbytek Run klíče zmizel (a šlo to zneužít
        // ke skrytí položky bez admina).
        let mut values = 0u32;
        let mut max_name = 0u32;
        let mut max_data = 0u32;
        let (name_cap, data_cap) = if RegQueryInfoKeyW(
            hkey,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&mut values),
            Some(&mut max_name),
            Some(&mut max_data),
            None,
            None,
        )
        .is_ok()
        {
            (max_name as usize + 1, max_data as usize + 2)
        } else {
            values = u32::MAX;
            (MAX_NAME, 64 * 1024)
        };
        let mut name = vec![0u16; name_cap.clamp(1, MAX_NAME)];
        // Strop dat jedné hodnoty. Run klíč smí zapsat i běžný uživatel:
        // bez stropu by hodnota o 16 MB donutila službu (SYSTEM) alokovat
        // stejně velký buffer při každém skenu a celá odpověď seznamu po
        // spuštění by přerostla limit rámce IPC — UI by neukázalo nic,
        // tedy přesně to schování položky, kvůli kterému se buffery
        // přestaly zkracovat. Delší hodnota se ukáže jako položka bez
        // příkazu.
        const MAX_DATA: usize = 64 * 1024;
        let mut data = vec![0u8; data_cap.clamp(4, MAX_DATA)];
        let mut index = 0u32;
        let mut retries = 0u32;
        // Horní mez chrání před zacyklením, kdyby klíč pod rukama rostl.
        while index < values.min(100_000) {
            let mut name_len = name.len() as u32;
            let mut data_len = data.len() as u32;
            let mut kind = 0u32;
            let r = RegEnumValueW(
                hkey,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            if r == ERROR_NO_MORE_ITEMS {
                break;
            }
            if r == ERROR_MORE_DATA && retries < 3 && data.len() < MAX_DATA {
                // Hodnota mezitím narostla: zvětšit a zkusit stejný index.
                retries += 1;
                data.resize((data_len as usize + 2).max(data.len() * 2).min(MAX_DATA), 0);
                if name.len() < MAX_NAME {
                    name.resize((name.len() * 2).min(MAX_NAME), 0);
                }
                continue;
            }
            if r == ERROR_MORE_DATA && data.len() >= MAX_DATA {
                // Hodnota přes strop: zjistit jen jméno a typ (bez dat).
                retries = 0;
                index += 1;
                let mut name_len = name.len() as u32;
                let mut kind = 0u32;
                let r = RegEnumValueW(
                    hkey,
                    index - 1,
                    Some(windows::core::PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    Some(&mut kind),
                    None,
                    None,
                );
                if r.is_ok() && (kind == 1 || kind == 2) {
                    out.push((
                        String::from_utf16_lossy(&name[..(name_len as usize).min(name.len())]),
                        String::new(),
                    ));
                }
                continue;
            }
            retries = 0;
            index += 1;
            if r.is_err() {
                // Jednu nečitelnou hodnotu přeskočit, výčet tím nekončí.
                continue;
            }
            // 1 = REG_SZ, 2 = REG_EXPAND_SZ.
            if kind == 1 || kind == 2 {
                let chars = (data_len as usize).min(data.len()) / 2;
                let wide: Vec<u16> = data[..chars * 2]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
                out.push((
                    String::from_utf16_lossy(&name[..(name_len as usize).min(name.len())]),
                    String::from_utf16_lossy(&wide[..end]),
                ));
            }
        }
        let _ = RegCloseKey(hkey);
    }
    out
}

/// Přečte REG_BINARY hodnotu (StartupApproved má 12 bajtů).
pub fn read_binary(root: HKEY, subkey: &str, value: &str) -> Option<Vec<u8>> {
    let subkey = HSTRING::from(subkey);
    let value = HSTRING::from(value);
    let mut buf = [0u8; 64];
    let mut len = buf.len() as u32;
    // SAFETY: buffer má pevnou velikost, len říká API kapacitu.
    unsafe {
        if RegGetValueW(
            root,
            &subkey,
            &value,
            RRF_RT_ANY,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
    }
    Some(buf[..len as usize].to_vec())
}

/// Zapíše REG_BINARY hodnotu. JEDINÁ zapisovací funkce v registry
/// modulu — smí ji volat pouze exekutor za validační vrstvou
/// (SPEC kap. 2, oddělené cesty). Klíč se v případě potřeby založí.
pub fn write_binary(
    root: HKEY,
    subkey: &str,
    value: &str,
    data: &[u8],
) -> Result<(), crate::Error> {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, KEY_SET_VALUE, REG_BINARY,
        REG_OPTION_NON_VOLATILE,
    };
    let wsub = HSTRING::from(subkey);
    let wval = HSTRING::from(value);
    let mut hkey = HKEY::default();
    // SAFETY: klíč se vždy zavírá; data mají délku dle slice.
    unsafe {
        RegCreateKeyExW(
            root,
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
        .map_err(|e| crate::Error::Win32 {
            call: "RegCreateKeyExW",
            code: e.code().0,
        })?;
        let r = RegSetValueExW(hkey, &wval, None, REG_BINARY, Some(data));
        let _ = RegCloseKey(hkey);
        r.ok().map_err(|e| crate::Error::Win32 {
            call: "RegSetValueExW",
            code: e.code().0,
        })
    }
}

/// Vyjmenuje názvy přímých podklíčů daného klíče.
pub fn enum_subkeys(root: HKEY, subkey: &str) -> Vec<String> {
    let mut out = Vec::new();
    let subkey = HSTRING::from(subkey);
    let mut hkey = HKEY::default();
    // SAFETY: otevřený klíč se vždy zavírá; buffery mají pevné velikosti.
    unsafe {
        if RegOpenKeyExW(
            root,
            &subkey,
            None,
            KEY_READ | KEY_ENUMERATE_SUB_KEYS,
            &mut hkey,
        )
        .is_err()
        {
            return out;
        }
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut name_len = name.len() as u32;
            if RegEnumKeyExW(
                hkey,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            )
            .is_err()
            {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..name_len as usize]));
            index += 1;
        }
        let _ = RegCloseKey(hkey);
    }
    out
}

/// Počká, až se pod klíčem něco změní — nebo až vyprší čas.
///
/// Proti opakovanému dotazování má tohle dvě výhody, kvůli kterým to
/// SPEC 13.4 přímo předepisuje: sledování stojí jedno spící vlákno
/// místo probouzení po sekundách, a hlavně se **nic nepropásne** —
/// aplikace, která si sáhne na mikrofon na dvě vteřiny mezi dvěma
/// dotazy, by jinak v historii nebyla.
///
/// `subtree` sleduje i podklíče (ConsentStore má aplikaci na každý
/// podklíč). Vrací `true`, když se něco změnilo, `false` při vypršení
/// nebo chybě — volající tak umí odejít i bez události.
pub fn wait_for_change(root: HKEY, subkey: &str, subtree: bool, timeout_ms: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Registry::{
        RegNotifyChangeKeyValue, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    let sub = HSTRING::from(subkey);
    let mut key = HKEY::default();
    // SAFETY: klíč se zavírá na všech cestách ven.
    unsafe {
        if RegOpenKeyExW(root, &sub, Some(0), KEY_READ, &mut key).is_err() {
            return false;
        }
        // Ruční událost — na signál se čeká jednou a pak se zahodí.
        let Ok(ev) = CreateEventW(None, true, false, None) else {
            let _ = RegCloseKey(key);
            return false;
        };
        let ok = RegNotifyChangeKeyValue(
            key,
            subtree,
            REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
            Some(ev),
            true,
        )
        .is_ok();
        let changed = ok && WaitForSingleObject(ev, timeout_ms) == WAIT_OBJECT_0;
        let _ = CloseHandle(ev);
        let _ = RegCloseKey(key);
        changed
    }
}

/// Smaže klíč včetně podklíčů a hodnot.
///
/// Jediná mazací operace v tomhle modulu a smí ji volat výhradně
/// exekutor za validační vrstvou. `RegDeleteTreeW` chce otevřený
/// rodičovský klíč s právem DELETE, proto se cesta dělí na rodiče
/// a poslední článek.
pub fn delete_key_tree(root: HKEY, subkey: &str) -> Result<(), crate::Error> {
    use windows::Win32::System::Registry::{RegDeleteTreeW, REG_SAM_FLAGS};
    // DELETE (0x0001_0000) je standardní přístupové právo; ve windows
    // crate pro registr vlastní konstantu nemá.
    const DELETE_RIGHT: u32 = 0x0001_0000;

    let subkey = subkey.trim_matches(char::from(92u8));
    let (parent, leaf) = match subkey.rfind(char::from(92u8)) {
        Some(i) => (&subkey[..i], &subkey[i + 1..]),
        None => ("", subkey),
    };
    if leaf.is_empty() {
        return Err(crate::Error::Win32 {
            call: "delete_key_tree: prázdný klíč",
            code: 0,
        });
    }
    let wparent = HSTRING::from(parent);
    let wleaf = HSTRING::from(leaf);
    let mut hkey = HKEY::default();
    // SAFETY: otevřený klíč se vždy zavírá.
    unsafe {
        RegOpenKeyExW(
            root,
            &wparent,
            None,
            REG_SAM_FLAGS(DELETE_RIGHT) | KEY_ENUMERATE_SUB_KEYS | KEY_READ,
            &mut hkey,
        )
        .ok()
        .map_err(|e| crate::Error::Win32 {
            call: "RegOpenKeyExW",
            code: e.code().0,
        })?;
        let r = RegDeleteTreeW(hkey, &wleaf);
        let _ = RegCloseKey(hkey);
        r.ok().map_err(|e| crate::Error::Win32 {
            call: "RegDeleteTreeW",
            code: e.code().0,
        })
    }
}
