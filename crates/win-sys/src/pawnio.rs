//! PawnIO — teplota CPU přímo z čidla procesoru.
//!
//! PawnIO (<https://github.com/namazso/PawnIO>) je open-source, digitálně
//! podepsaný ovladač, který si uživatel instaluje **sám** (pawnio.eu,
//! `winget install namazso.PawnIO`, nebo ho nabídne LibreHardwareMonitor
//! od verze 0.9.5). Používají ho LibreHardwareMonitor, FanControl a další.
//! Winsent ovladač ani instalátor nešíří — když PawnIO v systému není,
//! tahle cesta stojí jedno neúspěšné `CreateFileW` (~1 µs) a kaskáda
//! v `thermal` jde dál.
//!
//! Ovladač sám nic neumí, kód mu dodává volající jako modul. Přibalené
//! jsou dva oficiální moduly z PawnIO.Modules 0.2.11
//! (<https://github.com/namazso/PawnIO.Modules>, LGPL-2.1, viz
//! `pawnio/README.md` — otisky, podpis a zdrojový kód). Oficiální
//! ovladač přijme jen moduly podepsané autorem (RSA-4096), takže je
//! nejde podvrhnout ani upravit.
//!
//! Z modulů se volají **výhradně čtecí funkce s pevnou adresou**
//! (`ioctl_read_smn`, `ioctl_read_msr`). Moduly mají i zápisovou funkci
//! (zápis MSR — napětí, limity příkonu); Winsent ji nikdy nevolá: jména
//! volaných funkcí jsou pevná v `read_one` a veřejné API nic jiného
//! nedovolí.
//!
//! Ovladač pustí k sobě jen SYSTEM a správce — čte tedy služba.
//! Ovladač se otevírá jen na dobu jednoho čtení: když UI nikdo nemá
//! otevřené (třeba při hraní), Winsent na PawnIO vůbec nesahá.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    LocalFree, ERROR_ACCESS_DENIED, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL,
    WAIT_ABANDONED, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenMutexW, ReleaseMutex, WaitForSingleObject, MUTEX_MODIFY_STATE,
    SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::Win32::System::IO::DeviceIoControl;

/// Cesta k zařízení. GLOBALROOT funguje na všech verzích 2.x (2.1.0
/// starý `\\.\PawnIO` dočasně neměla) a používá ji i LibreHardwareMonitor.
const DEVICE: PCWSTR = w!(r"\\?\GLOBALROOT\Device\PawnIO");

/// CTL_CODE(0xA1B2, func, METHOD_BUFFERED, FILE_ANY_ACCESS) — `pawnio_um.h`.
const fn ctl_code(func: u32) -> u32 {
    (0xA1B2 << 16) | (func << 2)
}
const IOCTL_PIO_LOAD_BINARY: u32 = ctl_code(0x821);
const IOCTL_PIO_EXECUTE_FN: u32 = ctl_code(0x841);
/// Jméno funkce v žádosti: 32 B, ukončené nulou.
const FN_NAME_LEN: usize = 32;

/// AMD Zen: THM_TCON_CUR_TMP v prostoru SMN (k10temp v Linuxu i LHM).
const SMN_THM_TCON_CUR_TMP: u64 = 0x0005_9800;
/// Rozsah −49…206 °C místo 0…255 °C (CUR_TEMP_RANGE_SEL).
const CUR_TEMP_RANGE_SEL: u32 = 1 << 19;
/// Totéž, když jsou oba bity TJ_SEL nastavené.
const CUR_TEMP_TJ_SEL: u32 = 0b11 << 16;
/// Intel: TjMax (bity 23:16) a teplota celého pouzdra (odstup od TjMax).
const IA32_TEMPERATURE_TARGET: u64 = 0x1A2;
const IA32_PACKAGE_THERM_STATUS: u64 = 0x1B1;

