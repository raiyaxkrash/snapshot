//! Глобальные горячие клавиши SnapShot:
//! - `PrintScreen` — низкоуровневый хук `WH_KEYBOARD_LL` (перехватывает раньше
//!   встроенного Snipping Tool в Windows 11, где `RegisterHotKey(VK_SNAPSHOT)` уже занят)
//! - `Ctrl+Shift+O` — мгновенный OCR текста с области экрана

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_CONTROL,
    MOD_NOREPEAT, MOD_SHIFT, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SNAPSHOT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    RegisterClassW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HWND_MESSAGE,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WNDCLASSW,
};

const HK_OCR: i32 = 2;
const LLKHF_INJECTED: u32 = 0x10;

static APP: OnceLock<AppHandle> = OnceLock::new();
static PRTSCR_HELD: AtomicBool = AtomicBool::new(false);

fn key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

unsafe extern "system" fn kb_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if (kb.flags.0 & LLKHF_INJECTED) == 0 && kb.vkCode == VK_SNAPSHOT.0 as u32 {
            let msg = wparam.0 as u32;
            let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

            if is_down {
                let win_or_mod = key_down(VK_LWIN.0)
                    || key_down(VK_RWIN.0)
                    || key_down(VK_CONTROL.0)
                    || key_down(VK_MENU.0);
                if !win_or_mod {
                    if !PRTSCR_HELD.swap(true, Ordering::SeqCst) {
                        if let Some(app) = APP.get() {
                            crate::trigger_capture(app.clone(), "capture".to_string());
                        }
                    }
                    // Глотаем событие до того, как его увидит Snipping Tool / Windows Shell.
                    return LRESULT(1);
                }
            } else if is_up && PRTSCR_HELD.swap(false, Ordering::SeqCst) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_HOTKEY && wparam.0 as i32 == HK_OCR {
        if let Some(app) = APP.get() {
            crate::trigger_capture(app.clone(), "ocr".to_string());
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub fn spawn(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), Some(hinst.into()), 0).ok();

        let cls = w!("SnapShotHotkeys");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: cls,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
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
        )
        .ok();

        if let Some(h) = hwnd {
            let _ = RegisterHotKey(
                Some(h),
                HK_OCR,
                HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0),
                b'O' as u32,
            );
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if let Some(h) = hwnd {
            let _ = UnregisterHotKey(Some(h), HK_OCR);
            let _ = DestroyWindow(h);
        }
        if let Some(hk) = hook {
            let _ = UnhookWindowsHookEx(hk);
        }
    });
}
