//! Obecné registry helpery (čtení hodnot, enumerace podklíčů).
//! Jen čtení — zápisy do registru patří výhradně za validační vrstvu.

use windows::core::HSTRING;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, KEY_ENUMERATE_SUB_KEYS,
    KEY_READ, RRF_RT_ANY, RRF_RT_REG_SZ,
};

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
    use windows::Win32::System::Registry::RegEnumValueW;
    let mut out = Vec::new();
    let wsub = HSTRING::from(subkey);
    let mut hkey = HKEY::default();
    // SAFETY: klíč se vždy zavírá; buffery mají pevné velikosti a délky
    // se předávají API dle kontraktu.
    unsafe {
        if RegOpenKeyExW(root, &wsub, None, KEY_READ, &mut hkey).is_err() {
            return out;
        }
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 512];
            let mut name_len = name.len() as u32;
            let mut data = [0u8; 2048];
            let mut data_len = data.len() as u32;
            let mut kind = 0u32;
            if RegEnumValueW(
                hkey,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            )
            .is_err()
            {
                break;
            }
            index += 1;
            // 1 = REG_SZ, 2 = REG_EXPAND_SZ.
            if kind == 1 || kind == 2 {
                let chars = (data_len as usize / 2).min(1024);
                let wide: Vec<u16> = data[..chars * 2]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
                out.push((
                    String::from_utf16_lossy(&name[..name_len as usize]),
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
