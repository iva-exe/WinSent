//! Obecné registry helpery (čtení hodnot, enumerace podklíčů).
//! Jen čtení — zápisy do registru patří výhradně za validační vrstvu.

use windows::core::HSTRING;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, KEY_ENUMERATE_SUB_KEYS,
    KEY_READ, RRF_RT_REG_SZ,
};

pub use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

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
