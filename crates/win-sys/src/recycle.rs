//! Mazání DO KOŠE (SPEC kap. 18.2) — jediná povolená cesta odstranění.
//!
//! `IFileOperation` s `FOF_ALLOWUNDO` = soubor jde vrátit z koše,
//! takže i „nevratná" akce má cestu zpět. Force delete a zakázaný
//! vzor `DUPLICATE_CLOSE_SOURCE` se v projektu NEIMPLEMENTUJÍ.
//!
//! VOLÁ SE V PROCESU UŽIVATELE (UI), NIKDY VE SLUŽBĚ. Koš patří účtu,
//! který maže: služba jako SYSTEM házela soubory do `$Recycle.Bin\S-1-5-18`,
//! kam uživatel nevidí — „jde vrátit z koše" pak nebyla pravda. Pod
//! tokenem uživatele navíc platí jeho práva k souborům.
//!
//! Proč tyhle příznaky:
//! - `FOF_ALLOWUNDO` + `FOFX_RECYCLEONDELETE` — koš, ne trvalé smazání;
//! - `FOF_NOCONFIRMATION` — potvrzení už proběhlo v UI Winsentu;
//! - `FOF_WANTNUKEWARNING` — když se položka do koše NEVEJDE (větší než
//!   koš, koš vypnutý), Windows se zeptá, místo aby ji potichu smazaly
//!   natrvalo. `FOF_NOCONFIRMATION` by jinak odpověděl „Ano všem";
//! - `FOF_SILENT` — bez okna s průběhem;
//! - bez `FOF_NOERRORUI` — chybu (soubor drží jiný program) Windows
//!   ukážou uživateli, místo aby zapadla.
//!
//! Proč `IFileOperation`, a ne `SHFileOperationW`: když uživatel na
//! dotaz Windows „nevejde se do koše — smazat natrvalo?" odpoví Ano,
//! `SHFileOperationW` vrátí 0 a `fAnyOperationsAborted` zůstane false —
//! stejně jako u přesunu do koše. Winsent pak hlásil „přesunuto do koše"
//! a historie „obnovit z koše" u souboru, který je pryč natrvalo.
//! `IFileOperation` to říká po položkách (`PostDeleteItem`: položka
//! v koši, nebo NULL = smazáno natrvalo).

use std::cell::Cell;
use std::rc::Rc;

use windows::core::{implement, Ref, HRESULT, PCWSTR};
use windows::Win32::Foundation::{HWND, S_OK};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IBindCtx, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl,
    IShellItem, SHCreateItemFromParsingName, COPYENGINE_E_USER_CANCELLED,
    COPYENGINE_S_DONT_PROCESS_CHILDREN, FILEOPERATION_FLAGS,
    FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT, FOF_WANTNUKEWARNING,
};

/// Jak přesun do koše dopadl (když nevrátil chybu).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recycled {
    /// Uživatel operaci zrušil (typicky na dotaz Windows, že se položka
    /// do koše nevejde a smazala by se natrvalo). Část položek mohla
    /// odejít do koše už předtím — co zbylo, ověří volající.
    pub aborted: bool,
    /// Položky, které podle Windows skončily v koši (jdou obnovit).
    pub v_kosi: usize,
    /// Položky smazané NATRVALO: do koše se nevešly a uživatel smazání
    /// na dotaz Windows potvrdil. Z koše je obnovit nejde.
    pub natrvalo: usize,
}

/// HRESULT zrušené operace: ERROR_CANCELLED a COPYENGINE_E_USER_CANCELLED.
const ZRUSENO: [HRESULT; 2] = [HRESULT(0x8007_04C7_u32 as i32), COPYENGINE_E_USER_CANCELLED];

/// `hrDelete` v `PostDeleteItem`, když položka opravdu zmizela: S_OK,
/// nebo COPYENGINE_S_DONT_PROCESS_CHILDREN — ten Windows 10 hlásí
/// u každého odstraněného souboru, do koše i natrvalo (ověřeno testy
/// níž). Ostatní COPYENGINE_S_* (přeskočeno, už hotovo, odloženo…) jsou
/// formálně úspěch, ale položku neodstranily — a jejich NULL by se
/// spletlo se smazáním natrvalo.
const ODSTRANENO: [HRESULT; 2] = [S_OK, COPYENGINE_S_DONT_PROCESS_CHILDREN];

