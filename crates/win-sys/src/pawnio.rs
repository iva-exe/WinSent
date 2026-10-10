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
//! ## Jen čtení — a jak je to zajištěné
//!
//! Moduly umí i zápis (MSR: napětí, limity příkonu). Winsent ho nikdy
//! nevolá a tenhle soubor je postavený tak, aby to ani udělat nešlo:
//!
//! 1. **Uzavřený výčet čtení** ([`Cteni`]). Jméno funkce i adresa jsou
//!    pevně svázané v jedné hodnotě výčtu; jiný řetězec ani jiná adresa
//!    se do žádosti dostat nedají. Žádost skládá jediná funkce
//!    ([`zadost`]) a ta bere jen [`Cteni`].
//! 2. **Uzavřený výčet modulů** ([`Modul`]). Ovladači jde předat jen
//!    jeden z obou přibalených souborů, a to až po kontrole SHA-256
//!    proti otisku z oficiálního vydání. Kdyby někdo v repozitáři soubor
//!    vyměnil (třeba za jiný podepsaný modul se zápisem do hardwaru),
//!    služba ho nenačte a test spadne.
//! 3. **Jen dva kódy IOCTL** (načtení modulu, volání funkce) a jen tady.
//!    Nic jiného v celém Winsentu na zařízení PawnIO nesahá.
//! 4. **Žádný vstup zvenku.** Veřejné API je [`cpu_temp`], [`installed`]
//!    a [`version`]; nic z nich nebere parametr, který by šel do
//!    ovladače. Přes IPC služby tak klient dostane nanejvýš číslo
//!    teploty.
//! 5. **UI na ovladač nesahá.** K zařízení smí jen SYSTEM a správci;
//!    UI (běžný uživatel) zjišťuje přítomnost PawnIO jen dotazem na
//!    správce služeb, zařízení vůbec neotevírá.
//!
//! Jediný zápis, který čtení vůbec obsahuje: na AMD se registr SMN čte
//! přes dvojici index/data v konfiguračním prostoru PCI (0:0.0, offsety
//! 0x60/0x64). Do indexu se zapíše adresa 0x59800 — to jen vybírá, který
//! registr se čte, žádné nastavení procesoru to nemění (stejně to dělá
//! Linux k10temp). Protože je ta dvojice sdílená s ostatními nástroji,
//! čte se jen pod mutexem `Global\Access_PCI`; bez něj se čtení vynechá.
//! Na Intelu jde o čisté čtení MSR.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    LocalFree, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, WAIT_ABANDONED, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
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
/// Jméno služby ovladače ve správci služeb.
const SLUZBA: PCWSTR = w!("PawnIO");

/// CTL_CODE(0xA1B2, func, METHOD_BUFFERED, FILE_ANY_ACCESS) — `pawnio_um.h`.
const fn ctl_code(func: u32) -> u32 {
    (0xA1B2 << 16) | (func << 2)
}
const IOCTL_PIO_LOAD_BINARY: u32 = ctl_code(0x821);
const IOCTL_PIO_EXECUTE_FN: u32 = ctl_code(0x841);
/// Jméno funkce v žádosti: 32 B, ukončené nulou.
const FN_NAME_LEN: usize = 32;

/// Rozsah −49…206 °C místo 0…255 °C (CUR_TEMP_RANGE_SEL).
const CUR_TEMP_RANGE_SEL: u32 = 1 << 19;
/// Totéž, když jsou oba bity TJ_SEL nastavené.
const CUR_TEMP_TJ_SEL: u32 = 0b11 << 16;

