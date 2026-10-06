//! Per-meeting speakers: names, voice centroids, merges and row reassignment.
use super::person::{clean_person_name, PeopleRepository, Person};
use super::transcript::TranscriptsRepository;
use crate::api::TranscriptSegment;
use crate::diarization::cluster::weighted_centroid;
use crate::diarization::diarizer::SpeakerCentroid;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Error as SqlxError, Row, SqliteConnection, SqlitePool};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Who gave a speaker its current name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameSource {
    /// Typed or confirmed by the user. Only these names teach a person's voice.
    User,
    /// A strong voice match to a person named in another meeting.
    Voice,
    /// Found in the conversation by the summary model and checked against the transcript.
    Conversation,
}

impl NameSource {
    pub fn as_str(self) -> &'static str {
        match self {
            NameSource::User => "user",
            NameSource::Voice => "voice",
            NameSource::Conversation => "conversation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(NameSource::User),
            "voice" => Some(NameSource::Voice),
            "conversation" => Some(NameSource::Conversation),
            _ => None,
        }
    }
}

/// Where a suggested name came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionSource {
    Voice,
    Conversation,
}

impl SuggestionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            SuggestionSource::Voice => "voice",
            SuggestionSource::Conversation => "conversation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "voice" => Some(SuggestionSource::Voice),
            "conversation" => Some(SuggestionSource::Conversation),
            _ => None,
        }
    }
}

/// A speaker's link to a person and its pending suggestion. A name without a person (typed while
/// voices are not remembered, or from before people existed) has `person_id` None.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpeakerLink {
    pub person_id: Option<String>,
    pub name_source: Option<NameSource>,
    /// The suggested person when it already exists.
    pub suggested_person_id: Option<String>,
    /// Name shown with the suggestion; always written, also for people that do not exist yet.
    pub suggested_name: Option<String>,
    pub suggestion_source: Option<SuggestionSource>,
    /// Why it is suggested, for example "voice match 0.68".
    pub suggestion_reason: Option<String>,
}

impl SpeakerLink {
    pub fn clear_suggestion(&mut self) {
        self.suggested_person_id = None;
        self.suggested_name = None;
        self.suggestion_source = None;
        self.suggestion_reason = None;
    }