/// Přesune soubory do koše přihlášeného uživatele.
///
/// `owner` = okno, ke kterému se váže případný dotaz Windows (HWND jako
/// číslo); `None` = bez vlastníka. `paths` musí být absolutní, bez znaku
/// NUL a musí existovat — chybějící položka operaci zastaví dřív, než se
/// čehokoli dotkne.
///
/// Blokuje po dobu přesunu (a případného dotazu Windows). Vlákno si COM
/// inicializuje samo jako STA (`IFileOperation` v MTA nefunguje); vlákno
/// už inicializované jako MTA vrátí chybu.
pub fn to_recycle_bin(paths: &[String], owner: Option<isize>) -> Result<Recycled, crate::Error> {
    proved(
        paths,
        owner,
        FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_SILENT,
    )
}

/// Počty z posluchače — sdílené mezi posluchačem a voláním (jedno STA
/// vlákno, proto `Rc<Cell>`).
#[derive(Default)]
struct Pocty {
    v_kosi: Cell<usize>,
    natrvalo: Cell<usize>,
}

/// Posluchač `IFileOperation`: z `PostDeleteItem` počítá, co skončilo
/// v koši a co bylo smazáno natrvalo. Ostatní události nepotřebujeme.
#[implement(IFileOperationProgressSink)]
struct Sledovani(Rc<Pocty>);

