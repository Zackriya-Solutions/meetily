//! Asks Core Audio which apps are using the microphone or speakers right now.
//!
//! This uses the same process list as `audio::system_detector`, but it is polled
//! only while a meeting is due, and it reports input as well as output, because a
//! call app holding the microphone is a far better sign of a live call than audio
//! playing through the speakers.

use cidre::core_audio as ca;

use super::logic::AudioUse;

/// Every process except Meetily itself that has audio input or output running.
///
/// Meetily is left out by process id, otherwise its own recording would look like
/// a call that never ends.
pub fn current_audio_use() -> Vec<AudioUse> {
    let own_pid = std::process::id() as i32;
    let processes = match ca::System::processes() {
        Ok(processes) => processes,
        Err(e) => {
            log::warn!("calendar_autorecord: could not list audio processes: {:?}", e);
            return Vec::new();
        }
    };

    processes
        .into_iter()
        .filter_map(|process| {
            let pid = process.pid().ok()?;
            if pid == own_pid {
                return None;
            }
            let input = process.is_running_input().unwrap_or(false);
            let output = process.is_running_output().unwrap_or(false);
            (input || output).then_some(AudioUse { pid, input, output })
        })
        .collect()
}
