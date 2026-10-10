//! Senzory GPU přes D3DKMT — VENDOR-NEUTRÁLNÍ (NVIDIA, AMD, Intel):
//! teplota, ventilátor a příkon v % limitu. Ptá se grafického jádra
//! Windows (gdi32 → dxgkrnl) na totéž, z čeho kreslí Správce úloh;
//! žádná knihovna výrobce, žádný vlastní ovladač.
//!
//! Cena dotazu `ADAPTERPERFDATA` záleží na tom, kdy přijde (RTX 2070
//! SUPER): hned po volání NVML 3–6 µs, protože ovladač naměřené hodnoty
//! chvíli drží; samotný každý druhý tick 3–7 ms CPU, kdy ovladač čte
//! senzory z karty. Kde NVML je, volat ho proto až po něm. Na AMD/Intel
//! (NVML tam není) neměřeno. Výběr adaptéru, strop ventilátoru
//! a velikosti segmentů se zjišťují jen při otevření.
//!
//! Jednotky (ověřeno proti nvidia-smi na RTX 2070 SUPER): `Temperature`
//! v desetinách °C, `Power` v desetinách % limitu příkonu desky,
//! `FanRPM` v otáčkách za minutu. `MemoryBandwidth`, `PCIEBandwidth`
//! a `PowerStateOverride` ovladač nevyplňuje (vždy 0) — nepoužívají se.
//!
//! Minimum je Windows 10 1803 (WDDM 2.4). Na starším systému nebo
//! s ovladačem, který údaje nedává, dotaz vrátí chybu a hodnoty
//! zůstanou None — nikdy vymyšlené.

use std::ffi::c_void;

use windows::Wdk::Graphics::Direct3D::{
    D3DKMTCloseAdapter, D3DKMTEnumAdapters2, D3DKMTQueryAdapterInfo, D3DKMT_ADAPTERADDRESS,
    D3DKMT_ADAPTERINFO, D3DKMT_ADAPTERTYPE, D3DKMT_ADAPTER_PERFDATA, D3DKMT_ADAPTER_PERFDATACAPS,
    D3DKMT_CLOSEADAPTER, D3DKMT_ENUMADAPTERS2, D3DKMT_QUERYADAPTERINFO, D3DKMT_SEGMENTSIZEINFO,
    KMTQAITYPE_ADAPTERADDRESS, KMTQAITYPE_ADAPTERPERFDATA, KMTQAITYPE_ADAPTERPERFDATA_CAPS,
    KMTQAITYPE_ADAPTERTYPE, KMTQAITYPE_GETSEGMENTSIZE, KMTQUERYADAPTERINFOTYPE,
};
use windows::Win32::Foundation::LUID;

/// Bity `D3DKMT_ADAPTERTYPE`.
const RENDER_SUPPORTED: u32 = 1 << 0;
const SOFTWARE_DEVICE: u32 = 1 << 2;

const MB: u64 = 1024 * 1024;

/// Vlastněný handle adaptéru. Každý handle z `D3DKMTEnumAdapters2` se
/// musí zavřít (MS docs) — i ty, které nakonec nepoužijeme. Drop to
/// zaručí na každé cestě, včetně předčasného návratu.
struct Handle(u32);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: handle vrátil D3DKMTEnumAdapters2 a zavírá se právě jednou.
        unsafe {
            let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: self.0 });
        }
    }
}

/// Jeden vzorek senzorů. None = ovladač údaj nehlásí.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct KmtPerf {
    pub temp_c: Option<f32>,
    pub fan_rpm: Option<u32>,
    pub fan_pct: Option<f32>,
    pub power_pct: Option<f32>,
}

/// Otevřený hlavní adaptér. Handle zavře Drop.
pub struct KmtAdapter {
    h: Handle,
    /// LUID ve tvaru, jakým začínají instance PDH čítačů GPU
    /// („luid_0x00000000_0x0000C958") — páruje senzory s enginy a VRAM
    /// téhož adaptéru.
    pub pdh_luid: String,
    pub dedicated_mb: u64,
    /// Strop sdílené paměti (Správce úloh: „Sdílená paměť GPU").
    pub shared_limit_mb: u64,
    /// Max otáčky z CAPS; 0 = ovladač ventilátor nehlásí.
    max_fan_rpm: u32,
}

