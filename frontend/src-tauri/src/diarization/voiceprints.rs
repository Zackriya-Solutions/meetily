// diarization/voiceprints.rs
//
// Each speaker's voice at the end of a recording, saved next to the audio in
// the meeting folder. Naming a speaker later can then turn that voice into a
// profile of the person, and the file travels with the recording.

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const VOICEPRINTS_FILE: &str = "speakers.json";

/// One speaker's cluster centroid (L2-normalized) and how many segments
/// built it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voiceprint {
    pub label: String,
    pub segments: usize,
    pub embedding: Vec<f32>,
}

/// Writes the voiceprints of one meeting. Through a temp file, so a crash
/// mid-write leaves the old file or none, never half of one.
pub fn write_voiceprints(folder: &Path, voiceprints: &[Voiceprint]) -> std::io::Result<()> {
    let json = serde_json::to_vec(voiceprints)?;
    let temp = folder.join(format!(".{}.tmp", VOICEPRINTS_FILE));
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, folder.join(VOICEPRINTS_FILE))
}

/// The voiceprints saved with a meeting; empty when it has none (recorded
/// before voiceprints were kept, or with no one on the other side).
pub fn read_voiceprints(folder: &Path) -> std::io::Result<Vec<Voiceprint>> {
    match std::fs::read(folder.join(VOICEPRINTS_FILE)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_meeting_folder() {
        let folder = tempfile::tempdir().unwrap();
        assert!(read_voiceprints(folder.path()).unwrap().is_empty());
        let prints = vec![Voiceprint { label: "Speaker 1".into(), segments: 4, embedding: vec![0.6, 0.8] }];
        write_voiceprints(folder.path(), &prints).unwrap();
        assert_eq!(read_voiceprints(folder.path()).unwrap(), prints);
    }
}
