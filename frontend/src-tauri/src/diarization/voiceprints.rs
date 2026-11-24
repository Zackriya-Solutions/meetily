// diarization/voiceprints.rs
//
// Each speaker's voice at the end of a recording, saved next to the audio in
// the meeting folder. Naming a speaker later can then turn that voice into a
// profile of the person, and the file travels with the recording.
//
// A profile is the average of every voice named after that person. While
// recording, each voice heard is compared with the profiles, and a close,
// clear match becomes that voice's guessed name, kept in the same file.

use super::clustering::cosine_similarity;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub const VOICEPRINTS_FILE: &str = "speakers.json";

/// One speaker's cluster centroid (L2-normalized) and how many segments
/// built it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voiceprint {
    pub label: String,
    pub segments: usize,
    pub embedding: Vec<f32>,
    /// The person this voice sounds like, when it matched a profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One person's voice: the segment-weighted mean of their samples,
/// L2-normalized.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceProfile {
    pub name: String,
    pub embedding: Vec<f32>,
}

/// Starting points, not yet tuned on named calls; the speaker accuracy score
/// shows whether they hold. Both sides are averages over many segments, so
/// one voice scores well above the 0.55 a single segment needs to join a
/// speaker. The margin keeps two similar-sounding people from being named
/// on a coin flip.
const PROFILE_MATCH_THRESHOLD: f32 = 0.6;
const PROFILE_MATCH_MARGIN: f32 = 0.08;
/// A voice heard in fewer segments is too unsettled to name.
const MIN_SEGMENTS_TO_NAME: usize = 3;

/// The guessed name of each voice, in order. Each person names at most one
/// voice, the one closest to their profile; the rest stay anonymous.
pub fn guess_names(voices: &[Voiceprint], profiles: &[VoiceProfile]) -> Vec<Option<String>> {
    let mut candidates: Vec<(f32, usize, usize)> = Vec::new();
    for (v, voice) in voices.iter().enumerate().filter(|(_, voice)| voice.segments >= MIN_SEGMENTS_TO_NAME) {
        let mut scores: Vec<(f32, usize)> = profiles
            .iter()
            .enumerate()
            .map(|(p, profile)| (cosine_similarity(&voice.embedding, &profile.embedding), p))
            .collect();
        scores.sort_by(|a, b| b.0.total_cmp(&a.0));
        let Some(&(best, p)) = scores.first() else { continue };
        let runner_up = scores.get(1).map_or(f32::MIN, |s| s.0);
        if best >= PROFILE_MATCH_THRESHOLD && best - runner_up >= PROFILE_MATCH_MARGIN {
            candidates.push((best, v, p));
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut names = vec![None; voices.len()];
    let mut taken = vec![false; profiles.len()];
    for (_, v, p) in candidates {
        if !std::mem::replace(&mut taken[p], true) {
            names[v] = Some(profiles[p].name.clone());
        }
    }
    names
}

/// Every person's profile, from the voices named after them. Names that
/// differ only in case or spacing are one person.
pub async fn load_voice_profiles(pool: &sqlx::SqlitePool) -> Result<Vec<VoiceProfile>, sqlx::Error> {
    let rows: Vec<(String, i64, String)> =
        sqlx::query_as("SELECT name, segments, embedding FROM voice_samples ORDER BY meeting_id")
            .fetch_all(pool)
            .await?;
    let mut sums: HashMap<String, (String, Vec<f32>)> = HashMap::new();
    for (name, segments, embedding) in rows {
        let Ok(embedding) = serde_json::from_str::<Vec<f32>>(&embedding) else {
            log::warn!("Skipping an unreadable voice sample");
            continue;
        };
        let key = name.trim().to_lowercase();
        let (_, sum) = sums
            .entry(key)
            .or_insert_with(|| (name.trim().to_string(), vec![0.0; embedding.len()]));
        if sum.len() == embedding.len() {
            sum.iter_mut().zip(&embedding).for_each(|(s, e)| *s += e * segments as f32);
        }
    }
    Ok(sums
        .into_values()
        .filter_map(|(name, mut embedding)| {
            let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            (norm > 0.0).then(|| {
                embedding.iter_mut().for_each(|x| *x /= norm);
                VoiceProfile { name, embedding }
            })
        })
        .collect())
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
        let prints = vec![Voiceprint { label: "Speaker 1".into(), segments: 4, embedding: vec![0.6, 0.8], name: Some("Alex".into()) }];
        write_voiceprints(folder.path(), &prints).unwrap();
        assert_eq!(read_voiceprints(folder.path()).unwrap(), prints);
    }

    fn voice(label: &str, segments: usize, embedding: Vec<f32>) -> Voiceprint {
        Voiceprint { label: label.into(), segments, embedding, name: None }
    }

    fn profile(name: &str, embedding: Vec<f32>) -> VoiceProfile {
        VoiceProfile { name: name.into(), embedding }
    }

    #[test]
    fn names_a_voice_that_clearly_matches_one_profile() {
        let profiles = [profile("Alex", vec![1.0, 0.0, 0.0]), profile("Sam", vec![0.0, 1.0, 0.0])];
        let voices = [
            voice("Speaker 1", 5, vec![0.98, 0.2, 0.0]),
            voice("Speaker 2", 5, vec![0.0, 0.0, 1.0]),
            voice("Speaker 3", 2, vec![0.0, 1.0, 0.0]),
        ];
        assert_eq!(
            guess_names(&voices, &profiles),
            vec![Some("Alex".into()), None, None],
            "a stranger and a barely heard voice stay anonymous"
        );
    }

    #[test]
    fn leaves_a_voice_between_two_people_anonymous() {
        let profiles = [profile("Alex", vec![1.0, 0.0]), profile("Sam", vec![0.8, 0.6])];
        assert_eq!(guess_names(&[voice("Speaker 1", 9, vec![0.95, 0.31])], &profiles), vec![None]);
    }

    #[test]
    fn gives_a_name_only_to_the_closest_voice() {
        let profiles = [profile("Alex", vec![1.0, 0.0])];
        let voices = [voice("Speaker 1", 4, vec![0.8, 0.6]), voice("Speaker 2", 4, vec![0.99, 0.14])];
        assert_eq!(guess_names(&voices, &profiles), vec![None, Some("Alex".into())]);
    }

    #[tokio::test]
    async fn a_profile_is_the_weighted_mean_of_one_persons_samples() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::database::manager::ensure_voice_samples_table(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO voice_samples VALUES ('m1', 'Speaker 1', 'Alex', 3, '[1.0, 0.0]'),
             ('m2', 'Speaker 4', 'alex ', 1, '[0.0, 1.0]'), ('m2', 'Speaker 2', 'Sam', 2, '[0.0, 1.0]')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut profiles = load_voice_profiles(&pool).await.unwrap();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "Alex");
        let expected = [3.0 / 10f32.sqrt(), 1.0 / 10f32.sqrt()];
        assert!(profiles[0].embedding.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-6));
        assert_eq!(profiles[1], profile("Sam", vec![0.0, 1.0]));
    }
}
