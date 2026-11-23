//! SnapShot — современный PrintScreen для Windows: выделение области, стрелки,
//! размытие паролей, Beautify-фон с тенью и оффлайн OCR (`Windows.Media.Ocr`).

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

mod capture;
mod hotkeys;
mod ocr;

static CAPTURING: AtomicBool = AtomicBool::new(false);

pub(crate) fn trigger_capture(app: AppHandle, mode: String) {
    if CAPTURING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        if let Some(w) = app.get_webview_window("overlay") {
            let _ = w.hide();
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
        if let Some(shot) = capture::capture_virtual_screen() {
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.set_position(PhysicalPosition::new(shot.x, shot.y));
                let _ = w.set_size(PhysicalSize::new(shot.w as u32, shot.h as u32));
                let _ = app.emit("snapshot-ready", json!({ "mode": mode, "screen": shot }));
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        CAPTURING.store(false, Ordering::SeqCst);
    });
}

#[tauri::command]
fn cancel_capture(app: AppHandle) {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }
}

#[tauri::command]
async fn copy_image(app: AppHandle, data_url: String) -> bool {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }
    tauri::async_runtime::spawn_blocking(move || {
        capture::decode_data_url_png(&data_url)
            .is_some_and(|png| capture::copy_png_to_clipboard(&png))
    })
    .await
    .unwrap_or(false)
}

#[tauri::command]
async fn save_image(app: AppHandle, data_url: String) -> Option<String> {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }
    tauri::async_runtime::spawn_blocking(move || {
        let png = capture::decode_data_url_png(&data_url)?;
        let _ = capture::copy_png_to_clipboard(&png);
        capture::save_png_file(&png)
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
async fn ocr_image(app: AppHandle, data_url: String) -> Value {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }
    let res = tauri::async_runtime::spawn_blocking(move || {
        let Some(png) = capture::decode_data_url_png(&data_url) else {
            return Err("decode");
        };
        let text = ocr::recognize_png(&png)?;
        if !text.is_empty() {
            let _ = capture::copy_text_to_clipboard(&text);
        }
        Ok(text)
    })
    .await
    .unwrap_or(Err("ocr"));

    match res {
        Ok(text) => json!({ "ok": true, "text": text }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

fn open_save_folder() {
    let dir = capture::save_dir();
    let _ = std::fs::create_dir_all(&dir);
    unsafe {
        let wide = HSTRING::from(dir.to_string_lossy().as_ref());
        let _ = ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

fn already_running() -> bool {
    unsafe {
        let _ = CreateMutexW(None, true, w!("Local\\SnapShot.SingleInstance"));
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}

pub fn run() {
    if already_running() {
        return;
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            cancel_capture,
            copy_image,
            save_image,
            ocr_image
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            hotkeys::spawn(handle.clone());

            let cap_i = MenuItem::with_id(app, "capture", "Сделать скриншот (PrintScreen)", true, None::<&str>)?;
            let ocr_i = MenuItem::with_id(app, "ocr", "Распознать текст OCR (Ctrl+Shift+O)", true, None::<&str>)?;
            let dir_i = MenuItem::with_id(app, "folder", "Открыть папку скриншотов", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_i = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&cap_i, &ocr_i, &dir_i, &sep, &quit_i])?;

            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("SnapShot — PrintScreen & OCR")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        trigger_capture(tray.app_handle().clone(), "capture".to_string());
                    }
                })
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "capture" => trigger_capture(app.clone(), "capture".to_string()),
                    "ocr" => trigger_capture(app.clone(), "ocr".to_string()),
                    "folder" => open_save_folder(),
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("не удалось запустить SnapShot");
}
