//! Baterie (SPEC kap. 15.1): stav napájení + opotřebení článku.
//!
//! Dva zdroje, každý odpovídá na jinou otázku:
//! - `GetSystemPowerStatus` — nabito %, na baterii/v síti, odhad zbytku.
//!   Levné, jde volat v cyklu.
//! - battery IOCTL přes SetupAPI — návrhová vs. skutečná kapacita
//!   a počet cyklů, tedy „jak je baterka opotřebená". Jednorázově.
//!
//! Desktop bez baterie vrací `None` — nikdy se nepředstírá 100 %.

use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, BATTERY_INFORMATION, BATTERY_QUERY_INFORMATION,
    BATTERY_QUERY_INFORMATION_LEVEL, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::IO::DeviceIoControl;

/// Stav baterie. `None` u položek, které zařízení nehlásí — nikdy
/// dopočítané číslo (SPEC: nepředstírej hodnotu, kterou nemáš).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Battery {
    /// Nabití 0–100 %.
    pub percent: Option<u8>,
    /// Je stroj napájený ze sítě?
    pub ac_online: bool,
    /// Nabíjí se právě teď?
    pub charging: bool,
    /// Odhad zbývajícího času na baterii (s) — Windows ho hlásí jen
    /// při vybíjení a chvíli po odpojení nic.
    pub remaining_s: Option<u32>,
    /// Návrhová kapacita článku (mWh).
    pub design_mwh: Option<u32>,
    /// Skutečná kapacita plně nabitého článku dnes (mWh).
    pub full_mwh: Option<u32>,
    /// Počet nabíjecích cyklů, když ho firmware hlásí.
    pub cycles: Option<u32>,
}

impl Battery {
    /// Opotřebení v procentech (0 = jako nová). `None`, když chybí
    /// některá kapacita — dopočítávat z ničeho nebudeme.
    pub fn wear_pct(&self) -> Option<f32> {
        let (design, full) = (self.design_mwh?, self.full_mwh?);
        if design == 0 {
            return None;
        }
        Some(((design.saturating_sub(full)) as f32 / design as f32) * 100.0)
    }
}

/// GUID_DEVICE_BATTERY — třída rozhraní baterií.
const GUID_DEVICE_BATTERY: windows::core::GUID =
    windows::core::GUID::from_u128(0x72631e54_78a4_11d0_bcf7_00aa00b7b32a);

const IOCTL_BATTERY_QUERY_TAG: u32 = 0x294040;
const IOCTL_BATTERY_QUERY_INFORMATION: u32 = 0x294044;

/// Přečte stav baterie. `None` = stroj žádnou nemá (desktop).
pub fn battery() -> Option<Battery> {
    let mut st = SYSTEM_POWER_STATUS::default();
    // SAFETY: struktura se jen vyplní, žádné vlastnictví.
    unsafe {
        GetSystemPowerStatus(&mut st).ok()?;
    }
    // BatteryFlag 128 = žádná baterie, 255 = neznámý stav.
    if st.BatteryFlag == 128 {
        return None;
    }
    let mut b = Battery {
        percent: (st.BatteryLifePercent <= 100).then_some(st.BatteryLifePercent),
        ac_online: st.ACLineStatus == 1,
        // Bit 3 = nabíjí se.
        charging: st.BatteryFlag & 8 != 0,
        remaining_s: (st.BatteryLifeTime != u32::MAX).then_some(st.BatteryLifeTime),
        ..Default::default()
    };
    if let Some((design, full, cycles)) = wear() {
        b.design_mwh = (design > 0).then_some(design);
        b.full_mwh = (full > 0).then_some(full);
        b.cycles = (cycles > 0).then_some(cycles);
    }
    Some(b)
}

