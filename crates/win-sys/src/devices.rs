//! Inventář všech zařízení přes SetupAPI (SPEC kap. 15.1).
//!
//! Tohle je stejný zdroj, ze kterého čte Správce zařízení — projde se
//! celý strom přítomných zařízení a u každého se přečte, co o sobě
//! hlásí: jméno, výrobce, třída, ovladač a jeho verze.
//!
//! Vědomě **ne WMI**: `Win32_PnPEntity` je tentýž seznam, ale trvá
//! sekundy a umí se zaseknout. SetupAPI je přímé a rychlé.
//!
//! Prázdné pole znamená „zařízení to nehlásí" — nic se nedoplňuje
//! odhadem. Nefunkční zařízení (problém s ovladačem) se pozná podle
//! `problem_code`, ne podle toho, že by chyběla data.

use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_Status, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
    SetupDiGetClassDescriptionW, SetupDiGetClassDevsW, SetupDiGetDevicePropertyW,
    SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SPDRP_CLASS,
    SPDRP_CLASSGUID, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SPDRP_HARDWAREID, SPDRP_MFG,
    SP_DEVINFO_DATA,
};
use windows::Win32::Foundation::DEVPROPKEY;

/// Jedno zařízení ze systémového stromu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Device {
    /// Jméno, jak ho ukazuje Správce zařízení (FriendlyName, jinak popis).
    pub name: String,
    /// Výrobce, jak se hlásí v ovladači.
    pub manufacturer: String,
    /// Třída zařízení („Display", „Net", „DiskDrive"…).
    pub class: String,
    /// Lidský popis třídy („Grafické adaptéry"), když ho Windows mají.
    pub class_desc: String,
    /// První hardwarové ID — obsahuje VID/PID, tedy skutečný model.
    pub hardware_id: String,
    /// Verze a datum ovladače, když je zařízení hlásí.
    pub driver_version: String,
    pub driver_date: String,
    /// Kód problému z CM_Get_DevNode_Status (0 = běží v pořádku).
    /// Přesně to, co Správce zařízení kreslí vykřičníkem.
    pub problem_code: u32,
}

impl Device {
    /// Má zařízení problém (chybí ovladač, zakázané, konflikt)?
    pub fn has_problem(&self) -> bool {
        self.problem_code != 0
    }
}

/// DEVPKEY_Device_DriverVersion — {a8b865dd-2e3d-4094-ad97-e593a70c75d6} 3.
const DEVPKEY_DRIVER_VERSION: DEVPROPKEY = DEVPROPKEY {
    fmtid: windows::core::GUID::from_u128(0xa8b865dd_2e3d_4094_ad97_e593a70c75d6),
    pid: 3,
};
/// DEVPKEY_Device_DriverDate — tentýž fmtid, pid 2.
const DEVPKEY_DRIVER_DATE: DEVPROPKEY = DEVPROPKEY {
    fmtid: windows::core::GUID::from_u128(0xa8b865dd_2e3d_4094_ad97_e593a70c75d6),
    pid: 2,
};