/// Konvence nástrojů pro hardwarový monitoring (LibreHardwareMonitor a další):
/// kdo sahá na PCI konfigurační prostor, drží tenhle mutex. Čtení SMN
/// jsou dva kroky (zapsat index, přečíst data) a cizí nástroj by je
/// jinak mohl prostřídat.
const PCI_MUTEX: PCWSTR = w!(r"Global\Access_PCI");
/// SYSTEM a správci mají k mutexu plná práva (LHM a spol. běží jako
/// správce a otevírají ho s MUTEX_ALL_ACCESS); ostatní smí jen čekat
/// a uvolnit (SYNCHRONIZE | MUTEX_MODIFY_STATE), což stačí i .NET
/// `Mutex.OpenExisting`. LHM dává všem plná práva — pak by ale kdokoli
/// mohl přepsat DACL (WRITE_DAC) a koordinaci nástrojů vypnout.
/// Podržet mutex může kdokoli dál (tak konvence funguje); Winsent pak
/// jen vynechá čtení.
const PCI_MUTEX_SDDL: PCWSTR = w!("D:(A;;0x1F0001;;;SY)(A;;0x1F0001;;;BA)(A;;0x100001;;;WD)");
/// Jak dlouho čekat na mutex. Čtení samo trvá mikrosekundy, kdo mutex
/// drží poctivě, pustí ho hned; déle se nečeká, ať sampler nestojí.
const PCI_LOCK_MS: u32 = 5;
/// Po tolika obsazených pokusech v řadě se na mutex chvíli nečeká vůbec.
const PCI_OBSAZENO_LIMIT: u32 = 5;
const PCI_PAUZA: Duration = Duration::from_secs(30);

/// Odmítnutý modul (starý ovladač, nečekaný procesor) se nezkouší při
/// každém čtení, ale až po téhle době.
const ZNOVU_PO_ODMITNUTI: Duration = Duration::from_secs(600);

static AMD_FAMILY17: &[u8] = include_bytes!("../pawnio/AMDFamily17.bin");
static INTEL_MSR: &[u8] = include_bytes!("../pawnio/IntelMSR.bin");

/// Moduly, které Winsent ovladači vůbec umí předat. Nic jiného nejde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modul {
    AmdFamily17,
    IntelMsr,
}

impl Modul {
    fn data(self) -> &'static [u8] {
        match self {
            Modul::AmdFamily17 => AMD_FAMILY17,
            Modul::IntelMsr => INTEL_MSR,
        }
    }

    /// SHA-256 souboru z oficiálního vydání PawnIO.Modules 0.2.11
    /// (stejné otisky jsou v `pawnio/README.md`).
    fn otisk(self) -> &'static str {
        match self {
            Modul::AmdFamily17 => "dae74615761b78bdf064dfb3e136252ddcc6fc727d88f14738d0e5800d427a91",
            Modul::IntelMsr => "d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f",
        }
    }

    /// Sedí přibalený soubor s otiskem? Počítá se jednou za běh služby.
    fn overeny(self) -> bool {
        static AMD: OnceLock<bool> = OnceLock::new();
        static INTEL: OnceLock<bool> = OnceLock::new();
        let bunka = match self {
            Modul::AmdFamily17 => &AMD,
            Modul::IntelMsr => &INTEL,
        };
        *bunka.get_or_init(|| sha256_hex(self.data()).as_deref() == Some(self.otisk()))
    }
}

/// Jediná čtení, která Winsent z modulů volá. Jméno funkce a adresa
/// jsou svázané tady a nikde jinde — žádost s jiným jménem (třeba
/// zápisem) nebo jinou adresou se v tomhle souboru složit nedá.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cteni {
    /// AMD Zen: THM_TCON_CUR_TMP v prostoru SMN (k10temp v Linuxu i LHM).
    AmdTctl,
    /// Intel: IA32_TEMPERATURE_TARGET — TjMax v bitech 23:16.
    IntelTjMax,
    /// Intel: IA32_PACKAGE_THERM_STATUS — odstup teploty pouzdra od TjMax.
    IntelPouzdro,
}

impl Cteni {
    #[cfg(test)]
    const VSECHNA: [Cteni; 3] = [Cteni::AmdTctl, Cteni::IntelTjMax, Cteni::IntelPouzdro];

