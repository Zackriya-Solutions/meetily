//! One place where macOS-only calls meet the rest of the feature, so the
//! scheduler and commands compile everywhere and simply report "unsupported"
//! on Windows and Linux.

use chrono::{DateTime, Utc};

use super::logic::{AudioUse, CalendarAccess, CalendarEvent};

#[cfg(target_os = "macos")]
pub fn access_status() -> CalendarAccess {
    super::eventkit::access_status()
}

#[cfg(target_os = "macos")]
pub fn request_access() -> CalendarAccess {
    super::eventkit::request_access()
}

#[cfg(target_os = "macos")]
pub fn fetch_events(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<CalendarEvent>, String> {
    super::eventkit::fetch_events(from, to)
}

#[cfg(target_os = "macos")]
pub fn current_audio_use() -> Vec<AudioUse> {
    super::audio_probe::current_audio_use()
}

/// Opens a meeting link in the user's default browser.
#[cfg(target_os = "macos")]
pub fn open_link(url: &str) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open meeting link: {}", e))
}

#[cfg(not(target_os = "macos"))]
pub fn access_status() -> CalendarAccess {
    CalendarAccess::Unsupported
}

#[cfg(not(target_os = "macos"))]
pub fn request_access() -> CalendarAccess {
    CalendarAccess::Unsupported
}

#[cfg(not(target_os = "macos"))]
pub fn fetch_events(_from: DateTime<Utc>, _to: DateTime<Utc>) -> Result<Vec<CalendarEvent>, String> {
    Err("Calendar auto-record is only available on macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn current_audio_use() -> Vec<AudioUse> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
pub fn open_link(_url: &str) -> Result<(), String> {
    Err("Calendar auto-record is only available on macOS".to_string())
}
