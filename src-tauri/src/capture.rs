//! Захват виртуального экрана (`BitBlt`), кодирование PNG через WIC,
//! запись изображений (`PNG` + `CF_DIB`) и текста в буфер обмена, сохранение в `Pictures\SnapShot`.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    IWICImagingFactory, WICBitmapEncoderNoCache, WICConvertBitmapSource,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;

#[derive(Serialize, Clone, Debug)]
pub struct ScreenCapture {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub image: String,
}

pub struct ComGuard(bool);
impl ComGuard {
    pub fn init() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self(hr.is_ok() && hr != RPC_E_CHANGED_MODE)
    }
}
impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn png_fmt() -> u32 {
    static FMT: OnceLock<u32> = OnceLock::new();
    *FMT.get_or_init(|| unsafe { RegisterClipboardFormatW(w!("PNG")) })
}

unsafe fn bgra_to_png(bgra: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let bmp = factory
        .CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppBGRA, w * 4, bgra)
        .ok()?;
    let out_stream = CreateStreamOnHGlobal(HGLOBAL::default(), true).ok()?;
    let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()?;
    encoder.Initialize(&out_stream, WICBitmapEncoderNoCache).ok()?;
    let mut frame = None;
    let mut props = None;
    encoder.CreateNewFrame(&mut frame, &mut props).ok()?;
    let frame = frame?;
    frame.Initialize(props.as_ref()).ok()?;
    frame.SetSize(w, h).ok()?;
    frame.WriteSource(&bmp, std::ptr::null()).ok()?;
    frame.Commit().ok()?;
    encoder.Commit().ok()?;

    let hg = GetHGlobalFromStream(&out_stream).ok()?;
    let size = GlobalSize(hg);
    if size == 0 {
        return None;
    }
    let ptr = GlobalLock(hg) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let out = std::slice::from_raw_parts(ptr, size).to_vec();
    let _ = GlobalUnlock(hg);
    Some(out)
}

/// Захватывает весь виртуальный экран (все мониторы) в 1:1 DPI.
pub fn capture_virtual_screen() -> Option<ScreenCapture> {
    let _com = ComGuard::init();
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w <= 0 || h <= 0 {
            return None;
        }
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return None;
        }
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.is_invalid() {
            ReleaseDC(None, screen_dc);
            return None;
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(hbmp) = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);
            return None;
        };
        if bits.is_null() {
            let _ = DeleteObject(hbmp.into());
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);
            return None;
        }
        let old = SelectObject(mem_dc, hbmp.into());
        let ok = BitBlt(mem_dc, 0, 0, w, h, Some(screen_dc), x, y, SRCCOPY | CAPTUREBLT).is_ok();
        let png = if ok {
            let slice = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
            bgra_to_png(slice, w as u32, h as u32)
        } else {
            None
        };
        SelectObject(mem_dc, old);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);

        let png = png?;
        let image = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        Some(ScreenCapture { x, y, w, h, image })
    }
}

pub fn decode_data_url_png(data_url: &str) -> Option<Vec<u8>> {
    let b64 = data_url.strip_prefix("data:image/png;base64,")?;
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

fn png_to_dib(png: &[u8]) -> Option<Vec<u8>> {
    let _com = ComGuard::init();
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let in_stream = factory.CreateStream().ok()?;
        in_stream.InitializeFromMemory(png).ok()?;
        let decoder = factory
            .CreateDecoderFromStream(&in_stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        let bgra = WICConvertBitmapSource(&GUID_WICPixelFormat32bppBGRA, &frame).ok()?;
        let stride = w.checked_mul(4)?;
        let img_bytes = stride.checked_mul(h)? as usize;
        let mut pixels = vec![0u8; img_bytes];
        bgra.CopyPixels(std::ptr::null(), stride, &mut pixels).ok()?;

        let mut dib = Vec::with_capacity(40 + img_bytes);
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(w as i32).to_le_bytes());
        dib.extend_from_slice(&(-(h as i32)).to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&(img_bytes as u32).to_le_bytes());
        dib.extend_from_slice(&[0u8; 16]);
        dib.extend_from_slice(&pixels);
        Some(dib)
    }
}

unsafe fn open_clipboard_retry() -> bool {
    for _ in 0..6 {
        if OpenClipboard(None).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

unsafe fn set_clip_bytes(fmt: u32, bytes: &[u8]) -> bool {
    let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return false };
    let dst = GlobalLock(hmem) as *mut u8;
    if dst.is_null() {
        let _ = GlobalFree(Some(hmem));
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    let _ = GlobalUnlock(hmem);
    if SetClipboardData(fmt, Some(HANDLE(hmem.0))).is_err() {
        let _ = GlobalFree(Some(hmem));
        return false;
    }
    true
}

pub fn copy_png_to_clipboard(png: &[u8]) -> bool {
    if png.is_empty() {
        return false;
    }
    let dib = png_to_dib(png);
    unsafe {
        if !open_clipboard_retry() {
            return false;
        }
        let _ = EmptyClipboard();
        let mut ok = false;
        let pfmt = png_fmt();
        if pfmt != 0 {
            ok |= set_clip_bytes(pfmt, png);
        }
        if let Some(d) = dib.as_deref() {
            ok |= set_clip_bytes(CF_DIB, d);
        }
        let _ = CloseClipboard();
        ok
    }
}

pub fn copy_text_to_clipboard(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    unsafe {
        if !open_clipboard_retry() {
            return false;
        }
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        let _ = EmptyClipboard();
        let ok = set_clip_bytes(CF_UNICODETEXT, bytes);
        let _ = CloseClipboard();
        ok
    }
}

pub fn save_dir() -> PathBuf {
    let base = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Pictures").join("SnapShot")
}

pub fn save_png_file(png: &[u8]) -> Option<String> {
    let dir = save_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let t = unsafe { GetLocalTime() };
    let name = format!(
        "SnapShot-{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.png",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    let path = dir.join(name);
    std::fs::write(&path, png).ok()?;
    Some(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_valid_data_url_and_rejects_other() {
        let encoded = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"\x89PNGtest")
        );
        assert_eq!(decode_data_url_png(&encoded).as_deref(), Some(&b"\x89PNGtest"[..]));
        assert!(decode_data_url_png("data:text/plain;base64,abc").is_none());
    }
}