    fn funkce(self) -> &'static [u8] {
        match self {
            Cteni::AmdTctl => b"ioctl_read_smn",
            Cteni::IntelTjMax | Cteni::IntelPouzdro => b"ioctl_read_msr",
        }
    }

    fn adresa(self) -> u64 {
        match self {
            Cteni::AmdTctl => 0x0005_9800,
            Cteni::IntelTjMax => 0x1A2,
            Cteni::IntelPouzdro => 0x1B1,
        }
    }

    /// Modul, ve kterém funkce je — jinde se volat nesmí.
    fn modul(self) -> Modul {
        match self {
            Cteni::AmdTctl => Modul::AmdFamily17,
            Cteni::IntelTjMax | Cteni::IntelPouzdro => Modul::IntelMsr,
        }
    }
}

/// Žádost pro IOCTL_PIO_EXECUTE_FN: 32 B jména (zbytek nuly =
/// ukončení) + jeden argument (u64 LE). Jediné místo, kde žádost vzniká.
fn zadost(c: Cteni) -> [u8; FN_NAME_LEN + 8] {
    let mut z = [0u8; FN_NAME_LEN + 8];
    let jmeno = c.funkce();
    z[..jmeno.len()].copy_from_slice(jmeno);
    z[FN_NAME_LEN..].copy_from_slice(&c.adresa().to_le_bytes());
    z
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Cpu {
    /// AMD rodiny 17h–1Ah (Zen). Posun Tctl → Tdie podle modelu.
    Amd { tdie_offset: f32 },
    /// Intel s teplotou pouzdra (CPUID.06H:EAX bit 6).
    Intel,
    /// Cokoli jiného — moduly by stejně odmítly načtení.
    Jiny,
}

/// Otevřené zařízení s načteným modulem. Služba čte teplotu každou
/// sekundu (živý graf i historie), takže se modul načte jednou a drží
/// — opakované načítání by pokaždé stálo ověření podpisu v ovladači.
/// Při chybě volání se sezení zahodí a příště se otevře znovu (ovladač
/// mohl být aktualizován nebo odebrán).
struct Sezeni {
    dev: OwnedHandle,
    modul: Modul,
    /// `Global\Access_PCI` (jen AMD); None = zatím nejde založit ani
    /// otevřít — zkouší se znovu při každém čtení, bez něj se nečte.
    pci: Option<OwnedHandle>,
    /// Obsazený mutex v řadě a do kdy se na něj nečeká (viz PCI_PAUZA).
    pci_obsazeno: u32,
    pci_pauza_do: Option<Instant>,
    /// TjMax (Intel) — mění se jen s procesorem, čte se jednou.
    tjmax: Option<u8>,
    /// Už se z tohohle sezení aspoň jednou přečetlo?
    precteno: bool,
}

struct Stav {
    cpu: Option<Cpu>,
    odmitnuto: Option<Instant>,
    sezeni: Option<Sezeni>,
}

static STAV: Mutex<Stav> = Mutex::new(Stav {
    cpu: None,
    odmitnuto: None,
    sezeni: None,
});

/// Je PawnIO v systému a běží? Jen dotaz na správce služeb — zařízení
/// se neotevírá, takže to bez rizika smí volat i UI pod běžným
/// uživatelem.
pub fn installed() -> bool {
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT,
        SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS,
    };
    // SAFETY: oba handly se zavřou; struktura stavu žije přes volání.
    unsafe {
        let Ok(scm) = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) else {
            return false;
        };
        let bezi = match OpenServiceW(scm, SLUZBA, SERVICE_QUERY_STATUS) {
            Ok(svc) => {
                let mut st = SERVICE_STATUS::default();
                let ok = QueryServiceStatus(svc, &mut st).is_ok()
                    && st.dwCurrentState == SERVICE_RUNNING;
                let _ = CloseServiceHandle(svc);
                ok
            }
            Err(_) => false,
        };
        let _ = CloseServiceHandle(scm);
        bezi
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
        Cpu::Amd { .. } => Modul::AmdFamily17,
        Cpu::Intel => Modul::IntelMsr,
        Cpu::Jiny => return None,
    };
    if stav.odmitnuto.is_some_and(|t| t.elapsed() < ZNOVU_PO_ODMITNUTI) {
        return None;
    }
    if stav.sezeni.is_none() {
        match otevri(modul) {
            Ok(s) => {
                stav.sezeni = Some(s);
                stav.odmitnuto = None;
            }
            // Chybějící ovladač nebo chybějící práva jsou běžný stav.
            Err(Otevreni::Neni) => return None,
            Err(Otevreni::Odmitnuto(proc)) => {
                // Podpis nesedí se starým ovladačem, main() modulu
                // procesor odmítl, nebo přibalený soubor neprošel
                // kontrolou otisku. Jednou zalogovat a chvíli nezkoušet.
                if stav.odmitnuto.is_none() {
                    tracing::warn!("PawnIO: modul se nenačetl: {proc}");
                }
                stav.odmitnuto = Some(Instant::now());
                return None;
            }
        }
    }
    let sezeni = stav.sezeni.as_mut()?;
    let cerstve = !sezeni.precteno;
    let t = match cpu {
        Cpu::Amd { tdie_offset } => read_amd(sezeni).map(|t| t.map(|t| t - tdie_offset)),
        Cpu::Intel => read_intel(sezeni),
        Cpu::Jiny => Ok(None),
    };
    match t {
        Ok(t) => {
            sezeni.precteno = true;
            t.filter(|c| (5.0..=120.0).contains(c))
        }
        Err(()) => {
            stav.sezeni = None;
            if cerstve {
                // Selhalo hned první čtení po načtení modulu (třeba VM,
                // kde PCI 0:0.0 není AMD): stejné jako odmítnutí — jinak
                // by se modul každou sekundu načítal a ověřoval znovu.
                if stav.odmitnuto.is_none() {
                    tracing::warn!("PawnIO: čtení selhalo hned po načtení modulu");
                }
                stav.odmitnuto = Some(Instant::now());
            }
            // Jinak handle mohl přestat platit (aktualizace nebo odebrání
            // ovladače) — příště se otevře znovu.
            None
        }
    }
}