/// Konvence nástrojů pro hardwarový monitoring (LibreHardwareMonitor a další):
/// kdo sahá na PCI konfigurační prostor, drží tenhle mutex. Čtení SMN
/// jsou dva kroky (zapsat index, přečíst data) a cizí nástroj by je
/// jinak mohl prostřídat.
const PCI_MUTEX: PCWSTR = w!(r"Global\Access_PCI");
/// Jako LHM: na mutexu smí čekat a uvolnit ho kdokoli — jinak by ho
/// služba (SYSTEM) založila tak, že by ostatní nástroje nemohly čekat.
const PCI_MUTEX_SDDL: PCWSTR = w!("D:(A;;0x1F0001;;;WD)");
const PCI_LOCK_MS: u32 = 20;

/// Odmítnutý modul (starý ovladač, nečekaný procesor) se nezkouší při
/// každém čtení, ale až po téhle době.
const ZNOVU_PO_ODMITNUTI: Duration = Duration::from_secs(600);

static AMD_FAMILY17: &[u8] = include_bytes!("../pawnio/AMDFamily17.bin");
static INTEL_MSR: &[u8] = include_bytes!("../pawnio/IntelMSR.bin");

#[derive(Debug, Clone, Copy, PartialEq)]
enum Cpu {
    /// AMD rodiny 17h–1Ah (Zen). Posun Tctl → Tdie podle modelu.
    Amd { tdie_offset: f32 },
    /// Intel s teplotou pouzdra (CPUID.06H:EAX bit 6).
    Intel,
    /// Cokoli jiného — moduly by stejně odmítly načtení.
    Jiny,
}

/// Stav mezi voláními: jen procesor (zjistí se jednou) a čas
/// posledního odmítnutí. Handle na ovladač se mezi voláními nedrží.
struct Stav {
    cpu: Option<Cpu>,
    odmitnuto: Option<Instant>,
}

static STAV: Mutex<Stav> = Mutex::new(Stav {
    cpu: None,
    odmitnuto: None,
});

/// Je PawnIO v systému (ovladač běží)? Funguje i z UI bez práv správce:
/// existující zařízení, ke kterému nemáme přístup, hlásí „přístup
/// odepřen“, chybějící „nenalezeno“.
pub fn installed() -> bool {
    match open() {
        Ok(_) => true,
        Err(e) => e.code() == ERROR_ACCESS_DENIED.to_hresult(),
    }
}

/// Verze nainstalovaného PawnIO z jeho záznamu v Přidat/odebrat programy.
pub fn version() -> Option<String> {
    crate::registry::read_string(
        windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO",
        "DisplayVersion",
    )
}

/// Teplota CPU ve °C přes PawnIO, nebo `None` — PawnIO není, volající
/// nemá práva (vše kromě služby), procesor není podporovaný, nebo
/// hodnota nedává smysl. Nikdy nic nepředstírá.
pub fn cpu_temp() -> Option<f32> {
    let mut stav = STAV.lock().unwrap_or_else(|p| p.into_inner());
    let cpu = *stav.cpu.get_or_insert_with(detect_cpu);
    let modul = match cpu {
        Cpu::Amd { .. } => AMD_FAMILY17,
        Cpu::Intel => INTEL_MSR,
        Cpu::Jiny => return None,
    };
    if stav.odmitnuto.is_some_and(|t| t.elapsed() < ZNOVU_PO_ODMITNUTI) {
        return None;
    }
    // Chybějící ovladač nebo chybějící práva jsou běžný stav, ne chyba.
    let dev = open().ok()?;
    if let Err(e) = ioctl(&dev, IOCTL_PIO_LOAD_BINARY, modul, &mut []) {
        // Podpis nesedí se starým ovladačem, nebo main() modulu procesor
        // odmítl. Jednou zalogovat a chvíli nezkoušet.
        if stav.odmitnuto.is_none() {
            tracing::warn!("PawnIO odmítlo modul: {e}");
        }
        stav.odmitnuto = Some(Instant::now());
        return None;
    }
    stav.odmitnuto = None;
    let t = match cpu {
        Cpu::Amd { tdie_offset } => read_amd(&dev).map(|t| t - tdie_offset),
        Cpu::Intel => read_intel(&dev),
        Cpu::Jiny => None,
    };
    // Handle se zavře tady (drop) a ovladač tím zahodí i načtený modul.
    t.filter(|c| (5.0..=120.0).contains(c))
}

