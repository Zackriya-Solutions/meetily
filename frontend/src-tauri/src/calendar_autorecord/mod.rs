//! Calendar auto-record: opens the link for an upcoming meeting and records it.
//!
//! When switched on, Meetily reads the Mac's Calendar app and acts on events the
//! user has accepted or organized that carry a Google Meet, Zoom or Teams link.
//! About a minute before each one it opens the link in the default browser, then
//! starts recording once the call app begins using the microphone, and stops
//! after the call has been quiet for two minutes. It never records when the user
//! is not on the call, because Meetily captures this Mac's audio and nothing else.
//!
//! The feature is off by default and macOS only.

pub mod commands;
pub mod logic;
mod platform;
mod scheduler;

#[cfg(target_os = "macos")]
mod audio_probe;
#[cfg(target_os = "macos")]
mod eventkit;
