//! Tauri commands for the calendar auto-record settings screen.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use super::logic::{self, CalendarAccess, CallService, Eligibility};
use super::{platform, scheduler};

const STORE_FILE: &str = "calendar_autorecord.json";
const ENABLED_KEY: &str = "enabled";
/// The settings list shows the next day of meetings, which is enough to check the rule.
const UPCOMING_HOURS: i64 = 24;

#[derive(Debug, Clone, Serialize)]
pub struct CalendarAutoRecordStatus {
    pub enabled: bool,
    pub access: CalendarAccess,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpcomingMeeting {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub service: Option<CallService>,
    pub eligibility: Eligibility,
}

/// Reads the saved switch and starts the background loop. Called from app setup.
///
/// The feature is off unless the user has turned it on, so nobody's calls start
/// being recorded just because they updated the app.
pub fn init<R: Runtime>(app: &AppHandle<R>) {
    let enabled = app
        .store(STORE_FILE)
        .ok()
        .and_then(|store| store.get(ENABLED_KEY))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    scheduler::set_enabled(enabled);
    log::info!("Calendar auto-record: {}", if enabled { "enabled" } else { "disabled" });
    scheduler::start(app.clone());
}

fn status() -> CalendarAutoRecordStatus {
    CalendarAutoRecordStatus { enabled: scheduler::is_enabled(), access: platform::access_status() }
}

#[tauri::command]
pub async fn calendar_autorecord_get_status() -> Result<CalendarAutoRecordStatus, String> {
    Ok(status())
}

/// Turns the feature on or off and saves the choice.
///
/// Turning it on asks for calendar access first, and the switch stays off if
/// access is refused, since a feature that cannot read the calendar would look
/// enabled while doing nothing.
#[tauri::command]
pub async fn calendar_autorecord_set_enabled<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
) -> Result<CalendarAutoRecordStatus, String> {
    if enabled {
        let access = tauri::async_runtime::spawn_blocking(platform::request_access)
            .await
            .map_err(|e| format!("Calendar permission request failed: {}", e))?;
        if access != CalendarAccess::Granted {
            return Ok(CalendarAutoRecordStatus { enabled: false, access });
        }
    }

    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("Failed to access settings store: {}", e))?;
    store.set(ENABLED_KEY, serde_json::Value::Bool(enabled));
    store
        .save()
        .map_err(|e| format!("Failed to save settings: {}", e))?;

    scheduler::set_enabled(enabled);
    log::info!("Calendar auto-record turned {}", if enabled { "on" } else { "off" });
    Ok(status())
}

/// The next day's timed meetings and whether each would be recorded.
#[tauri::command]
pub async fn calendar_autorecord_upcoming() -> Result<Vec<UpcomingMeeting>, String> {
    let now = Utc::now();
    let to = now + Duration::hours(UPCOMING_HOURS);
    let events = tauri::async_runtime::spawn_blocking(move || platform::fetch_events(now, to))
        .await
        .map_err(|e| format!("Calendar read failed: {}", e))??;

    let mut meetings: Vec<UpcomingMeeting> = events
        .iter()
        .filter(|e| !e.all_day && e.end > now)
        .map(|e| UpcomingMeeting {
            title: e.title.clone(),
            start: e.start,
            end: e.end,
            service: logic::find_call_link(e).map(|l| l.service),
            eligibility: logic::eligibility(e),
        })
        .collect();
    meetings.sort_by_key(|m| m.start);
    Ok(meetings)
}