impl IFileOperationProgressSink_Impl for Sledovani_Impl {
    fn PostDeleteItem(
        &self,
        _flags: u32,
        _item: Ref<'_, IShellItem>,
        hr: HRESULT,
        v_kosi: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        if ODSTRANENO.contains(&hr) {
            let c = if v_kosi.is_null() {
                &self.0.natrvalo
            } else {
                &self.0.v_kosi
            };
            c.set(c.get() + 1);
        }
        Ok(())
    }
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _hr: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreDeleteItem(&self, _: u32, _: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

/// COM v STA po dobu života. Vzniká jen z úspěšného `CoInitializeEx`
/// (S_OK i S_FALSE — obojí se páruje s `CoUninitialize`).
struct ComSta;

impl ComSta {
    fn init() -> Result<ComSta, crate::Error> {
        // SAFETY: párové s CoUninitialize v Drop, na témže vlákně.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_err() {
            // RPC_E_CHANGED_MODE: vlákno je MTA a IFileOperation v něm
            // nefunguje — volající si má pustit vlastní vlákno.
            return Err(chyba("CoInitializeEx(STA)", hr));
        }
        Ok(ComSta)
    }
}

impl Drop for ComSta {
    fn drop(&mut self) {
        // SAFETY: k úspěšnému CoInitializeEx v init, na témže vlákně
        // (ComSta žije jen uvnitř `proved`, nikam se nepředává).
        unsafe { CoUninitialize() };
    }
}

fn chyba(call: &'static str, hr: HRESULT) -> crate::Error {
    crate::Error::Win32 { call, code: hr.0 }
}

/// Smaže `paths` jedním `IFileOperation` s příznaky `flags`. Veřejně jen
/// přes `to_recycle_bin` (pevné příznaky); testy sem pouštějí i mazání
/// bez koše, aby ověřily, že posluchač pozná smazání natrvalo.
fn proved(
    paths: &[String],
    owner: Option<isize>,
    flags: FILEOPERATION_FLAGS,
) -> Result<Recycled, crate::Error> {
    if paths.is_empty() {
        return Ok(Recycled::default());
    }
    if paths.iter().any(|p| p.is_empty() || p.contains('\0')) {
        return Err(crate::Error::Win32 {
            call: "IFileOperation(neplatná cesta)",
            code: -1,
        });
    }
    let _com = ComSta::init()?;
    // SAFETY: COM je na vlákně inicializovaný (STA) po celou dobu; každé
    // rozhraní žije, dokud se používá; řetězce cest jsou NUL-terminované
    // a žijí přes volání, které je čte.
    unsafe {
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)
            .map_err(|e| chyba("CoCreateInstance(FileOperation)", e.code()))?;
        op.SetOperationFlags(flags)
            .map_err(|e| chyba("IFileOperation::SetOperationFlags", e.code()))?;
        if let Some(h) = owner {
            op.SetOwnerWindow(HWND(h as *mut core::ffi::c_void))
                .map_err(|e| chyba("IFileOperation::SetOwnerWindow", e.code()))?;
        }
        for p in paths {
            let w: Vec<u16> = p.encode_utf16().chain(Some(0)).collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None::<&IBindCtx>)
                .map_err(|e| chyba("SHCreateItemFromParsingName", e.code()))?;
            op.DeleteItem(&item, None::<&IFileOperationProgressSink>)
                .map_err(|e| chyba("IFileOperation::DeleteItem", e.code()))?;
        }
        let pocty = Rc::new(Pocty::default());
        let sink: IFileOperationProgressSink = Sledovani(pocty.clone()).into();
        let cookie = op
            .Advise(&sink)
            .map_err(|e| chyba("IFileOperation::Advise", e.code()))?;
        let vysledek = op.PerformOperations();
        let _ = op.Unadvise(cookie);
        let aborted = op.GetAnyOperationsAborted().is_ok_and(|b| b.as_bool())
            || vysledek.as_ref().is_err_and(|e| ZRUSENO.contains(&e.code()));
        let r = Recycled {
            aborted,
            v_kosi: pocty.v_kosi.get(),
            natrvalo: pocty.natrvalo.get(),
        };
        match vysledek {
            // Selhání, které nic neodstranilo a nebylo zrušením → chyba.
            // Jinak počty: co zůstalo na disku, dopočítá volající.
            Err(e) if !aborted && r.v_kosi + r.natrvalo == 0 => {
                Err(chyba("IFileOperation::PerformOperations", e.code()))
            }
            _ => Ok(r),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Je někde zapnuté „mazat hned, bez koše"? Pak by FOF_WANTNUKEWARNING
    /// ukázal dialog a test by na ploše čekal na kliknutí — to nesmí.
    fn kos_nekde_vypnuty() -> bool {
        use crate::registry::{enum_subkeys, read_u64, HKEY_CURRENT_USER};
        const BB: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket";
        if read_u64(HKEY_CURRENT_USER, BB, "NukeOnDelete") == Some(1) {
            return true;
        }
        let vol = format!(r"{BB}\Volume");
        enum_subkeys(HKEY_CURRENT_USER, &vol)
            .iter()
            .any(|g| read_u64(HKEY_CURRENT_USER, &format!(r"{vol}\{g}"), "NukeOnDelete") == Some(1))
    }

    /// Původní cesty položek v koších, které tenhle účet smí číst
    /// (`<disk>:\$Recycle.Bin\<SID>\$I…`), spolu se jménem složky (SID).
    ///
    /// Formát `$I` (Vista+): verze (i64), velikost (i64), čas smazání
    /// (i64), pak u verze 2 délka cesty ve znacích (u32) a cesta UTF-16;
    /// verze 1 má cestu pevně 260 znaků.
    fn polozky_kose(disk: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let Ok(sidy) = std::fs::read_dir(format!(r"{disk}\$Recycle.Bin")) else {
            return out;
        };
        for sid in sidy.flatten() {
            let sid_jm = sid.file_name().to_string_lossy().into_owned();
            // Cizí koše (SYSTEM, jiní uživatelé) číst nejde — a nemáme.
            let Ok(rd) = std::fs::read_dir(sid.path()) else {
                continue;
            };
            for e in rd.flatten() {
                if !e.file_name().to_string_lossy().starts_with("$I") {
                    continue;
                }
                let Ok(b) = std::fs::read(e.path()) else {
                    continue;
                };
                if b.len() < 28 {
                    continue;
                }
                let verze = i64::from_le_bytes(b[0..8].try_into().unwrap_or_default());
                let (start, znaku) = if verze >= 2 {
                    (
                        28,
                        u32::from_le_bytes(b[24..28].try_into().unwrap_or_default()) as usize,
                    )
                } else {
                    (24, 260)
                };
                let konec = (start + znaku * 2).min(b.len());
                let w: Vec<u16> = b[start..konec]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&c| c != 0)
                    .collect();
                out.push((sid_jm.clone(), String::from_utf16_lossy(&w)));
            }
        }
        out
    }