    pub fn has_suggestion(&self) -> bool {
        self.suggested_person_id.is_some() || self.suggested_name.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetingSpeaker {
    pub speaker_key: String,
    pub display_name: Option<String>,
    /// Speech time from diarization; the weight used when centroids are merged.
    pub speech_seconds: f64,
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
    /// Transcript rows currently labelled with this speaker.
    pub row_count: i64,
    /// Seconds covered by those rows (the speaker's share in the speaker bar).
    pub row_seconds: f64,
    #[serde(flatten)]
    pub link: SpeakerLink,
}

#[derive(Debug, Clone, Default)]
pub struct NewSpeaker {
    pub key: String,
    pub display_name: Option<String>,
    pub embedding: Vec<f32>,
    pub speech_seconds: f64,
    pub link: SpeakerLink,
}

/// An unnamed speaker from a diarization run.
impl From<&SpeakerCentroid> for NewSpeaker {
    fn from(s: &SpeakerCentroid) -> Self {
        Self {
            key: s.key.clone(),
            display_name: None,
            embedding: s.embedding.clone(),
            speech_seconds: s.speech_seconds,
            link: SpeakerLink::default(),
        }
    }
}

/// Link columns of a `meeting_speakers` row; unknown source strings read as None.
fn link_from_row(r: &SqliteRow) -> SpeakerLink {
    SpeakerLink {
        person_id: r.get("person_id"),
        name_source: r.get::<Option<String>, _>("name_source").as_deref().and_then(NameSource::parse),
        suggested_person_id: r.get("suggested_person_id"),
        suggested_name: r.get("suggested_name"),
        suggestion_source: r
            .get::<Option<String>, _>("suggestion_source")
            .as_deref()
            .and_then(SuggestionSource::parse),
        suggestion_reason: r.get("suggestion_reason"),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SplitRow {
    pub text: String,
    pub start_s: f64,
    pub end_s: f64,
    pub speaker: String,
}

/// Everything one diarization run writes, applied atomically.
#[derive(Default)]
pub struct SpeakerWrite {
    pub speakers: Vec<NewSpeaker>,
    /// (transcript id, new speaker key or NULL)
    pub row_labels: Vec<(String, Option<String>)>,
    /// (transcript id, replacement rows)
    pub row_splits: Vec<(String, Vec<SplitRow>)>,
}

pub enum ReassignTarget {
    Existing(String),
    New,
}

fn key_index(key: &str) -> Option<usize> {
    key.strip_prefix("spk_")?.parse().ok()
}

/// Label shown to the user: the display name, or "Speaker N" (1-based).
pub fn speaker_label(key: &str, display_name: Option<&str>) -> String {
    match display_name {
        Some(name) if !name.trim().is_empty() => name.to_string(),
        _ => key_index(key)
            .map(|i| format!("Speaker {}", i + 1))
            .unwrap_or_else(|| key.to_string()),
    }
}

pub fn embedding_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn blob_to_embedding(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn not_found(what: &str) -> SqlxError {
    SqlxError::Protocol(format!("{what} not found"))
}

/// A name given by voice matching or the conversation, not yet confirmed by the user.
fn is_auto_name(link: &SpeakerLink) -> bool {
    matches!(link.name_source, Some(NameSource::Voice) | Some(NameSource::Conversation))
}

pub struct SpeakersRepository;

impl SpeakersRepository {
    pub async fn list(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<MeetingSpeaker>, SqlxError> {
        let mut conn = pool.acquire().await?;
        Self::list_conn(&mut conn, meeting_id).await
    }

    /// Speakers with row-based stats, read through `conn` so callers can read inside their own
    /// transaction.
    pub async fn list_conn(conn: &mut SqliteConnection, meeting_id: &str) -> Result<Vec<MeetingSpeaker>, SqlxError> {
        let rows = sqlx::query(
            "SELECT ms.speaker_key, ms.display_name, ms.speech_seconds, ms.embedding,
                    ms.person_id, ms.name_source, ms.suggested_person_id, ms.suggested_name,
                    ms.suggestion_source, ms.suggestion_reason,
                    COALESCE(r.row_count, 0) AS row_count,
                    CAST(COALESCE(r.row_seconds, 0) AS REAL) AS row_seconds
             FROM meeting_speakers ms
             LEFT JOIN (
                 SELECT speaker,
                        COUNT(*) AS row_count,
                        SUM(COALESCE(audio_end_time - audio_start_time, duration, 0)) AS row_seconds
                 FROM transcripts
                 WHERE meeting_id = ?
                 GROUP BY speaker
             ) r ON r.speaker = ms.speaker_key
             WHERE ms.meeting_id = ?",
        )
        .bind(meeting_id)
        .bind(meeting_id)
        .fetch_all(&mut *conn)
        .await?;
        let mut speakers: Vec<MeetingSpeaker> = rows
            .into_iter()
            .map(|r| MeetingSpeaker {
                speaker_key: r.get("speaker_key"),
                display_name: r.get("display_name"),
                speech_seconds: r.get("speech_seconds"),
                embedding: r.get::<Option<Vec<u8>>, _>("embedding").map(|b| blob_to_embedding(&b)),
                row_count: r.get("row_count"),
                row_seconds: r.get("row_seconds"),
                link: link_from_row(&r),
            })
            .collect();
        speakers.sort_by_key(|s| (key_index(&s.speaker_key).unwrap_or(usize::MAX), s.speaker_key.clone()));
        Ok(speakers)
    }

    /// Speaker key → label shown to the user.
    pub async fn labels(pool: &SqlitePool, meeting_id: &str) -> Result<BTreeMap<String, String>, SqlxError> {
        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT speaker_key, display_name FROM meeting_speakers WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_all(pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(key, name)| {
                let label = speaker_label(&key, name.as_deref());
                (key, label)
            })
            .collect())
    }

    /// Rename a speaker for the whole meeting. An empty name resets to the default label.
    pub async fn rename(pool: &SqlitePool, meeting_id: &str, key: &str, name: &str) -> Result<(), SqlxError> {
        let trimmed = name.trim();
        let value = if trimmed.is_empty() { None } else { Some(trimmed) };
        let result = sqlx::query("UPDATE meeting_speakers SET display_name = ? WHERE meeting_id = ? AND speaker_key = ?")
            .bind(value)
            .bind(meeting_id)
            .bind(key)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(not_found("speaker"));
        }
        Ok(())
    }

    /// Name, link and suggestion of one speaker, read through `conn`.
    async fn name_state_conn(
        conn: &mut SqliteConnection,
        meeting_id: &str,
        key: &str,
    ) -> Result<(Option<String>, SpeakerLink), SqlxError> {
        let row = sqlx::query(
            "SELECT display_name, person_id, name_source, suggested_person_id, suggested_name, suggestion_source,
                    suggestion_reason
             FROM meeting_speakers WHERE meeting_id = ? AND speaker_key = ?",
        )
        .bind(meeting_id)
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| not_found("speaker"))?;
        Ok((row.get("display_name"), link_from_row(&row)))
    }

    /// A name typed or confirmed by the user: `name_source = 'user'`, suggestion cleared.
    async fn set_user_name_conn(
        conn: &mut SqliteConnection,
        meeting_id: &str,
        key: &str,
        name: &str,
        person_id: Option<&str>,
    ) -> Result<(), SqlxError> {
        sqlx::query(
            "UPDATE meeting_speakers SET display_name = ?, person_id = ?, name_source = 'user',
                 suggested_person_id = NULL, suggested_name = NULL, suggestion_source = NULL, suggestion_reason = NULL
             WHERE meeting_id = ? AND speaker_key = ?",
        )
        .bind(name)
        .bind(person_id)
        .bind(meeting_id)
        .bind(key)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// The person behind a name: `person_id` while it exists, else the person called `name`
    /// (created when new).
    async fn resolve_person_conn(
        conn: &mut SqliteConnection,
        person_id: Option<&str>,
        name: &str,
    ) -> Result<Person, SqlxError> {
        if let Some(id) = person_id {
            if let Some(person) = PeopleRepository::get_conn(&mut *conn, id).await? {
                return Ok(person);
            }
        }
        PeopleRepository::find_or_create_conn(&mut *conn, name).await
    }

    /// Typed name. An existing person's name (any case, extra spaces ignored) links that person,
    /// a new name creates one, an empty name unlinks the speaker. With `remember_voices` off the
    /// name is stored as text only. Returns the linked person id.
    pub async fn name(
        pool: &SqlitePool,
        meeting_id: &str,
        key: &str,
        name: &str,
        remember_voices: bool,
    ) -> Result<Option<String>, SqlxError> {
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;
        Self::name_state_conn(&mut tx, meeting_id, key).await?;
        let person_id = match clean_person_name(name) {
            None => {
                sqlx::query(
                    "UPDATE meeting_speakers SET display_name = NULL, person_id = NULL, name_source = NULL
                     WHERE meeting_id = ? AND speaker_key = ?",
                )
                .bind(meeting_id)
                .bind(key)
                .execute(&mut *tx)
                .await?;
                None
            }
            Some(cleaned) if remember_voices => {
                let person = PeopleRepository::find_or_create_conn(&mut tx, &cleaned).await?;
                Self::set_user_name_conn(&mut tx, meeting_id, key, &person.name, Some(&person.id)).await?;
                // Typing a name the user once rejected for this speaker overrides that rejection.
                PeopleRepository::remove_rejection_conn(&mut tx, meeting_id, key, &person.id).await?;
                Some(person.id)
            }
            Some(cleaned) => {
                Self::set_user_name_conn(&mut tx, meeting_id, key, &cleaned, None).await?;
                None
            }
        };
        tx.commit().await?;
        Ok(person_id)
    }

    /// The auto name, else the suggestion, becomes a name typed by the user (so it teaches the
    /// voice). Returns the linked person id; None with `remember_voices` off.
    pub async fn confirm(
        pool: &SqlitePool,
        meeting_id: &str,
        key: &str,
        remember_voices: bool,
    ) -> Result<Option<String>, SqlxError> {
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;
        let (display_name, link) = Self::name_state_conn(&mut tx, meeting_id, key).await?;
        let (person_id, name) = match display_name.filter(|_| is_auto_name(&link)) {
            Some(name) => (link.person_id.clone(), name),
            None => match link.suggested_name.clone() {
                Some(name) => (link.suggested_person_id.clone(), name),
                None => return Err(not_found("name to confirm")),
            },
        };
        let person = if remember_voices {
            Some(Self::resolve_person_conn(&mut tx, person_id.as_deref(), &name).await?)
        } else {
            None
        };
        let stored = person.as_ref().map_or(name.as_str(), |p| p.name.as_str());
        Self::set_user_name_conn(&mut tx, meeting_id, key, stored, person.as_ref().map(|p| p.id.as_str())).await?;
        if let Some(p) = &person {
            PeopleRepository::remove_rejection_conn(&mut tx, meeting_id, key, &p.id).await?;
        }
        tx.commit().await?;
        Ok(person.map(|p| p.id))
    }

    /// "Not <name>": records the rejection and clears the auto name, else the suggestion. A name
    /// without a person gets one, so the rejection has a stable id.
    pub async fn reject(pool: &SqlitePool, meeting_id: &str, key: &str) -> Result<(), SqlxError> {
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;
        let (display_name, link) = Self::name_state_conn(&mut tx, meeting_id, key).await?;
        if let Some(name) = display_name.filter(|_| is_auto_name(&link)) {
            let person = Self::resolve_person_conn(&mut tx, link.person_id.as_deref(), &name).await?;
            PeopleRepository::add_rejection_conn(&mut tx, meeting_id, key, &person.id).await?;
            sqlx::query(
                "UPDATE meeting_speakers SET display_name = NULL, person_id = NULL, name_source = NULL
                 WHERE meeting_id = ? AND speaker_key = ?",
            )
            .bind(meeting_id)
            .bind(key)
            .execute(&mut *tx)
            .await?;
        } else if let Some(name) = link.suggested_name.clone() {
            let person = Self::resolve_person_conn(&mut tx, link.suggested_person_id.as_deref(), &name).await?;
            PeopleRepository::add_rejection_conn(&mut tx, meeting_id, key, &person.id).await?;
            sqlx::query(
                "UPDATE meeting_speakers SET suggested_person_id = NULL, suggested_name = NULL, suggestion_source = NULL,
                     suggestion_reason = NULL
                 WHERE meeting_id = ? AND speaker_key = ?",
            )
            .bind(meeting_id)
            .bind(key)
            .execute(&mut *tx)
            .await?;
        } else {
            return Err(not_found("name to reject"));
        }
        tx.commit().await
    }

    /// Fold `from` into `into`: rows and rejections move, centroids combine weighted by speech
    /// time, `into` keeps its name and person link (or takes `from`'s when it has no name) and its
    /// suggestion (or takes `from`'s). The suggestion goes when the merged name was typed by the
    /// user or names the merged person, and no rejection of the merged person stays on `into`.
    pub async fn merge(pool: &SqlitePool, meeting_id: &str, from: &str, into: &str) -> Result<(), SqlxError> {
        if from == into {
            return Err(SqlxError::Protocol("cannot merge a speaker into itself".into()));
        }
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;
        let speakers = Self::list_conn(&mut tx, meeting_id).await?;
        let a = speakers.iter().find(|s| s.speaker_key == from).ok_or_else(|| not_found("speaker"))?;
        let b = speakers.iter().find(|s| s.speaker_key == into).ok_or_else(|| not_found("speaker"))?;

        let merged = match (&a.embedding, &b.embedding) {
            (Some(ea), Some(eb)) => Some(weighted_centroid(
                &[ea.as_slice(), eb.as_slice()],
                &[a.speech_seconds, b.speech_seconds],
            )),
            (None, Some(e)) | (Some(e), None) => Some(e.clone()),
            (None, None) => None,
        };

        sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND speaker = ?")
            .bind(into)
            .bind(meeting_id)
            .bind(from)
            .execute(&mut *tx)
            .await?;
        let (name, link) = if b.display_name.is_some() { (&b.display_name, &b.link) } else { (&a.display_name, &a.link) };
        let suggestion = if b.link.has_suggestion() { &b.link } else { &a.link };
        let own_person = suggestion.suggested_person_id.is_some() && suggestion.suggested_person_id == link.person_id;
        let suggestion = if link.name_source == Some(NameSource::User) || own_person {
            SpeakerLink::default()
        } else {
            suggestion.clone()
        };
        sqlx::query(
            "UPDATE meeting_speakers SET embedding = ?, speech_seconds = ?, display_name = ?, person_id = ?, name_source = ?,
                 suggested_person_id = ?, suggested_name = ?, suggestion_source = ?, suggestion_reason = ?
             WHERE meeting_id = ? AND speaker_key = ?",
        )
        .bind(merged.as_deref().map(embedding_to_blob))
        .bind(a.speech_seconds + b.speech_seconds)
        .bind(name.as_deref())
        .bind(link.person_id.as_deref())
        .bind(link.name_source.map(NameSource::as_str))
        .bind(suggestion.suggested_person_id.as_deref())
        .bind(suggestion.suggested_name.as_deref())
        .bind(suggestion.suggestion_source.map(SuggestionSource::as_str))
        .bind(suggestion.suggestion_reason.as_deref())
        .bind(meeting_id)
        .bind(into)
        .execute(&mut *tx)
        .await?;
        // A speaker never rejects its own person: `from`'s rejection of the merged person is not
        // moved, and `into`'s is lifted.
        sqlx::query(
            "INSERT OR IGNORE INTO speaker_rejections (meeting_id, speaker_key, person_id)
             SELECT meeting_id, ?, person_id FROM speaker_rejections
             WHERE meeting_id = ? AND speaker_key = ? AND person_id IS NOT ?",
        )
        .bind(into)
        .bind(meeting_id)
        .bind(from)
        .bind(link.person_id.as_deref())
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM speaker_rejections WHERE meeting_id = ? AND speaker_key = ?")
            .bind(meeting_id)
            .bind(from)
            .execute(&mut *tx)
            .await?;
        if let Some(person_id) = link.person_id.as_deref() {
            PeopleRepository::remove_rejection_conn(&mut tx, meeting_id, into, person_id).await?;
        }
        sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ? AND speaker_key = ?")
            .bind(meeting_id)
            .bind(from)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    /// Change who said one row. Returns the key the row now has. A hand-made speaker (no voice
    /// centroid) is removed once no row uses it; voiced speakers stay for name carry-over.
    pub async fn reassign_row(
        pool: &SqlitePool,
        meeting_id: &str,
        transcript_id: &str,
        target: ReassignTarget,
    ) -> Result<String, SqlxError> {
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;
        let previous: Option<Option<String>> =
            sqlx::query_scalar("SELECT speaker FROM transcripts WHERE meeting_id = ? AND id = ?")
                .bind(meeting_id)
                .bind(transcript_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(previous) = previous else {
            return Err(not_found("transcript"));
        };
        let key = match target {
            ReassignTarget::Existing(key) => {
                let exists: Option<i64> =
                    sqlx::query_scalar("SELECT 1 FROM meeting_speakers WHERE meeting_id = ? AND speaker_key = ?")
                        .bind(meeting_id)
                        .bind(&key)
                        .fetch_optional(&mut *tx)
                        .await?;
                if exists.is_none() {
                    return Err(not_found("speaker"));
                }
                key
            }
            ReassignTarget::New => {
                let keys: Vec<String> = sqlx::query_scalar("SELECT speaker_key FROM meeting_speakers WHERE meeting_id = ?")
                    .bind(meeting_id)
                    .fetch_all(&mut *tx)
                    .await?;
                let used: Vec<Option<String>> =
                    sqlx::query_scalar("SELECT DISTINCT speaker FROM transcripts WHERE meeting_id = ?")
                        .bind(meeting_id)
                        .fetch_all(&mut *tx)
                        .await?;
                let next = keys
                    .into_iter()
                    .chain(used.into_iter().flatten())
                    .filter_map(|k| key_index(&k))
                    .max()
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let key = format!("spk_{next}");
                sqlx::query("INSERT INTO meeting_speakers (meeting_id, speaker_key, created_at) VALUES (?, ?, ?)")
                    .bind(meeting_id)
                    .bind(&key)
                    .bind(chrono::Utc::now().to_rfc3339())
                    .execute(&mut *tx)
                    .await?;
                key
            }
        };
        sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND id = ?")
            .bind(&key)
            .bind(meeting_id)
            .bind(transcript_id)
            .execute(&mut *tx)
            .await?;
        if let Some(previous) = previous.filter(|p| *p != key) {
            sqlx::query(
                "DELETE FROM meeting_speakers
                 WHERE meeting_id = ? AND speaker_key = ? AND embedding IS NULL
                   AND NOT EXISTS (SELECT 1 FROM transcripts WHERE meeting_id = ? AND speaker = ?)",
            )
            .bind(meeting_id)
            .bind(&previous)
            .bind(meeting_id)
            .bind(&previous)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(key)
    }

    /// Replace the meeting's speakers and apply row labels and splits.
    /// Call inside the caller's transaction so a run is written atomically.
    pub async fn replace_for_meeting(
        conn: &mut SqliteConnection,
        meeting_id: &str,
        write: &SpeakerWrite,
    ) -> Result<(), SqlxError> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(&mut *conn)
            .await?;
        for s in &write.speakers {
            sqlx::query(
                "INSERT INTO meeting_speakers (meeting_id, speaker_key, display_name, embedding, speech_seconds, created_at,
                     person_id, name_source, suggested_person_id, suggested_name, suggestion_source, suggestion_reason)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(meeting_id)
            .bind(&s.key)
            .bind(&s.display_name)
            .bind(embedding_to_blob(&s.embedding))
            .bind(s.speech_seconds)
            .bind(&now)
            .bind(&s.link.person_id)
            .bind(s.link.name_source.map(NameSource::as_str))
            .bind(&s.link.suggested_person_id)
            .bind(&s.link.suggested_name)
            .bind(s.link.suggestion_source.map(SuggestionSource::as_str))
            .bind(&s.link.suggestion_reason)
            .execute(&mut *conn)
            .await?;
        }
        for (id, key) in &write.row_labels {
            sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND id = ?")
                .bind(key)
                .bind(meeting_id)
                .bind(id)
                .execute(&mut *conn)
                .await?;
        }
        for (id, pieces) in &write.row_splits {
            let timestamp: Option<String> =
                sqlx::query_scalar("SELECT timestamp FROM transcripts WHERE meeting_id = ? AND id = ?")
                    .bind(meeting_id)
                    .bind(id)
                    .fetch_optional(&mut *conn)
                    .await?;
            let Some(timestamp) = timestamp else { continue };
            sqlx::query("DELETE FROM transcripts WHERE meeting_id = ? AND id = ?")
                .bind(meeting_id)
                .bind(id)
                .execute(&mut *conn)
                .await?;
            for piece in pieces {
                let row = TranscriptSegment {
                    id: format!("transcript-{}", Uuid::new_v4()),
                    text: piece.text.clone(),
                    timestamp: timestamp.clone(),
                    audio_start_time: Some(piece.start_s),
                    audio_end_time: Some(piece.end_s),
                    duration: Some(piece.end_s - piece.start_s),
                    speaker: Some(piece.speaker.clone()),
                };
                TranscriptsRepository::insert_row(&mut *conn, &row.id, meeting_id, &row).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_support::{migrated_pool, seed_meeting, seed_person, SeedRow};

    const M: &str = "meeting-1";

    async fn seeded() -> SqlitePool {
        let pool = migrated_pool().await;
        seed_meeting(
            &pool,
            M,
            &[
                SeedRow { id: "t1", start: Some(0.0), end: Some(2.0), speaker: Some("spk_0"), text: "hello" },
                SeedRow { id: "t2", start: Some(2.0), end: Some(4.0), speaker: Some("spk_1"), text: "hi" },
                SeedRow { id: "t3", start: Some(4.0), end: Some(6.0), speaker: Some("spk_1"), text: "bye" },
            ],
        )
        .await;
        let mut conn = pool.acquire().await.unwrap();
        SpeakersRepository::replace_for_meeting(
            &mut conn,
            M,
            &SpeakerWrite {
                speakers: vec![
                    NewSpeaker { key: "spk_0".into(), display_name: None, embedding: vec![1.0, 0.0], speech_seconds: 3.0, ..Default::default() },
                    NewSpeaker { key: "spk_1".into(), display_name: Some("Ana".into()), embedding: vec![0.0, 1.0], speech_seconds: 1.0, ..Default::default() },
                ],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        drop(conn);
        pool
    }

    async fn speaker_of(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar("SELECT speaker FROM transcripts WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    use crate::database::repositories::person::PeopleRepository;
    use std::collections::HashSet;

    async fn one(pool: &SqlitePool, key: &str) -> MeetingSpeaker {
        SpeakersRepository::list(pool, M)
            .await
            .unwrap()
            .into_iter()
            .find(|s| s.speaker_key == key)
            .expect("speaker exists")
    }

    async fn write(pool: &SqlitePool, speakers: Vec<NewSpeaker>) {
        let mut conn = pool.acquire().await.unwrap();
        SpeakersRepository::replace_for_meeting(&mut conn, M, &SpeakerWrite { speakers, ..Default::default() })
            .await
            .unwrap();
    }

    async fn rejected(pool: &SqlitePool) -> HashSet<(String, String)> {
        let mut conn = pool.acquire().await.unwrap();
        PeopleRepository::rejections_conn(&mut conn, M).await.unwrap()
    }

    async fn people_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM people").fetch_one(pool).await.unwrap()
    }

    /// spk_0 named Noah by a voice match.
    fn auto_noah() -> NewSpeaker {
        NewSpeaker {
            key: "spk_0".into(),
            display_name: Some("Noah".into()),
            embedding: vec![1.0, 0.0],
            speech_seconds: 3.0,
            link: SpeakerLink {
                person_id: Some("person-noah".into()),
                name_source: Some(NameSource::Voice),
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn naming_links_to_existing_person_case_insensitively() {
        let pool = seeded().await;
        seed_person(&pool, "person-noah", "Noah").await;

        let person = SpeakersRepository::name(&pool, M, "spk_0", " noah ", true).await.unwrap();

        assert_eq!(person.as_deref(), Some("person-noah"));
        let s = one(&pool, "spk_0").await;
        assert_eq!(s.display_name.as_deref(), Some("Noah"), "the stored spelling is used");
        assert_eq!(s.link.person_id.as_deref(), Some("person-noah"));
        assert_eq!(s.link.name_source, Some(NameSource::User));
        assert_eq!(people_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn naming_a_new_name_creates_a_person() {
        let pool = seeded().await;
        let person = SpeakersRepository::name(&pool, M, "spk_0", "Sam", true).await.unwrap().expect("linked");
        assert!(person.starts_with("person-"));
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(PeopleRepository::get_conn(&mut conn, &person).await.unwrap().unwrap().name, "Sam");
        drop(conn);
        assert_eq!(one(&pool, "spk_0").await.link.person_id.as_deref(), Some(person.as_str()));
    }

    #[tokio::test]
    async fn clearing_the_name_unlinks() {
        let pool = seeded().await;
        SpeakersRepository::name(&pool, M, "spk_0", "Sam", true).await.unwrap();
        let person = SpeakersRepository::name(&pool, M, "spk_0", "   ", true).await.unwrap();
        assert_eq!(person, None);
        let s = one(&pool, "spk_0").await;
        assert_eq!(s.display_name, None);
        assert_eq!(s.link.person_id, None);
        assert_eq!(s.link.name_source, None);
    }

    #[tokio::test]
    async fn naming_with_remember_off_stores_text_only() {
        let pool = seeded().await;
        let person = SpeakersRepository::name(&pool, M, "spk_0", " Sam  Lee ", false).await.unwrap();
        assert_eq!(person, None);
        let s = one(&pool, "spk_0").await;
        assert_eq!(s.display_name.as_deref(), Some("Sam Lee"));
        assert_eq!(s.link.person_id, None);
        assert_eq!(s.link.name_source, Some(NameSource::User));
        assert_eq!(people_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn typing_a_rejected_name_lifts_the_rejection() {
        let pool = seeded().await;
        seed_person(&pool, "person-noah", "Noah").await;
        let mut conn = pool.acquire().await.unwrap();
        PeopleRepository::add_rejection_conn(&mut conn, M, "spk_0", "person-noah").await.unwrap();
        drop(conn);

        SpeakersRepository::name(&pool, M, "spk_0", "Noah", true).await.unwrap();

        assert!(rejected(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn confirm_turns_an_auto_name_into_user() {
        let pool = seeded().await;
        seed_person(&pool, "person-noah", "Noah").await;
        write(&pool, vec![auto_noah()]).await;

        let person = SpeakersRepository::confirm(&pool, M, "spk_0", true).await.unwrap();

        assert_eq!(person.as_deref(), Some("person-noah"));
        let s = one(&pool, "spk_0").await;
        assert_eq!(s.display_name.as_deref(), Some("Noah"));
        assert_eq!(s.link.name_source, Some(NameSource::User));
    }

    #[tokio::test]
    async fn confirm_takes_a_suggestion_and_creates_its_person() {
        let pool = seeded().await;
        write(
            &pool,
            vec![NewSpeaker {
                key: "spk_0".into(),
                embedding: vec![1.0, 0.0],
                speech_seconds: 3.0,
                link: SpeakerLink {
                    suggested_name: Some("Priya".into()),
                    suggestion_source: Some(SuggestionSource::Conversation),
                    suggestion_reason: Some("addressed as Priya at 00:42".into()),
                    ..Default::default()
                },
                ..Default::default()
            }],
        )
        .await;

        let person = SpeakersRepository::confirm(&pool, M, "spk_0", true).await.unwrap().expect("linked");

        let s = one(&pool, "spk_0").await;
        assert_eq!(s.display_name.as_deref(), Some("Priya"));
        assert_eq!(s.link.person_id.as_deref(), Some(person.as_str()));
        assert_eq!(s.link.name_source, Some(NameSource::User));
        assert!(!s.link.has_suggestion());
        assert_eq!(people_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn confirm_without_a_name_is_an_error() {
        let pool = seeded().await;
        // spk_0 has no name and no suggestion; spk_1 "Ana" predates people (no source).
        assert!(SpeakersRepository::confirm(&pool, M, "spk_0", true).await.is_err());
        assert!(SpeakersRepository::confirm(&pool, M, "spk_1", true).await.is_err());
        assert!(SpeakersRepository::confirm(&pool, M, "spk_9", true).await.is_err());
        assert_eq!(one(&pool, "spk_1").await.link.name_source, None);
    }

    #[tokio::test]
    async fn reject_auto_name_records_rejection_and_clears_it() {
        let pool = seeded().await;
        seed_person(&pool, "person-noah", "Noah").await;
        write(&pool, vec![auto_noah()]).await;

        SpeakersRepository::reject(&pool, M, "spk_0").await.unwrap();

        let s = one(&pool, "spk_0").await;
        assert_eq!(s.display_name, None);
        assert_eq!(s.link, SpeakerLink::default());
        assert_eq!(rejected(&pool).await, HashSet::from([("spk_0".to_string(), "person-noah".to_string())]));
    }

    #[tokio::test]
    async fn reject_suggestion_keeps_the_existing_name() {
        let pool = seeded().await;
        seed_person(&pool, "person-noah", "Noah").await;
        write(
            &pool,
            vec![NewSpeaker {
                key: "spk_1".into(),
                display_name: Some("Ana".into()),
                embedding: vec![0.0, 1.0],
                speech_seconds: 1.0,
                link: SpeakerLink {
                    suggested_person_id: Some("person-noah".into()),
                    suggested_name: Some("Noah".into()),
                    suggestion_source: Some(SuggestionSource::Voice),
                    suggestion_reason: Some("voice match 0.68".into()),
                    ..Default::default()
                },
            }],
        )
        .await;

        SpeakersRepository::reject(&pool, M, "spk_1").await.unwrap();

        let s = one(&pool, "spk_1").await;
        assert_eq!(s.display_name.as_deref(), Some("Ana"));
        assert!(!s.link.has_suggestion());
        assert_eq!(rejected(&pool).await, HashSet::from([("spk_1".to_string(), "person-noah".to_string())]));
        assert!(SpeakersRepository::reject(&pool, M, "spk_1").await.is_err(), "nothing left to reject");
    }

    #[tokio::test]
    async fn merging_meeting_speakers_keeps_link_and_moves_rejections() {
        let pool = seeded().await;
        seed_person(&pool, "person-noah", "Noah").await;
        let ana = SpeakersRepository::name(&pool, M, "spk_1", "Ana", true).await.unwrap().expect("linked");
        let mut conn = pool.acquire().await.unwrap();
        PeopleRepository::add_rejection_conn(&mut conn, M, "spk_1", "person-noah").await.unwrap();
        drop(conn);

        SpeakersRepository::merge(&pool, M, "spk_1", "spk_0").await.unwrap();

        let s = one(&pool, "spk_0").await;
        assert_eq!(s.display_name.as_deref(), Some("Ana"));
        assert_eq!(s.link.person_id.as_deref(), Some(ana.as_str()));
        assert_eq!(s.link.name_source, Some(NameSource::User));
        assert_eq!(rejected(&pool).await, HashSet::from([("spk_0".to_string(), "person-noah".to_string())]));
    }

    #[tokio::test]
    async fn merging_into_a_typed_name_drops_the_suggestion() {
        let pool = seeded().await;
        SpeakersRepository::name(&pool, M, "spk_0", "Noah", true).await.unwrap();
        sqlx::query(
            "UPDATE meeting_speakers SET suggested_name = 'Priya', suggestion_source = 'conversation',
                 suggestion_reason = 'addressed as Priya at 00:42'
             WHERE meeting_id = ? AND speaker_key = 'spk_1'",
        )
        .bind(M)
        .execute(&pool)
        .await
        .unwrap();

        SpeakersRepository::merge(&pool, M, "spk_1", "spk_0").await.unwrap();

        let s = one(&pool, "spk_0").await;
        assert_eq!(s.display_name.as_deref(), Some("Noah"));
        assert_eq!(s.link.name_source, Some(NameSource::User));
        assert!(!s.link.has_suggestion());
    }

    #[tokio::test]
    async fn merging_drops_the_suggestion_and_rejection_of_the_merged_person() {
        let pool = seeded().await;
        seed_person(&pool, "person-noah", "Noah").await;
        write(
            &pool,
            vec![
                auto_noah(),
                NewSpeaker {
                    key: "spk_1".into(),
                    embedding: vec![0.0, 1.0],
                    speech_seconds: 1.0,
                    link: SpeakerLink {
                        suggested_person_id: Some("person-noah".into()),
                        suggested_name: Some("Noah".into()),
                        suggestion_source: Some(SuggestionSource::Voice),
                        suggestion_reason: Some("voice match 0.68".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
        )
        .await;
        let mut conn = pool.acquire().await.unwrap();
        PeopleRepository::add_rejection_conn(&mut conn, M, "spk_1", "person-noah").await.unwrap();
        drop(conn);

        SpeakersRepository::merge(&pool, M, "spk_1", "spk_0").await.unwrap();

        let s = one(&pool, "spk_0").await;
        assert_eq!(s.link.person_id.as_deref(), Some("person-noah"));
        assert_eq!(s.link.name_source, Some(NameSource::Voice));
        assert!(!s.link.has_suggestion(), "a suggestion of its own person says nothing");
        assert!(rejected(&pool).await.is_empty(), "spk_0 is Noah, so it cannot reject Noah");
    }

    #[tokio::test]
    async fn merging_lifts_the_targets_rejection_of_its_new_person() {
        let pool = seeded().await;
        seed_person(&pool, "person-noah", "Noah").await;
        SpeakersRepository::name(&pool, M, "spk_1", "Noah", true).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        PeopleRepository::add_rejection_conn(&mut conn, M, "spk_0", "person-noah").await.unwrap();
        drop(conn);

        // spk_0 is unnamed, so it takes spk_1's name and link.
        SpeakersRepository::merge(&pool, M, "spk_1", "spk_0").await.unwrap();

        assert_eq!(one(&pool, "spk_0").await.link.person_id.as_deref(), Some("person-noah"));
        assert!(rejected(&pool).await.is_empty());
    }

    #[test]
    fn default_labels_are_one_based() {
        assert_eq!(speaker_label("spk_0", None), "Speaker 1");
        assert_eq!(speaker_label("spk_4", None), "Speaker 5");
        assert_eq!(speaker_label("spk_0", Some("Noah")), "Noah");
        assert_eq!(speaker_label("weird", None), "weird");
    }

    #[test]
    fn embedding_blob_round_trips() {
        let v = vec![0.25f32, -1.5, 3.0];
        assert_eq!(blob_to_embedding(&embedding_to_blob(&v)), v);
    }

    #[tokio::test]
    async fn list_returns_speakers_in_key_order_with_embeddings() {
        let pool = seeded().await;
        let speakers = SpeakersRepository::list(&pool, M).await.unwrap();
        assert_eq!(speakers.len(), 2);
        assert_eq!(speakers[0].speaker_key, "spk_0");
        assert_eq!(speakers[1].display_name.as_deref(), Some("Ana"));
        assert_eq!(speakers[1].embedding.as_deref(), Some(&[0.0f32, 1.0][..]));
        assert_eq!((speakers[0].row_count, speakers[1].row_count), (1, 2));
        assert_eq!(speakers[1].row_seconds, 4.0);
    }

    #[tokio::test]
    async fn rename_trims_and_empty_resets_to_default() {
        let pool = seeded().await;
        SpeakersRepository::rename(&pool, M, "spk_0", "  Noah ").await.unwrap();
        assert_eq!(SpeakersRepository::list(&pool, M).await.unwrap()[0].display_name.as_deref(), Some("Noah"));
        SpeakersRepository::rename(&pool, M, "spk_0", "   ").await.unwrap();
        assert_eq!(SpeakersRepository::list(&pool, M).await.unwrap()[0].display_name, None);
        assert!(SpeakersRepository::rename(&pool, M, "spk_9", "X").await.is_err());
    }

    #[tokio::test]
    async fn merge_moves_rows_and_combines_centroids() {
        let pool = seeded().await;
        SpeakersRepository::merge(&pool, M, "spk_1", "spk_0").await.unwrap();

        assert_eq!(speaker_of(&pool, "t2").await.as_deref(), Some("spk_0"));
        assert_eq!(speaker_of(&pool, "t3").await.as_deref(), Some("spk_0"));
        let speakers = SpeakersRepository::list(&pool, M).await.unwrap();
        assert_eq!(speakers.len(), 1);
        assert_eq!(speakers[0].speech_seconds, 4.0);
        let e = speakers[0].embedding.clone().unwrap();
        assert!(e[0] > e[1], "centroid leans towards the 3 s speaker");
        assert_eq!(speakers[0].display_name.as_deref(), Some("Ana"), "the unnamed target takes the merged speaker's name");
        assert_eq!((speakers[0].row_count, speakers[0].row_seconds), (3, 6.0));
    }

    #[tokio::test]
    async fn merge_keeps_the_target_name_when_both_are_named() {
        let pool = seeded().await;
        SpeakersRepository::rename(&pool, M, "spk_0", "Noah").await.unwrap();
        SpeakersRepository::merge(&pool, M, "spk_1", "spk_0").await.unwrap();
        let speakers = SpeakersRepository::list(&pool, M).await.unwrap();
        assert_eq!(speakers.len(), 1);
        assert_eq!(speakers[0].display_name.as_deref(), Some("Noah"));
    }

    #[tokio::test]
    async fn merge_into_self_is_rejected() {
        let pool = seeded().await;
        assert!(SpeakersRepository::merge(&pool, M, "spk_0", "spk_0").await.is_err());
        assert!(SpeakersRepository::merge(&pool, M, "spk_7", "spk_0").await.is_err());
    }

    #[tokio::test]
    async fn reassign_to_existing_and_new_speaker() {
        let pool = seeded().await;
        let key = SpeakersRepository::reassign_row(&pool, M, "t1", ReassignTarget::Existing("spk_1".into())).await.unwrap();
        assert_eq!(key, "spk_1");
        assert_eq!(speaker_of(&pool, "t1").await.as_deref(), Some("spk_1"));

        let new_key = SpeakersRepository::reassign_row(&pool, M, "t2", ReassignTarget::New).await.unwrap();
        assert_eq!(new_key, "spk_2");
        assert_eq!(speaker_of(&pool, "t2").await.as_deref(), Some("spk_2"));
        let speakers = SpeakersRepository::list(&pool, M).await.unwrap();
        assert_eq!(speakers.last().unwrap().speaker_key, "spk_2");
        assert_eq!(speakers.last().unwrap().embedding, None);

        // Rows now: t1 spk_1, t2 spk_2, t3 spk_1 (2 s each); spk_0 has no rows but keeps its voice.
        let stats: Vec<(String, i64, f64)> = SpeakersRepository::list(&pool, M)
            .await
            .unwrap()
            .into_iter()
            .map(|s| (s.speaker_key, s.row_count, s.row_seconds))
            .collect();
        assert_eq!(
            stats,
            vec![("spk_0".to_string(), 0, 0.0), ("spk_1".to_string(), 2, 4.0), ("spk_2".to_string(), 1, 2.0)]
        );
        // Moving the only row off a hand-made speaker removes that speaker.
        SpeakersRepository::reassign_row(&pool, M, "t2", ReassignTarget::Existing("spk_1".into())).await.unwrap();
        let keys: Vec<String> = SpeakersRepository::list(&pool, M).await.unwrap().into_iter().map(|s| s.speaker_key).collect();
        assert_eq!(keys, vec!["spk_0".to_string(), "spk_1".to_string()]);

        assert!(SpeakersRepository::reassign_row(&pool, M, "t1", ReassignTarget::Existing("spk_9".into())).await.is_err());
    }

    #[tokio::test]
    async fn replace_updates_labels_and_splits_rows() {
        let pool = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        SpeakersRepository::replace_for_meeting(
            &mut conn,
            M,
            &SpeakerWrite {
                speakers: vec![NewSpeaker { key: "spk_0".into(), display_name: Some("Noah".into()), embedding: vec![1.0, 0.0], speech_seconds: 6.0, ..Default::default() }],
                row_labels: vec![("t1".into(), Some("spk_0".into())), ("t2".into(), None)],
                row_splits: vec![(
                    "t3".into(),
                    vec![
                        SplitRow { text: "by".into(), start_s: 4.0, end_s: 5.0, speaker: "spk_0".into() },
                        SplitRow { text: "e".into(), start_s: 5.0, end_s: 6.0, speaker: "spk_0".into() },
                    ],
                )],
            },
        )
        .await
        .unwrap();
        drop(conn);

        assert_eq!(speaker_of(&pool, "t2").await, None);
        let rows: Vec<(String, f64)> = sqlx::query_as(
            "SELECT transcript, audio_start_time FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time",
        )
        .bind(M)
        .fetch_all(&pool)
        .await
        .unwrap();
        let texts: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(texts, vec!["hello", "hi", "by", "e"]);
        assert_eq!(SpeakersRepository::list(&pool, M).await.unwrap().len(), 1);
        let labels = SpeakersRepository::labels(&pool, M).await.unwrap();
        assert_eq!(labels.get("spk_0").map(String::as_str), Some("Noah"));
    }

    #[test]
    fn name_and_suggestion_sources_round_trip_as_strings() {
        for s in [NameSource::User, NameSource::Voice, NameSource::Conversation] {
            assert_eq!(NameSource::parse(s.as_str()), Some(s));
        }
        for s in [SuggestionSource::Voice, SuggestionSource::Conversation] {
            assert_eq!(SuggestionSource::parse(s.as_str()), Some(s));
        }
        assert_eq!(NameSource::parse("robot"), None);
        assert_eq!(SuggestionSource::parse("user"), None);
    }

    #[tokio::test]
    async fn speaker_links_round_trip_through_replace_and_list() {
        let pool = migrated_pool().await;
        seed_meeting(&pool, M, &[]).await;
        seed_person(&pool, "person-noah", "Noah").await;
        seed_person(&pool, "person-ana", "Ana").await;
        let link = SpeakerLink {
            person_id: Some("person-noah".into()),
            name_source: Some(NameSource::Voice),
            suggested_person_id: Some("person-ana".into()),
            suggested_name: Some("Ana".into()),
            suggestion_source: Some(SuggestionSource::Conversation),
            suggestion_reason: Some("addressed as Ana at 01:12".into()),
        };
        let mut conn = pool.acquire().await.unwrap();
        SpeakersRepository::replace_for_meeting(
            &mut conn,
            M,
            &SpeakerWrite {
                speakers: vec![NewSpeaker {
                    key: "spk_0".into(),
                    display_name: Some("Noah".into()),
                    embedding: vec![1.0, 0.0],
                    speech_seconds: 2.0,
                    link: link.clone(),
                }],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        drop(conn);

        let speakers = SpeakersRepository::list(&pool, M).await.unwrap();
        assert_eq!(speakers[0].link, link);
        let json = serde_json::to_value(&speakers[0]).unwrap();
        assert_eq!(json["person_id"], "person-noah");
        assert_eq!(json["name_source"], "voice");
        assert_eq!(json["suggested_person_id"], "person-ana");
        assert_eq!(json["suggested_name"], "Ana");
        assert_eq!(json["suggestion_source"], "conversation");
        assert_eq!(json["suggestion_reason"], "addressed as Ana at 01:12");
        assert!(json.get("link").is_none(), "link fields are flattened");
        assert!(json.get("embedding").is_none());
    }

    #[tokio::test]
    async fn existing_speakers_read_with_empty_links() {
        let pool = migrated_pool().await;
        seed_meeting(&pool, M, &[]).await;
        sqlx::query("INSERT INTO meeting_speakers (meeting_id, speaker_key, display_name, created_at) VALUES (?, 'spk_0', 'Ana', '2026-09-27T10:00:00Z')")
            .bind(M)
            .execute(&pool)
            .await
            .unwrap();
        let speakers = SpeakersRepository::list(&pool, M).await.unwrap();
        assert_eq!(speakers[0].display_name.as_deref(), Some("Ana"));
        assert_eq!(speakers[0].link, SpeakerLink::default());
        let json = serde_json::to_value(&speakers[0]).unwrap();
        assert!(json["person_id"].is_null());
        assert!(json["name_source"].is_null());
    }
}