fn open() -> windows::core::Result<OwnedHandle> {
    // SAFETY: platná nulou ukončená cesta; vrácený handle hned přebírá
    // OwnedHandle, který ho zavře.
    unsafe {
        let h = CreateFileW(
            DEVICE,
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?;
        Ok(OwnedHandle::from_raw_handle(h.0))
    }
}

/// Jedno volání ovladače. Výstup musí přijít celý — PawnIO vrací vždy
/// přesně tolik, kolik se žádá; méně znamená něco jiného, než čekáme.
fn ioctl(dev: &OwnedHandle, code: u32, input: &[u8], out: &mut [u64]) -> windows::core::Result<()> {
    let mut vraceno = 0u32;
    let out_len = std::mem::size_of_val(out) as u32;
    // SAFETY: oba buffery žijí po celou dobu synchronního volání a
    // velikosti odpovídají jejich skutečné délce.
    unsafe {
        DeviceIoControl(
            HANDLE(dev.as_raw_handle()),
            code,
            Some(input.as_ptr().cast()),
            input.len() as u32,
            (!out.is_empty()).then(|| out.as_mut_ptr().cast()),
            out_len,
            Some(&mut vraceno),
            None,
        )?;
    }
    if vraceno != out_len {
        return Err(windows::Win32::Foundation::E_UNEXPECTED.into());
    }
    Ok(())
}

/// Čtecí funkce modulu s jedním argumentem a jedním výsledkem. Jména
/// jsou pevná — nic jiného než čtení se odsud zavolat nedá.
#[derive(Clone, Copy)]
enum Cteni {
    Smn,
    Msr,
}

fn read_one(dev: &OwnedHandle, co: Cteni, adresa: u64) -> Option<u64> {
    let jmeno: &[u8] = match co {
        Cteni::Smn => b"ioctl_read_smn",
        Cteni::Msr => b"ioctl_read_msr",
    };
    // Žádost: 32 B jména (zbytek nuly = ukončení) + argument (u64 LE).
    let mut zadost = [0u8; FN_NAME_LEN + 8];
    zadost[..jmeno.len()].copy_from_slice(jmeno);
    zadost[FN_NAME_LEN..].copy_from_slice(&adresa.to_le_bytes());
    let mut out = [0u64; 1];
    ioctl(dev, IOCTL_PIO_EXECUTE_FN, &zadost, &mut out).ok()?;
    Some(out[0])
}

fn read_amd(dev: &OwnedHandle) -> Option<f32> {
    let _zamek = PciZamek::vezmi()?;
    let raw = read_one(dev, Cteni::Smn, SMN_THM_TCON_CUR_TMP)?;
    decode_tctl(raw as u32)
}

/// Tctl z registru THM_TCON_CUR_TMP: 11 bitů po 0,125 °C od bitu 21,
/// v rozšířeném rozsahu posunuté o −49 °C (Linux k10temp, LHM).
fn decode_tctl(raw: u32) -> Option<f32> {
    // 0 = nic nepřečteno, samé jedničky = chybová odpověď PCI.
    if raw == 0 || raw == u32::MAX {
        return None;
    }
    let mut t = (raw >> 21) as f32 * 0.125;
    if raw & CUR_TEMP_RANGE_SEL != 0 || raw & CUR_TEMP_TJ_SEL == CUR_TEMP_TJ_SEL {
        t -= 49.0;
    }
    Some(t)
}

fn read_intel(dev: &OwnedHandle) -> Option<f32> {
    let tjmax = (read_one(dev, Cteni::Msr, IA32_TEMPERATURE_TARGET)? >> 16) & 0xFF;
    // Bez TjMax nejde teplotu spočítat; LHM by dosadil 100 °C, tady se
    // radši přizná, že teplotu neznáme.
    if tjmax == 0 {
        return None;
    }
    let odstup = (read_one(dev, Cteni::Msr, IA32_PACKAGE_THERM_STATUS)? >> 16) & 0x7F;
    Some(tjmax as f32 - odstup as f32)
}

/// Drží `Global\Access_PCI` jen po dobu jednoho čtení.
struct PciZamek(OwnedHandle);

impl PciZamek {
    /// `None` = mutex drží někdo jiný déle než 20 ms; čtení se vynechá.
    /// Když mutex nejde vůbec založit ani otevřít, čte se bez něj
    /// (stejně jako LHM) — vrací se zámek bez mutexu.
    fn vezmi() -> Option<Option<PciZamek>> {
        let Some(h) = pci_mutex() else {
            return Some(None);
        };
        // SAFETY: platný handle mutexu.
        let w = unsafe { WaitForSingleObject(HANDLE(h.as_raw_handle()), PCI_LOCK_MS) };
        // Opuštěný mutex (držitel spadl) je náš stejně jako volný.
        (w == WAIT_OBJECT_0 || w == WAIT_ABANDONED).then(|| Some(PciZamek(h)))
    }
}

impl Drop for PciZamek {
    fn drop(&mut self) {
        // SAFETY: mutex vlastní tohle vlákno (vzal ho `vezmi` ve stejném
        // volání); handle zavře OwnedHandle až po uvolnění.
        unsafe {
            let _ = ReleaseMutex(HANDLE(self.0.as_raw_handle()));
        }
    }
}

fn pci_mutex() -> Option<OwnedHandle> {
    // SAFETY: deskriptor se po založení mutexu uvolní; handle přebírá
    // OwnedHandle.
    unsafe {
        let mut psd = PSECURITY_DESCRIPTOR::default();
        let h = if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCI_MUTEX_SDDL,
            SDDL_REVISION_1,
            &mut psd,
            None,
        )
        .is_ok()
        {
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: psd.0,
                bInheritHandle: false.into(),
            };
            let h = CreateMutexW(Some(&sa), false, PCI_MUTEX);
            let _ = LocalFree(Some(HLOCAL(psd.0)));
            h
        } else {
            CreateMutexW(None, false, PCI_MUTEX)
        };
        // Mutex už existuje s přísnějšími právy — aspoň ho otevřít.
        let h = h
            .or_else(|_| OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE, false, PCI_MUTEX))
            .ok()?;
        Some(OwnedHandle::from_raw_handle(h.0))
    }
}