/// Kapacity a cykly z prvního battery zařízení: (návrhová, plná, cykly).
fn wear() -> Option<(u32, u32, u32)> {
    // SAFETY: každý handle se zavírá; buffery mají hlášené velikosti.
    unsafe {
        let devs: HDEVINFO = SetupDiGetClassDevsW(
            Some(&GUID_DEVICE_BATTERY),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
        .ok()?;
        let mut result = None;
        // Bereme první baterii; víc jich mají jen speciální stroje.
        let mut iface = SP_DEVICE_INTERFACE_DATA {
            cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if SetupDiEnumDeviceInterfaces(devs, None, &GUID_DEVICE_BATTERY, 0, &mut iface).is_ok() {
            // Dvoufázově: nejdřív potřebná velikost, pak vlastní čtení.
            let mut need = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(devs, &iface, None, 0, Some(&mut need), None);
            if need > 0 {
                let mut buf = vec![0u8; need as usize];
                let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
                (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                if SetupDiGetDeviceInterfaceDetailW(devs, &iface, Some(detail), need, None, None)
                    .is_ok()
                {
                    let path = PCWSTR((*detail).DevicePath.as_ptr());
                    result = query_battery(path);
                }
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(devs);
        result
    }
}

/// Dotaz na jedno battery zařízení: nejdřív tag, pak informace.
/// # Safety
/// `path` musí ukazovat na platnou nulou ukončenou cestu k zařízení.
unsafe fn query_battery(path: PCWSTR) -> Option<(u32, u32, u32)> {
    let h: HANDLE = CreateFileW(
        path,
        (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        None,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        None,
    )
    .ok()?;

    let mut out = None;
    // Tag identifikuje konkrétní vloženou baterii; bez něj se ptát nedá.
    let mut wait = 0u32;
    let mut tag = 0u32;
    let mut ret = 0u32;
    let ok = DeviceIoControl(
        h,
        IOCTL_BATTERY_QUERY_TAG,
        Some(&mut wait as *mut _ as *mut _),
        std::mem::size_of::<u32>() as u32,
        Some(&mut tag as *mut _ as *mut _),
        std::mem::size_of::<u32>() as u32,
        Some(&mut ret),
        None,
    )
    .is_ok();
    if ok && tag != 0 {
        let mut info = BATTERY_INFORMATION::default();
        let q = BATTERY_QUERY_INFORMATION {
            BatteryTag: tag,
            InformationLevel: BATTERY_QUERY_INFORMATION_LEVEL(0), // BatteryInformation
            AtRate: 0,
        };
        if DeviceIoControl(
            h,
            IOCTL_BATTERY_QUERY_INFORMATION,
            Some(&q as *const _ as *const _),
            std::mem::size_of::<BATTERY_QUERY_INFORMATION>() as u32,
            Some(&mut info as *mut _ as *mut _),
            std::mem::size_of::<BATTERY_INFORMATION>() as u32,
            Some(&mut ret),
            None,
        )
        .is_ok()
        {
            out = Some((
                info.DesignedCapacity,
                info.FullChargedCapacity,
                info.CycleCount,
            ));
        }
    }
    let _ = CloseHandle(h);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Opotřebení se počítá jen z obou kapacit; jinak poctivě None.
    #[test]
    fn wear_needs_both_capacities() {
        let mut b = Battery {
            design_mwh: Some(50_000),
            full_mwh: Some(45_000),
            ..Default::default()
        };
        assert!((b.wear_pct().expect("wear") - 10.0).abs() < 0.01);

        b.full_mwh = None;
        assert!(b.wear_pct().is_none());

        b.design_mwh = Some(0);
        b.full_mwh = Some(0);
        assert!(b.wear_pct().is_none());
    }

    // Na desktopu None, na notebooku rozumné hodnoty — ani jedno není
    // chyba, jen se nesmí lhát.
    #[test]
    fn battery_is_absent_or_sane() {
        if let Some(b) = battery() {
            if let Some(p) = b.percent {
                assert!(p <= 100);
            }
            if let Some(w) = b.wear_pct() {
                assert!((0.0..=100.0).contains(&w));
            }
        }
    }
}
