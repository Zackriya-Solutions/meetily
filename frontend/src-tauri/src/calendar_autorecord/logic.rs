//! Platform-free rules for calendar auto-record.
//!
//! Everything here is plain data in, decision out, so the rules can be unit tested
//! without EventKit, Core Audio or a running Tauri app. The macOS glue in
//! `eventkit.rs` and `audio_probe.rs` only gathers the inputs, and `scheduler.rs`
//! only carries out the actions these functions return.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

/// How long before the start time the meeting link is opened.
pub const OPEN_LINK_LEAD_SECS: i64 = 60;
/// Recording may start this long before the scheduled start, for people who join early.
pub const ARM_BEFORE_START_SECS: i64 = 120;
/// A meeting found later than this after its start is not opened again,
/// because the user has most likely joined it already.
pub const OPEN_LINK_LATEST_SECS: i64 = 300;
/// The call app must have been silent for this long before an auto-started
/// recording is stopped, so a short dropout or a reconnect does not end it.
pub const STOP_AFTER_IDLE_SECS: i64 = 120;
/// After the scheduled end, speaker output alone no longer counts as the call
/// still running, since a browser that moves on to a video would otherwise keep
/// the recording alive. Only microphone use keeps it going past this point.
pub const OUTPUT_COUNTS_UNTIL_AFTER_END_SECS: i64 = 900;
/// A recording takes a few seconds to begin after the start signal, while the
/// page reloads and the transcription model loads, so "not recording" is only
/// believed once this much time has passed since the signal.
pub const START_GRACE_SECS: i64 = 60;

/// Calendar permission as the settings screen needs to show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarAccess {
    Granted,
    NotDetermined,
    Denied,
    /// The user allowed adding events but not reading them, which is not enough.
    WriteOnly,
    /// Not macOS, where EventKit does not exist.
    Unsupported,
}

/// The user's own response to an event, as far as the calendar reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MyStatus {
    /// The user organized the event.
    Organizer,
    Accepted,
    Tentative,
    Declined,
    Pending,
    /// The event has no invitees, so it is the user's own entry.
    NoAttendees,
    /// The event has invitees but none of them is marked as the current user.
    Unknown,
}

/// A calendar event reduced to the fields the rules need.
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarEvent {
    /// Stable per occurrence: the calendar's event id plus the start time,
    /// because recurring meetings share one event id across occurrences.
    pub key: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub cancelled: bool,
    pub my_status: MyStatus,
    pub url: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
}

/// Which call service a link belongs to, shown in the settings list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallService {
    GoogleMeet,
    Zoom,
    Teams,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallLink {
    pub url: String,
    pub service: CallService,
}

/// Finds the first Google Meet, Zoom or Teams link in the event.
///
/// The URL field is checked first, then the location, then the notes, since
/// Google Calendar writes the Meet link into the description for calendars synced
/// through the Mac's Calendar app, while Zoom and Teams often use the location.
pub fn find_call_link(event: &CalendarEvent) -> Option<CallLink> {
    [&event.url, &event.location, &event.notes]
        .into_iter()
        .flatten()
        .find_map(|text| first_call_link_in(text))
}

fn first_call_link_in(text: &str) -> Option<CallLink> {
    let mut rest = text;
    while let Some(pos) = rest.find("https://") {
        let candidate = &rest[pos..];
        let end = candidate
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | ','))
            .unwrap_or(candidate.len());
        let url = candidate[..end].trim_end_matches(['.', ';']);
        if let Some(service) = classify_call_url(url) {
            return Some(CallLink { url: url.to_string(), service });
        }
        rest = &candidate["https://".len()..];
    }
    None
}

fn classify_call_url(url: &str) -> Option<CallService> {
    let without_scheme = url.strip_prefix("https://")?;
    let (host, path) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None => (without_scheme, ""),
    };
    let host = host.to_ascii_lowercase();

    if host == "meet.google.com" {
        // A bare meet.google.com link is the Meet home page, not a meeting.
        let code = path.trim_start_matches('/').split(['?', '#']).next().unwrap_or("");
        return (!code.is_empty() && code != "new").then_some(CallService::GoogleMeet);
    }
    if host == "zoom.us" || host.ends_with(".zoom.us") {
        let is_meeting = path.starts_with("/j/") || path.starts_with("/my/") || path.starts_with("/w/");
        return is_meeting.then_some(CallService::Zoom);
    }
    if host == "teams.microsoft.com" && path.starts_with("/l/meetup-join/") {
        return Some(CallService::Teams);
    }
    if host == "teams.live.com" && path.starts_with("/meet/") {
        return Some(CallService::Teams);
    }
    None
}

/// Why an event will or will not be recorded, shown next to it in settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Eligibility {
    WillRecord,
    NoCallLink,
    AllDay,
    Cancelled,
    NotAccepted,
}