/// Vyjmenuje všechna PŘÍTOMNÁ zařízení. Odpojené se neukazují —
/// seznam má odpovídat tomu, co v počítači opravdu je.
pub fn devices() -> Vec<Device> {
    let mut out = Vec::new();
    // SAFETY: seznam se vždy uvolní; buffery mají hlášené velikosti
    // a index enumerace se posouvá dle kontraktu SetupAPI.
    unsafe {
        let Ok(set): Result<HDEVINFO, _> =
            SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_ALLCLASSES)
        else {
            return out;
        };
        let mut index = 0u32;
        loop {
            let mut info = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInfo(set, index, &mut info).is_err() {
                break;
            }
            index += 1;

            // FriendlyName mívají jen některá zařízení — jinak popis.
            let name = match prop(set, &info, SPDRP_FRIENDLYNAME) {
                s if !s.is_empty() => s,
                _ => prop(set, &info, SPDRP_DEVICEDESC),
            };
            if name.is_empty() {
                continue;
            }
            let class = prop(set, &info, SPDRP_CLASS);
            let mut dev = Device {
                name,
                manufacturer: prop(set, &info, SPDRP_MFG),
                class_desc: class_description(set, &info),
                class,
                // HardwareID je REG_MULTI_SZ; první položka je ta
                // nejkonkrétnější (obsahuje revizi zařízení).
                hardware_id: prop(set, &info, SPDRP_HARDWAREID),
                driver_version: dev_prop(set, &info, &DEVPKEY_DRIVER_VERSION),
                driver_date: dev_prop(set, &info, &DEVPKEY_DRIVER_DATE),
                problem_code: 0,
            };
            let mut status = Default::default();
            let mut problem = Default::default();
            if CM_Get_DevNode_Status(&mut status, &mut problem, info.DevInst, 0).0 == 0 {
                dev.problem_code = problem.0;
            }
            out.push(dev);
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out.sort_by(|a, b| {
        a.class
            .cmp(&b.class)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// Textová vlastnost zařízení z registru (SPDRP_*).
/// # Safety
/// `set` a `info` musí pocházet z probíhající enumerace.
unsafe fn prop(
    set: HDEVINFO,
    info: &SP_DEVINFO_DATA,
    key: windows::Win32::Devices::DeviceAndDriverInstallation::SETUP_DI_REGISTRY_PROPERTY,
) -> String {
    let mut buf = [0u8; 1024];
    let mut needed = 0u32;
    if SetupDiGetDeviceRegistryPropertyW(set, info, key, None, Some(&mut buf), Some(&mut needed))
        .is_err()
    {
        return String::new();
    }
    wide_from_bytes(&buf)
}

/// Vlastnost přes moderní DEVPROPKEY (ovladač a jeho datum).
/// # Safety
/// `set` a `info` musí pocházet z probíhající enumerace.
unsafe fn dev_prop(set: HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> String {
    let mut kind = Default::default();
    let mut buf = [0u8; 512];
    let mut needed = 0u32;
    if SetupDiGetDevicePropertyW(
        set,
        info,
        key,
        &mut kind,
        Some(&mut buf),
        Some(&mut needed),
        0,
    )
    .is_err()
    {
        return String::new();
    }
    // DEVPROP_TYPE_FILETIME (0x00000010) u data ovladače.
    if kind.0 == 0x10 && needed as usize >= 8 {
        let ft = u64::from_le_bytes(buf[..8].try_into().unwrap_or_default());
        return filetime_to_date(ft);
    }
    wide_from_bytes(&buf)
}

/// Lidský popis třídy zařízení („Grafické adaptéry").
/// # Safety
/// `set` a `info` musí pocházet z probíhající enumerace.
unsafe fn class_description(set: HDEVINFO, info: &SP_DEVINFO_DATA) -> String {
    // Registr vrací GUID ve složených závorkách, parser je nechce.
    let guid_str = prop(set, info, SPDRP_CLASSGUID);
    let guid_str = guid_str.trim_matches(['{', '}']);
    let Ok(guid) = windows::core::GUID::try_from(guid_str) else {
        return String::new();
    };
    let mut buf = [0u16; 256];
    let mut needed = 0u32;
    if SetupDiGetClassDescriptionW(&guid, &mut buf, Some(&mut needed)).is_err() {
        return String::new();
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

/// UTF-16 řetězec z bajtového bufferu (REG_SZ i první položka MULTI_SZ).
fn wide_from_bytes(buf: &[u8]) -> String {
    let wide: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end]).trim().to_string()
}

/// FILETIME → „DD.MM.RRRR". Datum ovladače je jen na den přesné.
fn filetime_to_date(ft: u64) -> String {
    // SAFETY: struktura se jen vyplní z platného FILETIME.
    unsafe {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Time::FileTimeToSystemTime;
        let ft = FILETIME {
            dwLowDateTime: ft as u32,
            dwHighDateTime: (ft >> 32) as u32,
        };
        let mut st = Default::default();
        if FileTimeToSystemTime(&ft, &mut st).is_err() {
            return String::new();
        }
        let st: windows::Win32::Foundation::SYSTEMTIME = st;
        format!("{}. {}. {}", st.wDay, st.wMonth, st.wYear)
    }
}

// Připojené obrazovky se tady VĚDOMĚ nečtou: `EnumDisplayDevicesW`
// odpovídá za relaci volajícího a služba běží v session 0, kde žádná
// plocha není. Čte je UI proces ve své relaci — viz
// `crates/ui/src-tauri/src/display.rs`.

#[cfg(test)]
mod tests {
    use super::*;

    // Každý Windows má desítky zařízení a mezi nimi procesor i disk.
    #[test]
    fn enumerates_real_devices() {
        let d = devices();
        assert!(d.len() > 20, "jen {} zařízení — enumerace selhala", d.len());
        assert!(
            d.iter().any(|x| x.class == "Processor"),
            "chybí procesor mezi zařízeními"
        );
        assert!(
            d.iter().any(|x| x.class == "DiskDrive"),
            "chybí disk mezi zařízeními"
        );
        // Jméno je povinné — bezejmenná položka je k ničemu.
        assert!(d.iter().all(|x| !x.name.is_empty()));
    }
}