/// Jeden dotaz `D3DKMTQueryAdapterInfo` do struktury `T`. Err = NTSTATUS.
///
/// SAFETY: `T` musí být struktura, kterou dotaz `typ` vyplňuje, a `h`
/// otevřený handle adaptéru.
unsafe fn dotaz<T>(h: u32, typ: KMTQUERYADAPTERINFOTYPE, out: &mut T) -> Result<(), i32> {
    let mut q = D3DKMT_QUERYADAPTERINFO {
        hAdapter: h,
        Type: typ,
        pPrivateDriverData: out as *mut T as *mut c_void,
        PrivateDriverDataSize: std::mem::size_of::<T>() as u32,
    };
    let st = D3DKMTQueryAdapterInfo(&mut q);
    if st.is_ok() {
        Ok(())
    } else {
        Err(st.0)
    }
}

impl KmtAdapter {
    /// Najde a otevře hlavní GPU. None = žádný vhodný adaptér nebo
    /// D3DKMT nejde.
    ///
    /// Software adaptér (Microsoft Basic Render) a adaptéry bez renderu
    /// (jen displej, NPU) se přeskočí. Přednost má adaptér na PCI
    /// sběrnici `prefer_pci_bus` — ten, který čte NVML, ať teplota
    /// a takt patří témuž GPU jako ventilátor. Jinak vyhraje největší
    /// dedikovaná VRAM (= diskrétní GPU hybridního notebooku).
    pub fn open_main(prefer_pci_bus: Option<u32>) -> Option<KmtAdapter> {
        // SAFETY: dvoufázový výčet; buffer má délku z prvního volání
        // a vrácené handly hned přebírá Handle.
        let mut handles: Vec<(Handle, LUID)> = unsafe {
            let mut e = D3DKMT_ENUMADAPTERS2::default();
            // S NULL vrátí MAXIMÁLNÍ počet (32), ne skutečný.
            if D3DKMTEnumAdapters2(&mut e).is_err() || e.NumAdapters == 0 {
                return None;
            }
            let cap = e.NumAdapters as usize;
            let mut infos = vec![D3DKMT_ADAPTERINFO::default(); cap];
            e.pAdapters = infos.as_mut_ptr();
            if D3DKMTEnumAdapters2(&mut e).is_err() {
                return None;
            }
            infos.truncate((e.NumAdapters as usize).min(cap));
            infos
                .into_iter()
                .map(|i| (Handle(i.hAdapter), i.AdapterLuid))
                .collect()
        };

        // (index, segmenty, shoda PCI sběrnice)
        let mut vybrany: Option<(usize, D3DKMT_SEGMENTSIZEINFO, bool)> = None;
        for (idx, (h, _)) in handles.iter().enumerate() {
            // SAFETY: handle je otevřený, struktury odpovídají dotazům.
            unsafe {
                let mut typ = D3DKMT_ADAPTERTYPE::default();
                if dotaz(h.0, KMTQAITYPE_ADAPTERTYPE, &mut typ).is_err() {
                    continue; // nevíme, jestli není software — radši ne
                }
                let bity = typ.Anonymous.Value;
                if bity & SOFTWARE_DEVICE != 0 || bity & RENDER_SUPPORTED == 0 {
                    continue;
                }
                let mut seg = D3DKMT_SEGMENTSIZEINFO::default();
                let _ = dotaz(h.0, KMTQAITYPE_GETSEGMENTSIZE, &mut seg);
                let bus = prefer_pci_bus.is_some_and(|b| {
                    let mut a = D3DKMT_ADAPTERADDRESS::default();
                    dotaz(h.0, KMTQAITYPE_ADAPTERADDRESS, &mut a).is_ok() && a.BusNumber == b
                });
                // Při remíze zůstává první v pořadí.
                let lepsi = vybrany.is_none_or(|(_, s, b)| {
                    (bus, seg.DedicatedVideoMemorySize) > (b, s.DedicatedVideoMemorySize)
                });
                if lepsi {
                    vybrany = Some((idx, seg, bus));
                }
            }
        }
        let (idx, seg, _) = vybrany?;
        let (h, luid) = handles.swap_remove(idx);
        drop(handles); // ostatní adaptéry zavřít hned, ne až s námi

        // Strop otáček. Bez něj (starší systém, notebook) zůstává 0.
        let mut caps = D3DKMT_ADAPTER_PERFDATACAPS::default();
        // SAFETY: handle je otevřený, struktura odpovídá dotazu.
        let max_fan_rpm = match unsafe { dotaz(h.0, KMTQAITYPE_ADAPTERPERFDATA_CAPS, &mut caps) } {
            Ok(()) => caps.MaxFanRPM,
            Err(_) => 0,
        };

        Some(KmtAdapter {
            h,
            pdh_luid: pdh_luid(luid),
            dedicated_mb: seg.DedicatedVideoMemorySize / MB,
            shared_limit_mb: seg.SharedSystemMemorySize / MB,
            max_fan_rpm,
        })
    }