    // Soubor smazaný v kontextu uživatele (tak, jak to dělá UI) musí
    // skončit v JEHO koši — ne natrvalo pryč a ne v koši SYSTEMu.
    // Ruční test: každé spuštění nechá v Koši vlastníka malý soubor,
    // proto neběží s každým `cargo test` (pustit: `-- --ignored`).
    #[test]
    #[ignore = "nechává soubor v Koši uživatele"]
    fn soubor_skonci_v_kosi_uzivatele() {
        if kos_nekde_vypnuty() {
            return;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let f = std::env::temp_dir().join(format!(
            "winsent-recycle-test-{}-{nanos}.tmp",
            std::process::id()
        ));
        std::fs::write(&f, b"winsent test kose").expect("zapsat testovací soubor");
        let p = f.to_string_lossy().into_owned();

        let r = to_recycle_bin(std::slice::from_ref(&p), None).expect("přesun do koše");
        assert!(!r.aborted, "nic se nemělo rušit");
        assert!(!f.exists(), "soubor po přesunu pořád existuje");
        assert_eq!((r.v_kosi, r.natrvalo), (1, 0), "posluchač má hlásit koš");

        let disk = &p[..2];
        let nalez = polozky_kose(disk)
            .into_iter()
            .find(|(_, puvodni)| puvodni.eq_ignore_ascii_case(&p));
        let Some((sid, _)) = nalez else {
            panic!("{p} v koši uživatele není — smazal se natrvalo?");
        };
        assert!(
            sid.starts_with("S-1-5-21-"),
            "soubor skončil v koši účtu {sid}, ne uživatele"
        );
    }

    // Posluchač pozná smazání NATRVALO (tak dopadne položka, u které
    // uživatel na dotaz Windows „nevejde se do koše" odpoví Ano). Dotaz
    // se v testu ukázat nesmí, proto vlastní testovací soubor mažeme
    // rovnou bez koše a bez jakéhokoli UI.
    #[test]
    fn posluchac_pozna_smazani_natrvalo() {
        use windows::Win32::UI::Shell::FOF_NOERRORUI;
        let f = std::env::temp_dir().join(format!(
            "winsent-nuke-test-{}.tmp",
            std::process::id()
        ));
        std::fs::write(&f, b"x").expect("zapsat testovací soubor");
        let p = f.to_string_lossy().into_owned();
        let r = proved(
            std::slice::from_ref(&p),
            None,
            FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
        );
        let zbyl = f.exists();
        let _ = std::fs::remove_file(&f);
        let r = r.expect("smazání");
        assert!(!zbyl, "soubor po smazání pořád existuje");
        assert_eq!((r.v_kosi, r.natrvalo, r.aborted), (0, 1, false));
    }

    // Chybějící položka zastaví operaci dřív, než se čehokoli dotkne.
    #[test]
    fn chybejici_polozka_nic_nesmaze() {
        let f = std::env::temp_dir().join(format!(
            "winsent-recycle-zustane-{}.tmp",
            std::process::id()
        ));
        std::fs::write(&f, b"x").expect("zapsat testovací soubor");
        let cesty = [
            f.to_string_lossy().into_owned(),
            std::env::temp_dir()
                .join("winsent-tohle-neexistuje-7f3a.tmp")
                .to_string_lossy()
                .into_owned(),
        ];
        let r = to_recycle_bin(&cesty, None);
        let zustal = f.exists();
        let _ = std::fs::remove_file(&f);
        assert!(r.is_err(), "{r:?}");
        assert!(zustal, "existující soubor se nesměl ani přesunout");
    }

    #[test]
    fn nul_v_ceste_se_odmita() {
        let r = to_recycle_bin(&["C:\\a.txt\0C:\\Windows".to_string()], None);
        assert!(r.is_err(), "NUL by z jedné cesty udělal dvě");
        assert_eq!(to_recycle_bin(&[], None).ok(), Some(Recycled::default()));
    }
}