/// Applies the recording rule: only events the user has
/// accepted or organized, and only when they carry a call link.
pub fn eligibility(event: &CalendarEvent) -> Eligibility {
    if event.cancelled {
        return Eligibility::Cancelled;
    }
    if event.all_day {
        return Eligibility::AllDay;
    }
    if !matches!(event.my_status, MyStatus::Organizer | MyStatus::Accepted | MyStatus::NoAttendees) {
        return Eligibility::NotAccepted;
    }
    if find_call_link(event).is_none() {
        return Eligibility::NoCallLink;
    }
    Eligibility::WillRecord
}

/// One app's current use of the Mac's audio, as Core Audio reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioUse {
    pub pid: i32,
    pub input: bool,
    pub output: bool,
}

/// Where the scheduler is with one meeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Nothing done yet.
    Waiting,
    /// The link was opened (or deliberately skipped); waiting for the call audio.
    Armed,
    /// This feature started the recording, triggered by the app with this pid.
    Recording { trigger_pid: i32, started_at: DateTime<Utc>, idle_since: Option<DateTime<Utc>> },
    /// Finished, skipped or handed over to the user; nothing more happens.
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    OpenLink(String),
    StartRecording { title: String, trigger_pid: i32 },
    StopRecording,
}

/// Everything `step` needs to know about the world at one tick.
pub struct Tick<'a> {
    pub now: DateTime<Utc>,
    /// True when any recording is running, whoever started it.
    pub recording_active: bool,
    /// True when the running recording was started by this feature for some
    /// meeting, which back-to-back meetings need to tell apart from a manual one.
    pub our_recording: bool,
    /// Apps other than Meetily that are using audio right now.
    pub audio: &'a [AudioUse],
}

