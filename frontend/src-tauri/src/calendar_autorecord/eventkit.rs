//! Reads events from the Mac's Calendar database through EventKit.
//!
//! EventKit sees every account the user has added to the Calendar app (Google,
//! iCloud, Exchange), so no Google sign-in is needed. Only reading is done here.
//! The store is created per call because `EKEventStore` is not `Send`, and a
//! fetch once a minute is cheap next to the cost of sharing it across threads.

use std::sync::mpsc;
use std::time::Duration as StdDuration;

use block2::RcBlock;
use chrono::{DateTime, TimeZone, Utc};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_event_kit::{
    EKAuthorizationStatus, EKEntityType, EKEvent, EKEventStatus, EKEventStore, EKParticipantStatus,
};
use objc2_foundation::{NSDate, NSError, NSString};

use super::logic::{CalendarAccess, CalendarEvent, MyStatus};

pub fn access_status() -> CalendarAccess {
    let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
    match status {
        EKAuthorizationStatus::FullAccess => CalendarAccess::Granted,
        EKAuthorizationStatus::NotDetermined => CalendarAccess::NotDetermined,
        EKAuthorizationStatus::WriteOnly => CalendarAccess::WriteOnly,
        _ => CalendarAccess::Denied,
    }
}

/// Shows the macOS calendar permission prompt and waits for the answer.
///
/// macOS only prompts once; after a refusal the user has to allow Meetily in
/// System Settings, Privacy & Security, Calendars, so this then just reports the status.
pub fn request_access() -> CalendarAccess {
    if access_status() != CalendarAccess::NotDetermined {
        return access_status();
    }

    let (tx, rx) = mpsc::channel::<bool>();
    let handler = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        let _ = tx.send(granted.as_bool());
    });

    unsafe {
        let store = EKEventStore::new();
        if objc2::available!(macos = 14.0) {
            store.requestFullAccessToEventsWithCompletion(RcBlock::as_ptr(&handler));
        } else {
            // The older call is deprecated, but it is the only one macOS 13 and earlier have.
            #[allow(deprecated)]
            store.requestAccessToEntityType_completion(EKEntityType::Event, RcBlock::as_ptr(&handler));
        }
        // The user may take a while to read the prompt, so wait generously.
        let _ = rx.recv_timeout(StdDuration::from_secs(300));
    }

    access_status()
}

/// Returns events that overlap the window, from every calendar the user can see.
pub fn fetch_events(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<CalendarEvent>, String> {
    if access_status() != CalendarAccess::Granted {
        return Err("Calendar access has not been granted".to_string());
    }

    unsafe {
        let store = EKEventStore::new();
        let start = NSDate::dateWithTimeIntervalSince1970(from.timestamp() as f64);
        let end = NSDate::dateWithTimeIntervalSince1970(to.timestamp() as f64);
        let predicate = store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, None);
        let events = store.eventsMatchingPredicate(&predicate);

        let mut out = Vec::with_capacity(events.count());
        for event in events.iter() {
            if let Some(converted) = convert(&event) {
                out.push(converted);
            }
        }
        Ok(out)
    }
}

unsafe fn convert(event: &EKEvent) -> Option<CalendarEvent> {
    let start = to_utc(&event.startDate())?;
    let end = to_utc(&event.endDate())?;
    let id = event.eventIdentifier().map(|s| s.to_string()).unwrap_or_default();

    Some(CalendarEvent {
        key: format!("{}@{}", id, start.to_rfc3339()),
        title: non_empty(event.title().to_string()).unwrap_or_else(|| "Calendar meeting".to_string()),
        start,
        end,
        all_day: event.isAllDay(),
        cancelled: event.status() == EKEventStatus::Canceled,
        my_status: my_status(event),
        url: event.URL().and_then(|u| u.absoluteString()).map(|s| s.to_string()),
        location: event.location().map(|s| ns_to_string(&s)),
        notes: event.notes().map(|s| ns_to_string(&s)),
    })
}

unsafe fn my_status(event: &EKEvent) -> MyStatus {
    if let Some(organizer) = event.organizer() {
        if organizer.isCurrentUser() {
            return MyStatus::Organizer;
        }
    }
    if !event.hasAttendees() {
        return MyStatus::NoAttendees;
    }
    let Some(attendees) = event.attendees() else {
        return MyStatus::NoAttendees;
    };
    for attendee in attendees.iter() {
        if attendee.isCurrentUser() {
            return match attendee.participantStatus() {
                EKParticipantStatus::Accepted => MyStatus::Accepted,
                EKParticipantStatus::Declined => MyStatus::Declined,
                EKParticipantStatus::Tentative => MyStatus::Tentative,
                EKParticipantStatus::Pending => MyStatus::Pending,
                _ => MyStatus::Unknown,
            };
        }
    }
    MyStatus::Unknown
}

fn to_utc(date: &NSDate) -> Option<DateTime<Utc>> {
    let secs = date.timeIntervalSince1970();
    Utc.timestamp_opt(secs.floor() as i64, 0).single()
}

fn ns_to_string(s: &Retained<NSString>) -> String {
    s.to_string()
}

fn non_empty(s: String) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
