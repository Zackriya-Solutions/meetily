use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Runtime};

static USE_CHINESE: AtomicBool = AtomicBool::new(false);

pub fn text<'a>(english: &'a str, chinese: &'a str) -> &'a str {
    if USE_CHINESE.load(Ordering::Relaxed) { chinese } else { english }
}

/// Synchronize native interface text with the persisted frontend UI language.
#[tauri::command]
pub fn set_ui_language<R: Runtime>(app: AppHandle<R>, language: String) -> Result<(), String> {
    match language.as_str() {
        "zh-CN" => USE_CHINESE.store(true, Ordering::Relaxed),
        "en-US" => USE_CHINESE.store(false, Ordering::Relaxed),
        _ => return Err("Unsupported UI language".to_string()),
    }
    crate::tray::update_tray_menu(&app);
    Ok(())
}
