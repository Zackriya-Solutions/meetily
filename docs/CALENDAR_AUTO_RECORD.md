# Calendar auto-record (macOS)

Calendar auto-record lets Meetily start recording a meeting without anyone pressing Start. It reads the Calendar app on the Mac, opens the meeting link shortly before each call, and records once the call is actually under way. It is off by default, so nothing changes for anyone who does not switch it on.

## What it does

Meetily checks the calendar once a minute while it is running. It acts only on timed events that you organized or accepted and that contain a Google Meet, Zoom or Teams link, which keeps focus blocks, reminders and declined invites out of it. For each of those meetings it follows the same sequence.

1. One minute before the start, Meetily opens the meeting link in your browser. If Meetily notices a meeting more than five minutes after it began, it skips this step, because you have most likely joined already.
2. From two minutes before the start, Meetily watches for another app (the browser, Zoom or Teams) to begin using the microphone. Recording starts at that moment and is named after the calendar event. Waiting for the microphone, not the clock, means an empty room is never recorded when you join late.
3. Recording stops once the call app has used neither the microphone nor the speakers for two minutes. Within fifteen minutes of the scheduled end, speaker output alone still counts as the call, and after that only microphone use does, so a browser tab that moves on to a video does not keep the recording alive.

Two rules keep it out of your way. A recording you started yourself is never stopped or replaced, and a recording you stop by hand is not restarted. When meetings run back to back, the second one waits until the first recording has stopped and then starts its own.

## What it cannot do

Meetily records the audio on this Mac, so it can only record calls you join from this Mac. It cannot attend a meeting for you, and it does nothing while Meetily is closed. So keep Meetily running. Closing its window only hides it, and adding it to Login Items means it starts with the Mac.

## Setting it up

1. If your meetings are in Google Calendar, add the Google account in System Settings, Internet Accounts, and tick Calendars. Meetily reads whatever the Calendar app shows, so iCloud and Exchange calendars work the same way.
2. In Meetily, open Settings, Preferences and switch on Calendar auto-record. macOS asks for calendar access the first time; choose Allow Full Access, since read access is what the feature needs.
3. Check the "Next 24 hours" list under the switch. Each meeting shows whether it will be recorded, and if not, why (no call link, not accepted, cancelled).

If you denied access by mistake, open System Settings, Privacy & Security, Calendars and allow Meetily, then switch the feature on again.

## Testing it

The quickest check uses a meeting you create yourself.

1. Create a Google Calendar event starting in five minutes, add Google Meet video conferencing, and save it. Confirm it appears in the Calendar app on the Mac.
2. In Meetily's settings, press Refresh under "Next 24 hours" and confirm the event says "Will record (Google Meet)".
3. One minute before the start, the Meet link should open in your browser. Join the call; recording should start within a few seconds of your microphone turning on, named after the event.
4. Leave the call. About two minutes later the recording should stop and save like any other.

Recording laws differ by place, and some require everyone on a call to agree to being recorded, so let attendees know when you use this.

## How it is built

The feature lives in `frontend/src-tauri/src/calendar_autorecord/`. The decisions (which events qualify, how links are found, and when to open, start and stop) are plain Rust in `logic.rs`, with unit tests that need no Mac, calendar or call. `eventkit.rs` reads the calendar through EventKit with the `objc2-event-kit` crate, and `audio_probe.rs` asks Core Audio which apps are using the microphone and speakers. `scheduler.rs` is the loop that ties them together, and it starts and stops recordings through the same path as the tray's Start and Stop items (`tray::start_recording_for_meeting` and `tray::stop_and_save_recording`), so an automatic recording is saved exactly like a manual one. The switch is stored in `calendar_autorecord.json` through the Tauri store.

The app needs the `com.apple.security.personal-information.calendars` entitlement because it runs with the hardened runtime, and `Info.plist` carries the calendar usage descriptions that macOS shows in its permission prompt.