enum Otevreni {
    Neni,
    Odmitnuto(String),
}

fn otevri(modul: Modul) -> Result<Sezeni, Otevreni> {
    // Kontrola otisku PŘED tím, než se čehokoli dotkne ovladač.
    if !modul.overeny() {
        return Err(Otevreni::Odmitnuto(format!(
            "přibalený {modul:?} neodpovídá oficiálnímu otisku"
        )));
    }
    // SAFETY: platná nulou ukončená cesta; vrácený handle hned přebírá
    // OwnedHandle, který ho zavře.
    let dev = unsafe {
        CreateFileW(
            DEVICE,
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map(|h| OwnedHandle::from_raw_handle(h.0))
        .map_err(|_| Otevreni::Neni)?
    };
    ioctl(&dev, IOCTL_PIO_LOAD_BINARY, modul.data(), &mut [])
        .map_err(|e| Otevreni::Odmitnuto(e.to_string()))?;
    let pci = (modul == Modul::AmdFamily17).then(pci_mutex).flatten();
    Ok(Sezeni {
        dev,
        modul,
        pci,
        pci_obsazeno: 0,
        pci_pauza_do: None,
        tjmax: None,
        precteno: false,
    })
}

/// Jedno volání ovladače. Volá se výhradně se dvěma kódy výše:
/// načtení modulu z [`Modul`] a funkce ze [`zadost`]. Výstup musí
/// přijít celý — PawnIO vrací vždy přesně tolik, kolik se žádá.
fn ioctl(dev: &OwnedHandle, code: u32, input: &[u8], out: &mut [u64]) -> windows::core::Result<()> {
    debug_assert!(code == IOCTL_PIO_LOAD_BINARY || code == IOCTL_PIO_EXECUTE_FN);
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

/// Jedno čtení z uzavřeného výčtu. `Err` = volání selhalo (sezení pryč).
fn precti(s: &Sezeni, c: Cteni) -> Result<u64, ()> {
    // Funkce z jiného modulu, než jaký je načtený, se nevolá vůbec.
    if c.modul() != s.modul {
        return Err(());
    }
    let mut out = [0u64; 1];
    ioctl(&s.dev, IOCTL_PIO_EXECUTE_FN, &zadost(c), &mut out).map_err(|_| ())?;
    Ok(out[0])
}

fn read_amd(s: &mut Sezeni) -> Result<Option<f32>, ()> {
    if s.pci_pauza_do.is_some_and(|t| Instant::now() < t) {
        return Ok(None);
    }
    // Bez mutexu se na sdílenou dvojici index/data nesahá vůbec (zavřeno,
    // ne otevřeno). Mutex se zkouší založit znovu — po krátkém výpadku
    // tak teplota naskočí sama.
    if s.pci.is_none() {
        s.pci = pci_mutex();
    }
    let Some(pci) = s.pci.as_ref() else {
        return Ok(None);
    };
    let Some(_zamek) = PciZamek::vezmi(pci) else {
        // Mutex drží jiný nástroj — tohle čtení se vynechá. Když ho drží
        // pořád, chvíli se nečeká vůbec, ať sampler nestojí každý tick.
        s.pci_obsazeno += 1;
        if s.pci_obsazeno >= PCI_OBSAZENO_LIMIT {
            s.pci_obsazeno = 0;
            s.pci_pauza_do = Some(Instant::now() + PCI_PAUZA);
        }
        return Ok(None);
    };
    s.pci_obsazeno = 0;
    let raw = precti(s, Cteni::AmdTctl)?;
    Ok(decode_tctl(raw as u32))
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

fn read_intel(s: &mut Sezeni) -> Result<Option<f32>, ()> {
    let tjmax = match s.tjmax {
        Some(t) => t,
        None => {
            let t = ((precti(s, Cteni::IntelTjMax)? >> 16) & 0xFF) as u8;
            // Bez TjMax nejde teplotu spočítat; LHM by dosadil 100 °C,
            // tady se radši přizná, že teplotu neznáme.
            if t == 0 {
                return Ok(None);
            }
            s.tjmax = Some(t);
            t
        }
    };
    let odstup = (precti(s, Cteni::IntelPouzdro)? >> 16) & 0x7F;
    Ok(Some(tjmax as f32 - odstup as f32))
}

/// Drží `Global\Access_PCI` po dobu jednoho čtení.
struct PciZamek<'a>(&'a OwnedHandle);

impl<'a> PciZamek<'a> {
    /// `None` = mutex drží někdo jiný déle než PCI_LOCK_MS.
    fn vezmi(h: &'a OwnedHandle) -> Option<PciZamek<'a>> {
        // SAFETY: platný handle mutexu.
        let w = unsafe { WaitForSingleObject(HANDLE(h.as_raw_handle()), PCI_LOCK_MS) };
        // Opuštěný mutex (držitel spadl) je náš stejně jako volný.
        (w == WAIT_OBJECT_0 || w == WAIT_ABANDONED).then_some(PciZamek(h))
    }
}

impl Drop for PciZamek<'_> {
    fn drop(&mut self) {
        // SAFETY: mutex vlastní tohle vlákno (vzal ho `vezmi` ve stejném
        // volání pod zámkem STAV).
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

/// SHA-256 v hexu přes Windows CNG; None jen když CNG selže.
fn sha256_hex(data: &[u8]) -> Option<String> {
    let mut out = [0u8; 32];
    // SAFETY: pseudo-handle algoritmu je statický; buffery mají správné délky.
    let st = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    st.is_ok()
        .then(|| out.iter().map(|b| format!("{b:02x}")).collect())
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
        b"GenuineIntel" if v.eax >= 6 && cpuid(6).eax & (1 << 6) != 0 => Cpu::Intel,
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

    // Jen čtení: každá žádost, kterou tenhle soubor umí složit, volá
    // čtecí funkci na jedné ze tří pevných adres. Kdyby někdo do výčtu
    // přidal cokoli se zápisem, tady to spadne.
    #[test]
    fn zadosti_jsou_jen_cteni() {
        // Match bez zástupného vzoru: nová varianta výčtu tenhle test
        // nepřeloží, dokud ji někdo nedopíše i sem a do VSECHNA.
        let _vycerpano = |c: Cteni| match c {
            Cteni::AmdTctl | Cteni::IntelTjMax | Cteni::IntelPouzdro => (),
        };
        assert_eq!(Cteni::VSECHNA.len(), 3);
        for c in Cteni::VSECHNA {
            let z = zadost(c);
            let konec = z[..FN_NAME_LEN].iter().position(|&b| b == 0).unwrap();
            let jmeno = std::str::from_utf8(&z[..konec]).unwrap();
            assert!(
                jmeno == "ioctl_read_smn" || jmeno == "ioctl_read_msr",
                "{c:?} volá {jmeno}"
            );
            assert!(!jmeno.contains("write"));
            // Za jménem jen nuly a pak právě jeden argument.
            assert!(z[konec..FN_NAME_LEN].iter().all(|&b| b == 0));
            let adresa = u64::from_le_bytes(z[FN_NAME_LEN..].try_into().unwrap());
            assert!([0x0005_9800, 0x1A2, 0x1B1].contains(&adresa));
            assert_eq!(z.len(), FN_NAME_LEN + 8);
        }
    }

    #[test]
    fn cteni_patri_ke_spravnemu_modulu() {
        assert_eq!(Cteni::AmdTctl.modul(), Modul::AmdFamily17);
        assert_eq!(Cteni::IntelTjMax.modul(), Modul::IntelMsr);
        assert_eq!(Cteni::IntelPouzdro.modul(), Modul::IntelMsr);
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

    // Přibalené moduly jsou přesně ty z oficiálního vydání: stejná
    // kontrola otisku, jakou dělá služba před načtením.
    #[test]
    fn moduly_odpovidaji_oficialnimu_otisku() {
        for m in [Modul::AmdFamily17, Modul::IntelMsr] {
            assert_eq!(sha256_hex(m.data()).as_deref(), Some(m.otisk()), "{m:?}");
            assert!(m.overeny());
            let sig = u32::from_le_bytes(m.data()[..4].try_into().unwrap()) as usize;
            assert_eq!(sig, 512, "podpis RSA-4096");
        }
        // A kontrola sama opravdu rozliší jiný soubor.
        assert_ne!(sha256_hex(b"jiny modul").as_deref(), Some(Modul::AmdFamily17.otisk()));
    }

    // Mutex: všem jen čekání a uvolnění, plná práva jen SYSTEM a správci.
    #[test]
    fn dacl_mutexu_neda_vsem_plna_prava() {
        let sddl = String::from_utf16_lossy(unsafe { PCI_MUTEX_SDDL.as_wide() });
        assert!(sddl.contains("(A;;0x100001;;;WD)"));
        assert!(!sddl.contains("0x1F0001;;;WD"));
    }

    // Bez služby (test běží jako uživatel) PawnIO nikdy nic nevrátí:
    // buď chybí, nebo nejsou práva. Hlavně nesmí spadnout.
    #[test]
    fn bez_prav_nic_nepredstira() {
        let t = cpu_temp();
        assert!(t.is_none() || (5.0..=120.0).contains(&t.unwrap()));
        // Dotaz na správce služeb funguje i bez práv a nespadne.
        let _ = installed();
    }
}
