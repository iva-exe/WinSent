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
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// Ikona jako RGBA (šířka, výška, bajty top-down RGBA).
pub struct IconRgba {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Extrahuje ikonu binárky. None = nemá ikonu / nelze načíst.
pub fn extract(path: &str) -> Option<IconRgba> {
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