fn detect_cpu() -> Cpu {
    #[allow(unused_unsafe)]
    // SAFETY: CPUID je na x86_64 vždy k dispozici.
    let cpuid = |leaf: u32| unsafe { core::arch::x86_64::__cpuid(leaf) };
    let v = cpuid(0);
    let mut vendor = [0u8; 12];
    vendor[..4].copy_from_slice(&v.ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&v.edx.to_le_bytes());
    vendor[8..].copy_from_slice(&v.ecx.to_le_bytes());
    let eax = cpuid(1).eax;
    let zaklad = (eax >> 8) & 0xF;
    let rodina = zaklad + if zaklad == 0xF { (eax >> 20) & 0xFF } else { 0 };
    match &vendor {
        b"AuthenticAMD" if (0x17..=0x1A).contains(&rodina) => {
            let mut znacka = Vec::with_capacity(48);
            if cpuid(0x8000_0000).eax >= 0x8000_0004 {
                for leaf in 0x8000_0002..=0x8000_0004 {
                    let r = cpuid(leaf);
                    for x in [r.eax, r.ebx, r.ecx, r.edx] {
                        znacka.extend_from_slice(&x.to_le_bytes());
                    }
                }
            }
            let znacka = String::from_utf8_lossy(&znacka);
            Cpu::Amd {
                tdie_offset: if rodina == 0x17 { tdie_offset(&znacka) } else { 0.0 },
            }
        }
        // Teplota pouzdra (PTM) je od Sandy Bridge; bez ní by se musela
        // číst jádra po jednom s přišpendleným vláknem — to tu není.
        b"GenuineIntel" if cpuid(0).eax >= 6 && cpuid(6).eax & (1 << 6) != 0 => Cpu::Intel,
        _ => Cpu::Jiny,
    }
}

