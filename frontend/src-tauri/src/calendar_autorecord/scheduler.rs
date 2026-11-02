//! The background loop that applies `logic::step` to upcoming meetings.
//!
//! It runs for the life of the app but does nothing while the feature is switched
//! off. The calendar is read once a minute, and audio is only queried while a
//! meeting is armed or being recorded, so an idle Mac pays almost nothing for it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration as StdDuration, Instant};

use chrono::{Duration, Utc};
use tauri::{AppHandle, Runtime};

use super::logic::{self, Action, CalendarAccess, CalendarEvent, Phase, Tick};
use super::platform;

static ENABLED: AtomicBool = AtomicBool::new(false);

const TICK: StdDuration = StdDuration::from_secs(5);
const CALENDAR_REFRESH: StdDuration = StdDuration::from_secs(60);
/// How far back events are read, so a long meeting already under way is still seen.
const LOOK_BACK_HOURS: i64 = 6;
/// How far ahead events are read; the link opens a minute before start, so this is plenty.
const LOOK_AHEAD_MINUTES: i64 = 30;

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

/// Starts the loop. Call once from app setup.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        run(app).await;
    });
}

struct Tracked {
    event: CalendarEvent,
    phase: Phase,
}

async fn run<R: Runtime>(app: AppHandle<R>) {
    let mut tracked: HashMap<String, Tracked> = HashMap::new();
    let mut last_refresh: Option<Instant> = None;
    let mut ticker = tokio::time::interval(TICK);

    loop {
        ticker.tick().await;

        if !is_enabled() {
            if !tracked.is_empty() {
                // Switching off forgets everything, but leaves a running recording to
                // the user rather than stopping it behind their back.
                tracked.clear();
                last_refresh = None;
            }
            continue;
        }
        if platform::access_status() != CalendarAccess::Granted {
            continue;
        }

        if last_refresh.map_or(true, |t| t.elapsed() >= CALENDAR_REFRESH) {
            refresh(&mut tracked).await;
            last_refresh = Some(Instant::now());
        }

        let needs_audio = tracked
            .values()
            .any(|t| matches!(t.phase, Phase::Armed | Phase::Recording { .. }));
        let audio = if needs_audio {
            tauri::async_runtime::spawn_blocking(platform::current_audio_use)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let recording_active = crate::audio::recording_commands::is_recording().await;
        let our_recording = tracked.values().any(|t| matches!(t.phase, Phase::Recording { .. }));

        let tick = Tick { now: Utc::now(), recording_active, our_recording, audio: &audio };

        // Meetings are stepped in start order, so when two are armed at once the
        // earlier one gets the recording.
        let mut keys: Vec<String> = tracked.keys().cloned().collect();
        keys.sort_by_key(|k| tracked[k].event.start);
        let mut started_this_tick = false;

        for key in keys {
            let entry = tracked.get_mut(&key).expect("key taken from the map");
            if started_this_tick && matches!(entry.phase, Phase::Armed) {
                // Only one recording can run, so later meetings wait for the next tick.
                continue;
            }
            let (next, action) = logic::step(&entry.event, &entry.phase, &tick);
            if matches!(entry.phase, Phase::Recording { .. }) && next == Phase::Done && !recording_active {
                // The start never took (no model, say), so put the tray back from "Starting".
                crate::tray::update_tray_menu(&app);
            }
            entry.phase = next;
            match action {
                Action::None => {}
                Action::OpenLink(url) => {
                    log::info!("Calendar auto-record: opening link for '{}'", entry.event.title);
                    if let Err(e) = platform::open_link(&url) {
                        log::error!("Calendar auto-record: {}", e);
                    }
                }
                Action::StartRecording { title, trigger_pid } => {
                    log::info!(
                        "Calendar auto-record: call audio detected (pid {}), starting recording for '{}'",
                        trigger_pid,
                        title
                    );
                    if crate::tray::start_recording_for_meeting(&app, &title) {
                        started_this_tick = true;
                    } else {
                        entry.phase = Phase::Done;
                    }
                }
                Action::StopRecording => {
                    log::info!(
                        "Calendar auto-record: call audio ended, stopping recording for '{}'",
                        entry.event.title
                    );
                    crate::tray::set_tray_state(&app, crate::tray::RecordingState::Stopping);
                    crate::tray::stop_and_save_recording(app.clone()).await;
                }
            }
        }
    }
}

/// Re-reads the calendar and merges the result into what is being tracked.
///
/// Existing phases are kept, so a refresh never reopens a link or restarts a
/// recording. An event that has gone from the calendar is dropped unless its
/// recording is running, because deleting the invite should not cut off a call.
async fn refresh(tracked: &mut HashMap<String, Tracked>) {
    let now = Utc::now();
    let from = now - Duration::hours(LOOK_BACK_HOURS);
    let to = now + Duration::minutes(LOOK_AHEAD_MINUTES);

    let fetched = match tauri::async_runtime::spawn_blocking(move || platform::fetch_events(from, to)).await {
        Ok(Ok(events)) => events,
        Ok(Err(e)) => {
            log::warn!("Calendar auto-record: could not read calendar: {}", e);
            return;
        }
        Err(e) => {
            log::warn!("Calendar auto-record: calendar read task failed: {}", e);
            return;
        }
    };

    let mut seen = std::collections::HashSet::new();
    for event in fetched {
        seen.insert(event.key.clone());
        match tracked.get_mut(&event.key) {
            Some(existing) => existing.event = event,
            None => {
                tracked.insert(event.key.clone(), Tracked { event, phase: Phase::Waiting });
            }
        }
    }
    tracked.retain(|key, t| seen.contains(key) || matches!(t.phase, Phase::Recording { .. }));
}