    /// Aktuální senzory. Err = NTSTATUS (starý systém, ovladač údaje
    /// nedává, adaptér zmizel po TDR nebo aktualizaci ovladače).
    pub fn sample(&self) -> Result<KmtPerf, i32> {
        // PhysicalAdapterIndex 0 — linkované adaptéry (SLI) mají senzory
        // první karty.
        let mut pd = D3DKMT_ADAPTER_PERFDATA::default();
        // SAFETY: handle drží self otevřený, struktura odpovídá dotazu.
        unsafe { dotaz(self.h.0, KMTQAITYPE_ADAPTERPERFDATA, &mut pd)? };
        Ok(prevod(&pd, self.max_fan_rpm))
    }
}

/// LUID → předpona instancí PDH čítačů GPU (velká hex, jako je PDH vrací).
fn pdh_luid(luid: LUID) -> String {
    format!("luid_0x{:08X}_0x{:08X}", luid.HighPart as u32, luid.LowPart)
}

/// Syrové hodnoty ovladače → jednotky UI.
fn prevod(pd: &D3DKMT_ADAPTER_PERFDATA, max_fan_rpm: u32) -> KmtPerf {
    // Desetiny °C → celé °C jako u NVML (jinak „35.5" v kartách
    // a reportu). Useknutí, ne zaokrouhlení: nvidia-smi u 375 a 385
    // ukázala 37 a 38. Mez 5–120 °C stejná jako u NVML: nula je
    // nevyplněná hodnota, ne teplota.
    let t = pd.Temperature / 10;
    KmtPerf {
        temp_c: (5..=120).contains(&t).then_some(t as f32),
        // Bez stropu a s nulou ovladač ventilátor nehlásí — „0 ot/min"
        // by tvrdilo, že stojí. Se stropem je nula skutečný zero-fan
        // režim a nenulové otáčky jsou měření i bez stropu.
        fan_rpm: (max_fan_rpm > 0 || pd.FanRPM > 0).then_some(pd.FanRPM),
        fan_pct: (max_fan_rpm > 0)
            .then(|| (pd.FanRPM as f32 * 100.0 / max_fan_rpm as f32).min(100.0)),
        // Desetiny % limitu. Nula = údaj chybí (zapnutá karta nikdy
        // nebere nic); nad 200 % je to nevyplněné smetí, ne boost.
        power_pct: (1..=2000)
            .contains(&pd.Power)
            .then(|| pd.Power as f32 / 10.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pd(temp: u32, fan: u32, power: u32) -> D3DKMT_ADAPTER_PERFDATA {
        D3DKMT_ADAPTER_PERFDATA {
            Temperature: temp,
            FanRPM: fan,
            Power: power,
            ..Default::default()
        }
    }

    // Hodnoty z RTX 2070 SUPER ve stejnou chvíli jako nvidia-smi
    // (35 °C, 25 %, 12,3 % z 215 W).
    #[test]
    fn jednotky_jako_nvidia_smi() {
        let p = prevod(&pd(355, 849, 123), 3400);
        assert_eq!(p.temp_c, Some(35.0));
        assert_eq!(p.fan_rpm, Some(849));
        assert!((p.fan_pct.unwrap() - 24.97).abs() < 0.01);
        assert_eq!(p.power_pct, Some(12.3));
    }

    // Notebook bez senzoru ventilátoru a ovladač bez údajů: nic se
    // nevymýšlí.
    #[test]
    fn chybejici_udaje_jsou_none() {
        let p = prevod(&pd(0, 0, 0), 0);
        assert_eq!(p, KmtPerf::default());
        // 255 °C z nepřečteného bajtu a 300 % limitu jsou smetí.
        let p = prevod(&pd(2550, 0, 3000), 0);
        assert_eq!(p.temp_c, None);
        assert_eq!(p.power_pct, None);
    }

    // Zero-fan režim se stropem je skutečná nula; otáčky bez stropu
    // jsou měření, jen bez procent.
    #[test]
    fn ventilator() {
        let p = prevod(&pd(400, 0, 50), 3400);
        assert_eq!((p.fan_rpm, p.fan_pct), (Some(0), Some(0.0)));
        let p = prevod(&pd(400, 1200, 50), 0);
        assert_eq!((p.fan_rpm, p.fan_pct), (Some(1200), None));
    }

    #[test]
    fn luid_jako_pdh() {
        let l = LUID {
            LowPart: 0xC958,
            HighPart: 0,
        };
        assert_eq!(pdh_luid(l), "luid_0x00000000_0x0000C958");
    }
}