/// Některé Zen 1/Zen+ hlásí Tctl posunuté nahoru kvůli ventilátorům;
/// skutečná teplota čipu (Tdie) je o tolik nižší. Tabulka z LHM
/// (Amd17Cpu.cs) a Linuxu (k10temp.c). Zen 2 a novější posun nemají.
fn tdie_offset(znacka: &str) -> f32 {
    const POSUNY: [(&str, f32); 6] = [
        ("AMD Ryzen 5 1600X", 20.0),
        ("AMD Ryzen 7 1700X", 20.0),
        ("AMD Ryzen 7 1800X", 20.0),
        ("AMD Ryzen 7 2700X", 10.0),
        ("AMD Ryzen Threadripper 19", 27.0),
        ("AMD Ryzen Threadripper 29", 27.0),
    ];
    POSUNY
        .iter()
        .find(|(z, _)| znacka.contains(z))
        .map_or(0.0, |&(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Kódy IOCTL z pawnio_um.h — přepočet CTL_CODE nesmí ujet.
    #[test]
    fn ioctl_kody() {
        assert_eq!(IOCTL_PIO_LOAD_BINARY, 0xA1B2_2084);
        assert_eq!(IOCTL_PIO_EXECUTE_FN, 0xA1B2_2104);
    }

    #[test]
    fn tctl_dekodovani() {
        assert_eq!(decode_tctl(360 << 21), Some(45.0));
        assert_eq!(decode_tctl((760 << 21) | CUR_TEMP_RANGE_SEL), Some(46.0));
        assert_eq!(decode_tctl((760 << 21) | CUR_TEMP_TJ_SEL), Some(46.0));
        // Jen jeden bit TJ_SEL rozsah neposouvá.
        assert_eq!(decode_tctl((360 << 21) | (1 << 16)), Some(45.0));
        assert_eq!(decode_tctl(0), None);
        assert_eq!(decode_tctl(u32::MAX), None);
    }

    #[test]
    fn posun_tdie_podle_modelu() {
        assert_eq!(tdie_offset("AMD Ryzen 5 3600X 6-Core Processor"), 0.0);
        assert_eq!(tdie_offset("AMD Ryzen 7 2700X Eight-Core Processor"), 10.0);
        assert_eq!(tdie_offset("AMD Ryzen Threadripper 1950X 16-Core Processor"), 27.0);
    }

    // Moduly jsou přesně ty ověřené z oficiálního vydání (SHA-256 viz
    // pawnio/README.md) — náhodná záměna souboru by tu spadla.
    #[test]
    fn moduly_maji_hlavicku_s_podpisem() {
        for m in [AMD_FAMILY17, INTEL_MSR] {
            let sig = u32::from_le_bytes(m[..4].try_into().unwrap()) as usize;
            assert_eq!(sig, 512, "podpis RSA-4096");
            assert!(m.len() > 4 + sig);
        }
        assert_eq!(AMD_FAMILY17.len(), 10_652);
        assert_eq!(INTEL_MSR.len(), 5_324);
    }

    // Bez služby (test běží jako uživatel) PawnIO nikdy nic nevrátí:
    // buď chybí, nebo nejsou práva. Hlavně nesmí spadnout.
    #[test]
    fn bez_prav_nic_nepredstira() {
        let t = cpu_temp();
        assert!(t.is_none() || (5.0..=120.0).contains(&t.unwrap()));
    }
}
