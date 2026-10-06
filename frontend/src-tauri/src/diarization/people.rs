//! Voice matching: recognises people the user named in other meetings by their stored voices.

use crate::database::repositories::person::PeopleRepository;
use crate::database::repositories::speaker::{blob_to_embedding, NameSource, NewSpeaker, SuggestionSource};
use crate::diarization::assign::greedy_pairs;
use crate::diarization::cluster::cosine;
use sqlx::{Error as SqlxError, SqliteConnection};
use std::collections::HashSet;

/// Score at or above which a voice is linked to a person automatically (shown as "auto").
/// Provisional until set from measured same-person and different-person scores.
pub const VOICE_STRONG: f32 = 0.75;
/// Score at or above which a person is suggested for a voice.
/// Provisional until set from measured same-person and different-person scores.
pub const VOICE_WEAK: f32 = 0.60;

/// A meeting speaker as voice matching sees it.
#[derive(Debug, Clone)]
pub struct VoiceSpeaker {
    pub key: String,
    /// None for hand-made speakers (rows reassigned to a new speaker).
    pub embedding: Option<Vec<f32>>,
    pub display_name: Option<String>,
    pub person_id: Option<String>,
}

/// A person and the voices that teach it: centroids of speakers the user named.
#[derive(Debug, Clone)]
pub struct PersonVoice {
    pub person_id: String,
    pub name: String,
    pub exemplars: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStrength {
    Strong,
    Weak,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceMatch {
    pub key: String,
    pub person_id: String,
    pub name: String,
    pub score: f32,
    pub strength: MatchStrength,
}

/// Best cosine between a speaker's centroid and a person's exemplars; 0 without exemplars.
pub fn person_score(centroid: &[f32], exemplars: &[Vec<f32>]) -> f32 {
    exemplars.iter().map(|e| cosine(centroid, e)).fold(0.0, f32::max)
}

/// Suggestion reason shown to the user.
pub fn voice_reason(score: f32) -> String {
    format!("voice match {score:.2}")
}

/// Pairs ≥ weak, best first, one person per speaker and one speaker per person. Skips speakers
/// without an embedding, with a display name or a person; people already linked in the meeting;
/// rejected pairs.
pub fn assign_voices(
    speakers: &[VoiceSpeaker],
    people: &[PersonVoice],
    rejected: &HashSet<(String, String)>,
    strong: f32,
    weak: f32,
) -> Vec<VoiceMatch> {
    let linked: HashSet<&str> = speakers.iter().filter_map(|s| s.person_id.as_deref()).collect();
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for (i, s) in speakers.iter().enumerate() {
        let Some(embedding) = s.embedding.as_deref().filter(|e| !e.is_empty()) else { continue };
        if s.display_name.is_some() || s.person_id.is_some() {
            continue;
        }
        for (j, p) in people.iter().enumerate() {
            if linked.contains(p.person_id.as_str()) || rejected.contains(&(s.key.clone(), p.person_id.clone())) {
                continue;
            }
            let score = person_score(embedding, &p.exemplars);
            if score >= weak {
                pairs.push((score, i, j));
            }
        }
    }
    greedy_pairs(pairs, speakers.len(), people.len())
        .into_iter()
        .map(|(score, i, j)| VoiceMatch {
            key: speakers[i].key.clone(),
            person_id: people[j].person_id.clone(),
            name: people[j].name.clone(),
            score,
            strength: if score >= strong { MatchStrength::Strong } else { MatchStrength::Weak },
        })
        .collect()
}

/// Exemplars of every person: speakers the user named (or confirmed) that are linked to the
/// person and have a stored voice. Automatic names never teach a voice. `exclude_meeting` leaves
/// out the meeting whose speakers are being replaced.
pub async fn exemplars_conn(conn: &mut SqliteConnection, exclude_meeting: Option<&str>) -> Result<Vec<PersonVoice>, SqlxError> {
    let rows: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT p.id, p.name, ms.embedding
         FROM meeting_speakers ms
         JOIN people p ON p.id = ms.person_id
         WHERE ms.name_source = 'user' AND ms.embedding IS NOT NULL AND (? IS NULL OR ms.meeting_id <> ?)
         ORDER BY p.id",
    )
    .bind(exclude_meeting)
    .bind(exclude_meeting)
    .fetch_all(&mut *conn)
    .await?;
    let mut people: Vec<PersonVoice> = Vec::new();
    for (person_id, name, blob) in rows {
        let exemplar = blob_to_embedding(&blob);
        if exemplar.is_empty() {
            continue;
        }
        match people.last_mut() {
            Some(p) if p.person_id == person_id => p.exemplars.push(exemplar),
            _ => people.push(PersonVoice { person_id, name, exemplars: vec![exemplar] }),
        }
    }
    Ok(people)
}

/// Match a speaker write against the people named in other meetings: a strong match links the
/// speaker (name shown as auto), a weak one becomes a voice suggestion unless the speaker already
/// has a suggestion. Named speakers, rejected pairs and people already in the meeting are left
/// alone. Does nothing with `remember_voices` off. Returns (linked, suggested).
pub async fn match_new_speakers_conn(
    conn: &mut SqliteConnection,
    meeting_id: &str,
    speakers: &mut [NewSpeaker],
    remember_voices: bool,
) -> Result<(usize, usize), SqlxError> {
    if !remember_voices || speakers.is_empty() {
        return Ok((0, 0));
    }
    let people = exemplars_conn(&mut *conn, Some(meeting_id)).await?;
    if people.is_empty() {
        return Ok((0, 0));
    }
    let rejected = PeopleRepository::rejections_conn(&mut *conn, meeting_id).await?;
    let voices: Vec<VoiceSpeaker> = speakers
        .iter()
        .map(|s| VoiceSpeaker {
            key: s.key.clone(),
            embedding: Some(s.embedding.clone()),
            display_name: s.display_name.clone(),
            person_id: s.link.person_id.clone(),
        })
        .collect();
    let (mut linked, mut suggested) = (0, 0);
    for m in assign_voices(&voices, &people, &rejected, VOICE_STRONG, VOICE_WEAK) {
        let Some(s) = speakers.iter_mut().find(|s| s.key == m.key) else { continue };
        match m.strength {
            MatchStrength::Strong => {
                s.display_name = Some(m.name);
                s.link.person_id = Some(m.person_id);
                s.link.name_source = Some(NameSource::Voice);
                s.link.clear_suggestion();
                linked += 1;
            }
            MatchStrength::Weak if !s.link.has_suggestion() => {
                s.link.suggested_person_id = Some(m.person_id);
                s.link.suggested_name = Some(m.name);
                s.link.suggestion_source = Some(SuggestionSource::Voice);
                s.link.suggestion_reason = Some(voice_reason(m.score));
                suggested += 1;
            }
            MatchStrength::Weak => {}
        }
    }
    Ok((linked, suggested))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speaker(key: &str, embedding: &[f32]) -> VoiceSpeaker {
        VoiceSpeaker { key: key.into(), embedding: Some(embedding.to_vec()), display_name: None, person_id: None }
    }

    fn person(id: &str, exemplars: &[&[f32]]) -> PersonVoice {
        PersonVoice { person_id: id.into(), name: id.to_uppercase(), exemplars: exemplars.iter().map(|e| e.to_vec()).collect() }
    }

    fn none_rejected() -> HashSet<(String, String)> {
        HashSet::new()
    }

    fn pairs(matches: &[VoiceMatch]) -> Vec<(&str, &str, MatchStrength)> {
        matches.iter().map(|m| (m.key.as_str(), m.person_id.as_str(), m.strength)).collect()
    }

    #[test]
    fn score_is_max_over_exemplars() {
        let score = person_score(&[1.0, 0.0], &[vec![0.0, 1.0], vec![0.8, 0.6]]);
        assert!((score - 0.8).abs() < 1e-6, "{score}");
        assert_eq!(person_score(&[1.0, 0.0], &[]), 0.0);
    }

    #[test]
    fn different_lengths_score_zero() {
        assert_eq!(person_score(&[1.0, 0.0], &[vec![1.0, 0.0, 0.0]]), 0.0);
    }

    #[test]
    fn assignment_is_one_to_one_best_first() {
        // "a" is closest to both people; it takes p1 (1.0), so p3 goes to "b" (0.98) even though
        // "a" scores 0.99 with p3 too. "b" is listed first to show input order does not matter.
        let speakers = vec![speaker("b", &[0.95, 0.312]), speaker("a", &[1.0, 0.0])];
        let people = vec![person("p1", &[&[1.0, 0.0]]), person("p3", &[&[0.99, 0.141]])];

        let matches = assign_voices(&speakers, &people, &none_rejected(), VOICE_STRONG, VOICE_WEAK);

        assert_eq!(pairs(&matches), vec![("a", "p1", MatchStrength::Strong), ("b", "p3", MatchStrength::Strong)]);
        assert!(matches[0].score >= matches[1].score);
        assert_eq!(matches[0].name, "P1");
    }

    #[test]
    fn strong_links_weak_suggests_below_nothing() {
        let speakers = vec![
            speaker("s1", &[1.0, 0.0, 0.0]),
            speaker("s2", &[0.0, 1.0, 0.0]),
            speaker("s3", &[-1.0, 0.0, 0.0]),
        ];
        let people = vec![person("p1", &[&[1.0, 0.0, 0.0]]), person("p2", &[&[0.0, 0.7, 0.714]])];

        let matches = assign_voices(&speakers, &people, &none_rejected(), 0.75, 0.60);

        assert_eq!(pairs(&matches), vec![("s1", "p1", MatchStrength::Strong), ("s2", "p2", MatchStrength::Weak)]);
        assert!((matches[1].score - 0.70).abs() < 0.01, "{}", matches[1].score);
    }

    #[test]
    fn rejected_pairs_are_skipped() {
        let speakers = vec![speaker("s1", &[1.0, 0.0])];
        let people = vec![person("p1", &[&[1.0, 0.0]]), person("p2", &[&[0.8, 0.6]])];
        let rejected = HashSet::from([("s1".to_string(), "p1".to_string())]);

        let matches = assign_voices(&speakers, &people, &rejected, 0.75, 0.60);

        assert_eq!(pairs(&matches), vec![("s1", "p2", MatchStrength::Strong)]);
    }

    #[test]
    fn people_already_linked_in_the_meeting_are_skipped() {
        let linked = VoiceSpeaker { display_name: Some("P1".into()), person_id: Some("p1".into()), ..speaker("s0", &[0.0, 1.0]) };
        let speakers = vec![linked, speaker("s1", &[1.0, 0.0])];
        let people = vec![person("p1", &[&[1.0, 0.0]])];

        assert!(assign_voices(&speakers, &people, &none_rejected(), VOICE_STRONG, VOICE_WEAK).is_empty());
    }

    #[test]
    fn named_and_handmade_speakers_are_skipped() {
        // A name from before people existed (no person), a hand-made speaker (no voice) and an
        // empty centroid never match, however close the voice.
        let legacy = VoiceSpeaker { display_name: Some("Ana".into()), ..speaker("s0", &[1.0, 0.0]) };
        let handmade = VoiceSpeaker { embedding: None, ..speaker("s1", &[]) };
        let empty = speaker("s2", &[]);
        let people = vec![person("p1", &[&[1.0, 0.0]])];

        assert!(assign_voices(&[legacy, handmade, empty], &people, &none_rejected(), VOICE_STRONG, VOICE_WEAK).is_empty());
    }

    #[test]
    fn voice_reason_has_two_decimals() {
        assert_eq!(voice_reason(0.6789), "voice match 0.68");
        assert_eq!(voice_reason(0.7), "voice match 0.70");
    }
}
