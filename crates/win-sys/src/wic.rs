//! Dekódování obrázků přes WIC (Windows Imaging Component) → RGBA.
//! Pro ikony aplikací z PNG assetů (MSIX loga) — .exe ikony řeší
//! icon.rs, tohle doplňuje zbytek. Vyžaduje COM inicializované vlákno.

use windows::core::HSTRING;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

use crate::icon::IconRgba;

/// Inicializuje COM pro aktuální vlákno (nutné před `decode`).
/// Bezpečné volat opakovaně; výsledek se ignoruje (S_FALSE = už je).
pub fn init_com_for_thread() {
    // SAFETY: standardní COM init, jednou na vlákno.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
}

/// Dekóduje obrázek (PNG/JPG/BMP…) do RGBA. None = nejde dekódovat.
pub fn decode(path: &str) -> Option<IconRgba> {
    // SAFETY: standardní WIC sekvence; COM objekty se uvolní přes Drop.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let dec = factory
            .CreateDecoderFromFilename(
                &HSTRING::from(path),
                None,
                windows::Win32::Foundation::GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || w > 1024 || h > 1024 {
            return None;
        }
        let conv = factory.CreateFormatConverter().ok()?;
        conv.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .ok()?;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut buf).ok()?;
        Some(IconRgba { w, h, rgba: buf })
    }
}