/// Advances one meeting by one tick and says what, if anything, to do.
///
/// The caller stores the returned phase and carries out the action. Keeping
/// this pure is what lets the timing rules be tested without a calendar or a call.
pub fn step(event: &CalendarEvent, phase: &Phase, tick: &Tick) -> (Phase, Action) {
    let now = tick.now;
    match phase {
        Phase::Waiting => {
            let link = match find_call_link(event) {
                Some(link) if eligibility(event) == Eligibility::WillRecord => link,
                _ => return (Phase::Done, Action::None),
            };
            if now >= event.end {
                return (Phase::Done, Action::None);
            }
            if now < event.start - Duration::seconds(OPEN_LINK_LEAD_SECS) {
                return (Phase::Waiting, Action::None);
            }
            if now > event.start + Duration::seconds(OPEN_LINK_LATEST_SECS) {
                // Found late (Meetily was opened mid-meeting): arm without opening a
                // second copy of a call the user is probably already in.
                return (Phase::Armed, Action::None);
            }
            (Phase::Armed, Action::OpenLink(link.url))
        }
        Phase::Armed => {
            if now >= event.end {
                return (Phase::Done, Action::None);
            }
            if tick.recording_active && !tick.our_recording {
                // The user started a recording themselves, so leave it to them.
                return (Phase::Done, Action::None);
            }
            if tick.recording_active || now < event.start - Duration::seconds(ARM_BEFORE_START_SECS) {
                // Either too early, or the previous meeting's recording is still
                // winding down; this one waits its turn rather than giving up.
                return (Phase::Armed, Action::None);
            }
            match tick.audio.iter().find(|a| a.input) {
                Some(app) => (
                    Phase::Recording { trigger_pid: app.pid, started_at: now, idle_since: None },
                    Action::StartRecording { title: event.title.clone(), trigger_pid: app.pid },
                ),
                None => (Phase::Armed, Action::None),
            }
        }
        Phase::Recording { trigger_pid, started_at, idle_since } => {
            if !tick.recording_active {
                if now - *started_at < Duration::seconds(START_GRACE_SECS) {
                    // Still starting up, so wait before concluding anything.
                    return (phase.clone(), Action::None);
                }
                // Stopped by hand, or it failed to start; either way it is not ours to manage.
                return (Phase::Done, Action::None);
            }
            let output_still_counts = now < event.end + Duration::seconds(OUTPUT_COUNTS_UNTIL_AFTER_END_SECS);
            let call_active = tick
                .audio
                .iter()
                .any(|a| a.pid == *trigger_pid && (a.input || (output_still_counts && a.output)));
            let recording = |idle_since| Phase::Recording { trigger_pid: *trigger_pid, started_at: *started_at, idle_since };
            if call_active {
                return (recording(None), Action::None);
            }
            let since = idle_since.unwrap_or(now);
            if now - since >= Duration::seconds(STOP_AFTER_IDLE_SECS) {
                (Phase::Done, Action::StopRecording)
            } else {
                (recording(Some(since)), Action::None)
            }
        }
        Phase::Done => (Phase::Done, Action::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(min: i64, sec: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 29, 15, 0, 0).unwrap() + Duration::minutes(min) + Duration::seconds(sec)
    }

    fn meeting() -> CalendarEvent {
        CalendarEvent {
            key: "abc@2026-10-29T15:00:00Z".into(),
            title: "Seo Update Meeting".into(),
            start: at(0, 0),
            end: at(45, 0),
            all_day: false,
            cancelled: false,
            my_status: MyStatus::Accepted,
            url: None,
            location: None,
            notes: Some("Join with Google Meet: https://meet.google.com/hhy-yirm-jsi\nOr dial: +1 555".into()),
        }
    }

    fn tick<'a>(now: DateTime<Utc>, recording_active: bool, audio: &'a [AudioUse]) -> Tick<'a> {
        Tick { now, recording_active, our_recording: false, audio }
    }

    /// A tick while a recording this feature started is running.
    fn ours<'a>(now: DateTime<Utc>, audio: &'a [AudioUse]) -> Tick<'a> {
        Tick { now, recording_active: true, our_recording: true, audio }
    }

    /// The phase right after this feature started recording at 15:00:30.
    fn recording() -> Phase {
        Phase::Recording { trigger_pid: 501, started_at: at(0, 30), idle_since: None }
    }

    const CHROME_ON_CALL: AudioUse = AudioUse { pid: 501, input: true, output: true };

    #[test]
    fn finds_meet_link_in_google_description() {
        let link = find_call_link(&meeting()).unwrap();
        assert_eq!(link.url, "https://meet.google.com/hhy-yirm-jsi");
        assert_eq!(link.service, CallService::GoogleMeet);
    }

    #[test]
    fn finds_zoom_and_teams_links_and_ignores_other_urls() {
        let mut e = meeting();
        e.notes = Some("Agenda: https://docs.google.com/x. Join: https://us02web.zoom.us/j/8812345678?pwd=abc.".into());
        assert_eq!(find_call_link(&e).unwrap().url, "https://us02web.zoom.us/j/8812345678?pwd=abc");

        e.notes = None;
        e.location = Some("Microsoft Teams Meeting <https://teams.microsoft.com/l/meetup-join/19%3ameeting_x/0>".into());
        assert_eq!(find_call_link(&e).unwrap().service, CallService::Teams);

        e.location = Some("https://meet.google.com/ and https://zoom.us/signin".into());
        assert_eq!(find_call_link(&e), None);
    }

    #[test]
    fn url_field_wins_over_notes() {
        let mut e = meeting();
        e.url = Some("https://zoom.us/j/123".into());
        assert_eq!(find_call_link(&e).unwrap().service, CallService::Zoom);
    }

    #[test]
    fn eligibility_follows_the_chosen_rule() {
        let mut e = meeting();
        assert_eq!(eligibility(&e), Eligibility::WillRecord);
        e.my_status = MyStatus::Organizer;
        assert_eq!(eligibility(&e), Eligibility::WillRecord);
        e.my_status = MyStatus::NoAttendees;
        assert_eq!(eligibility(&e), Eligibility::WillRecord);
        for status in [MyStatus::Tentative, MyStatus::Declined, MyStatus::Pending, MyStatus::Unknown] {
            e.my_status = status;
            assert_eq!(eligibility(&e), Eligibility::NotAccepted, "{status:?}");
        }
        let mut e = meeting();
        e.notes = Some("Focus block".into());
        assert_eq!(eligibility(&e), Eligibility::NoCallLink);
        let mut e = meeting();
        e.all_day = true;
        assert_eq!(eligibility(&e), Eligibility::AllDay);
        let mut e = meeting();
        e.cancelled = true;
        assert_eq!(eligibility(&e), Eligibility::Cancelled);
    }

    #[test]
    fn opens_link_one_minute_before_start() {
        let e = meeting();
        assert_eq!(step(&e, &Phase::Waiting, &tick(at(-2, 0), false, &[])), (Phase::Waiting, Action::None));
        assert_eq!(
            step(&e, &Phase::Waiting, &tick(at(-1, 0), false, &[])),
            (Phase::Armed, Action::OpenLink("https://meet.google.com/hhy-yirm-jsi".into()))
        );
    }

    #[test]
    fn found_late_arms_without_reopening() {
        let e = meeting();
        assert_eq!(step(&e, &Phase::Waiting, &tick(at(10, 0), false, &[])), (Phase::Armed, Action::None));
        assert_eq!(step(&e, &Phase::Waiting, &tick(at(46, 0), false, &[])), (Phase::Done, Action::None));
    }

    #[test]
    fn ineligible_meeting_is_dropped_straight_away() {
        let mut e = meeting();
        e.my_status = MyStatus::Declined;
        assert_eq!(step(&e, &Phase::Waiting, &tick(at(-1, 0), false, &[])), (Phase::Done, Action::None));
    }

    #[test]
    fn starts_only_when_another_app_uses_the_microphone() {
        let e = meeting();
        let music = [AudioUse { pid: 700, input: false, output: true }];
        assert_eq!(step(&e, &Phase::Armed, &tick(at(0, 30), false, &music)), (Phase::Armed, Action::None));
        assert_eq!(
            step(&e, &Phase::Armed, &tick(at(0, 30), false, &[CHROME_ON_CALL])),
            (recording(), Action::StartRecording { title: "Seo Update Meeting".into(), trigger_pid: 501 })
        );
    }

    #[test]
    fn leaves_a_manual_recording_alone() {
        let e = meeting();
        assert_eq!(step(&e, &Phase::Armed, &tick(at(1, 0), true, &[CHROME_ON_CALL])), (Phase::Done, Action::None));
    }

    #[test]
    fn stops_after_two_quiet_minutes_not_before() {
        let e = meeting();
        let rec = recording();
        let (p1, a1) = step(&e, &rec, &ours(at(40, 0), &[]));
        assert_eq!(a1, Action::None);
        let (p2, a2) = step(&e, &p1, &ours(at(41, 0), &[]));
        assert_eq!(a2, Action::None);
        // The call comes back before the two minutes are up, which resets the clock.
        let (p3, _) = step(&e, &p2, &ours(at(41, 30), &[CHROME_ON_CALL]));
        assert_eq!(p3, rec);
        let (p4, _) = step(&e, &p3, &ours(at(42, 0), &[]));
        let (_, a5) = step(&e, &p4, &ours(at(43, 59), &[]));
        assert_eq!(a5, Action::None);
        let (p6, a6) = step(&e, &p4, &ours(at(44, 0), &[]));
        assert_eq!((p6, a6), (Phase::Done, Action::StopRecording));
    }

    #[test]
    fn overrunning_meeting_keeps_recording_while_the_call_is_live() {
        let e = meeting();
        let rec = recording();
        assert_eq!(step(&e, &rec, &ours(at(70, 0), &[CHROME_ON_CALL])), (rec.clone(), Action::None));
    }

    #[test]
    fn speaker_output_alone_stops_counting_well_after_the_end() {
        let e = meeting();
        let rec = recording();
        let video = [AudioUse { pid: 501, input: false, output: true }];
        // Within fifteen minutes of the end, speaker output still counts as the call.
        assert_eq!(step(&e, &rec, &ours(at(50, 0), &video)).0, rec);
        // Later, the same tab playing a video no longer keeps the recording alive.
        let (p, _) = step(&e, &rec, &ours(at(61, 0), &video));
        assert!(matches!(p, Phase::Recording { idle_since: Some(_), .. }));
    }

    #[test]
    fn other_apps_do_not_keep_our_recording_alive() {
        let e = meeting();
        let rec = recording();
        let zoom = [AudioUse { pid: 900, input: true, output: true }];
        let (p, _) = step(&e, &rec, &ours(at(30, 0), &zoom));
        assert!(matches!(p, Phase::Recording { idle_since: Some(_), .. }));
    }

    #[test]
    fn manual_stop_hands_control_back() {
        let e = meeting();
        let rec = recording();
        assert_eq!(step(&e, &rec, &tick(at(20, 0), false, &[CHROME_ON_CALL])), (Phase::Done, Action::None));
    }

    #[test]
    fn slow_start_is_not_mistaken_for_a_manual_stop() {
        let e = meeting();
        // Five seconds after the signal nothing is recording yet, which is normal.
        assert_eq!(step(&e, &recording(), &tick(at(0, 35), false, &[CHROME_ON_CALL])).0, recording());
        // A minute on, still nothing means the start failed, so let go.
        assert_eq!(step(&e, &recording(), &tick(at(1, 30), false, &[CHROME_ON_CALL])), (Phase::Done, Action::None));
    }

    #[test]
    fn back_to_back_meeting_waits_for_the_previous_recording() {
        let next = CalendarEvent { key: "next".into(), start: at(45, 0), end: at(75, 0), ..meeting() };
        // The earlier meeting's recording is still ours and running, so wait.
        assert_eq!(step(&next, &Phase::Armed, &ours(at(44, 0), &[CHROME_ON_CALL])), (Phase::Armed, Action::None));
        // Once it has stopped, the next meeting starts its own recording.
        let (p, a) = step(&next, &Phase::Armed, &tick(at(46, 0), false, &[CHROME_ON_CALL]));
        assert!(matches!(p, Phase::Recording { trigger_pid: 501, .. }));
        assert!(matches!(a, Action::StartRecording { .. }));
    }
}
