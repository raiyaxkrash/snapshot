//! Глобальные горячие клавиши SnapShot:
//! - `PrintScreen` — скриншот области + редактор / Beautify / Blur
//! - `Ctrl+Shift+O` — мгновенный OCR текста с области экрана

use std::sync::OnceLock;

use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
    VK_SNAPSHOT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, HWND_MESSAGE, MSG, WM_HOTKEY, WNDCLASSW,
};

const HK_CAPTURE: i32 = 1;
const HK_OCR: i32 = 2;

static APP: OnceLock<AppHandle> = OnceLock::new();

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_HOTKEY {
        let id = wparam.0 as i32;
        if let Some(app) = APP.get() {
            let mode = if id == HK_OCR { "ocr" } else { "capture" };
            crate::trigger_capture(app.clone(), mode.to_string());
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub fn spawn(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let cls = w!("SnapShotHotkeys");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: cls,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            Default::default(),
            cls,
            w!(""),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinst.into()),
            None,
        ) else {
            return;
        };

        let _ = RegisterHotKey(Some(hwnd), HK_CAPTURE, MOD_NOREPEAT, VK_SNAPSHOT.0 as u32);
        let _ = RegisterHotKey(
            Some(hwnd),
            HK_OCR,
            HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0),
            b'O' as u32,
        );

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnregisterHotKey(Some(hwnd), HK_CAPTURE);
        let _ = UnregisterHotKey(Some(hwnd), HK_OCR);
        let _ = DestroyWindow(hwnd);
    });
}
