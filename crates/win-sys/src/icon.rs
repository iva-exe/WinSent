//! Extrakce ikony aplikace z .exe (SPEC kap. 9 — vizuál). Vrací syrové
//! RGBA pixely; PNG kódování neřešíme, UI si je vykreslí na canvas.
//!
//! POMALÉ (GDI + shell) — volá se výhradně z background vlákna identity,
//! jednou na aplikaci, výsledek se cachuje.

use std::os::windows::ffi::OsStrExt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::UI::Shell::{
    ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// Ikona jako RGBA (šířka, výška, bajty top-down RGBA).
pub struct IconRgba {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Extrahuje ikonu binárky. None = nemá ikonu / nelze načíst.
pub fn extract(path: &str) -> Option<IconRgba> {
    // PŘEDNOSTNĚ z PE resource: služba běží jako SYSTEM v session 0,
    // kde shell (SHGetFileInfo) vrací GENERICKOU ikonu — proto dřív
    // většina aplikací dostala default „okýnko". Čtení resource na
    // shellu ani na session nezávisí.
    if let Some(i) = extract_pe(path) {
        return Some(i);
    }
    let wide: Vec<u16> = std::path::Path::new(path)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: shell/GDI sekvence; HICON i HBITMAP se vždy uvolní.
    unsafe {
        let mut info = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let result = icon_to_rgba(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        result
    }
}

/// Ikona přímo z PE resource binárky (RT_GROUP_ICON → nejlepší
/// velikost → RT_ICON → HICON). Funguje i v session 0 (služba),
/// protože nesahá na shell ani na ikonové cache.
pub fn extract_pe(path: &str) -> Option<IconRgba> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{
        EnumResourceNamesW, FindResourceW, LoadLibraryExW, LoadResource, LockResource,
        SizeofResource, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, LookupIconIdFromDirectoryEx, LR_DEFAULTCOLOR, RT_GROUP_ICON,
        RT_ICON,
    };

    let wide: Vec<u16> = std::path::Path::new(path)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Callback pro EnumResourceNamesW — vezme PRVNÍ group icon
    // (u .exe je to ikona aplikace) a uloží její id do kontextu.
    unsafe extern "system" fn first_name(
        _module: windows::Win32::Foundation::HMODULE,
        _ty: PCWSTR,
        name: PCWSTR,
        param: isize,
    ) -> windows::core::BOOL {
        let slot = param as *mut usize;
        if !slot.is_null() {
            *slot = name.0 as usize;
        }
        // false = přestat enumerovat (máme první).
        false.into()
    }

    // SAFETY: modul se načítá jen jako data (nespouští se žádný kód),
    // handle se vždy uvolní; resource ukazatele žijí po dobu modulu.
    unsafe {
        let module = LoadLibraryExW(
            PCWSTR(wide.as_ptr()),
            None,
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
        .ok()?;

        let mut group_name: usize = 0;
        let _ = EnumResourceNamesW(
            Some(module),
            RT_GROUP_ICON,
            Some(first_name),
            &mut group_name as *mut usize as isize,
        );
        if group_name == 0 {
            let _ = FreeLibrary(module);
            return None;
        }

        let result = (|| {
            // Group icon direktář → id nejlepší varianty pro 32×32.
            let group = FindResourceW(
                Some(module),
                PCWSTR(group_name as *const u16),
                RT_GROUP_ICON,
            );
            if group.is_invalid() {
                return None;
            }
            let g_res = LoadResource(Some(module), group).ok()?;
            let g_ptr = LockResource(g_res) as *const u8;
            if g_ptr.is_null() {
                return None;
            }
            let g_len = SizeofResource(Some(module), group) as usize;
            let dir = std::slice::from_raw_parts(g_ptr, g_len);
            let id = LookupIconIdFromDirectoryEx(dir.as_ptr(), true, 32, 32, LR_DEFAULTCOLOR);
            if id == 0 {
                return None;
            }

            // Konkrétní ikona podle id (id je číselné → PCWSTR z čísla).
            let icon = FindResourceW(Some(module), PCWSTR(id as u16 as *const u16), RT_ICON);
            if icon.is_invalid() {
                return None;
            }
            let i_res = LoadResource(Some(module), icon).ok()?;
            let i_ptr = LockResource(i_res) as *const u8;
            if i_ptr.is_null() {
                return None;
            }
            let i_len = SizeofResource(Some(module), icon) as usize;
            let bytes = std::slice::from_raw_parts(i_ptr, i_len);
            // 0x00030000 = verze formátu ikony (dokumentovaná konstanta).
            let hicon =
                CreateIconFromResourceEx(bytes, true, 0x0003_0000, 32, 32, LR_DEFAULTCOLOR).ok()?;
            let rgba = icon_to_rgba(hicon);
            let _ = DestroyIcon(hicon);
            rgba
        })();

        let _ = FreeLibrary(module);
        result
    }
}

/// Extrahuje ikonu podle DisplayIcon spec z registru: `cesta[,index]`,
/// s uvozovkami a env proměnnými (`%SystemRoot%\…`). Fallback zdroj,
/// když .exe procesu vlastní ikonu nemá (SPEC 5.2 — DisplayIcon).
pub fn extract_spec(spec: &str) -> Option<IconRgba> {
    // "cesta,index" — index oddělený poslední čárkou (cesta může mít čárky
    // jen výjimečně; DisplayIcon formát je takto definovaný).
    let (raw_path, index) = match spec.rsplit_once(',') {
        Some((p, idx)) => match idx.trim().parse::<i32>() {
            Ok(i) => (p, i),
            Err(_) => (spec, 0),
        },
        None => (spec, 0),
    };
    let path = expand_env(raw_path.trim().trim_matches('"'));
    if path.is_empty() {
        return None;
    }

    // Bez indexu zkusit shell (umí .ico, .exe, asociace) — pak ExtractIconEx.
    if index == 0 {
        if let Some(icon) = extract(&path) {
            return Some(icon);
        }
    }
    let wide: Vec<u16> = std::path::Path::new(&path)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: HICON z ExtractIconEx vždy uvolníme.
    unsafe {
        let mut hicon = HICON::default();
        let got = ExtractIconExW(PCWSTR(wide.as_ptr()), index, Some(&mut hicon), None, 1);
        if got == 0 || hicon.is_invalid() {
            return None;
        }
        let result = icon_to_rgba(hicon);
        let _ = DestroyIcon(hicon);
        result
    }
}

/// Expanze `%NAZEV%` proměnných prostředí v cestě.
fn expand_env(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(val) => out.push_str(&val),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// SAFETY: `hicon` je platná ikona; HBITMAPy z GetIconInfo uvolníme.
unsafe fn icon_to_rgba(hicon: HICON) -> Option<IconRgba> {
    let mut ii = ICONINFO::default();
    GetIconInfo(hicon, &mut ii).ok()?;

    // Rozměry z barevného bitmapu.
    let mut bmp = BITMAP::default();
    let got = GetObjectW(
        HGDIOBJ(ii.hbmColor.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bmp as *mut _ as *mut core::ffi::c_void),
    );
    if got == 0 {
        cleanup(&ii);
        return None;
    }
    let w = bmp.bmWidth.unsigned_abs();
    let h = bmp.bmHeight.unsigned_abs();
    if w == 0 || h == 0 || w > 512 || h > 512 {
        cleanup(&ii);
        return None;
    }

    // GetDIBits: top-down 32bit BGRA.
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // záporné = top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(Some(HWND::default()));
    let lines = GetDIBits(
        dc,
        ii.hbmColor,
        0,
        h,
        Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
        &mut bi,
        DIB_RGB_COLORS,
    );
    ReleaseDC(Some(HWND::default()), dc);
    cleanup(&ii);
    if lines == 0 {
        return None;
    }

    // BGRA → RGBA. Když je alfa všude nulová (staré ikony), zneprůhledni.
    let has_alpha = buf.chunks_exact(4).any(|px| px[3] != 0);
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2); // B <-> R
        if !has_alpha {
            px[3] = 255;
        }
    }

    Some(IconRgba { w, h, rgba: buf })
}

/// SAFETY: uvolní HBITMAPy z ICONINFO.
unsafe fn cleanup(ii: &ICONINFO) {
    if !ii.hbmColor.is_invalid() {
        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
    }
    if !ii.hbmMask.is_invalid() {
        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
    }
}
