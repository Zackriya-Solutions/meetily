//! Per-meeting speakers: names, voice centroids, merges and row reassignment.
use crate::diarization::cluster::weighted_centroid;
use serde::{Deserialize, Serialize};
use sqlx::{Connection, Error as SqlxError, Row, SqliteConnection, SqlitePool};
use std::collections::BTreeMap;
use uuid::Uuid;

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
}

pub struct NewSpeaker {
    pub key: String,
    pub display_name: Option<String>,
    pub embedding: Vec<f32>,
    pub speech_seconds: f64,
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
            })
            .collect();
        speakers.sort_by_key(|s| (key_index(&s.speaker_key).unwrap_or(usize::MAX), s.speaker_key.clone()));
        Ok(speakers)
    }

    /// Speaker key → label shown to the user.
    pub async fn labels(pool: &SqlitePool, meeting_id: &str) -> Result<BTreeMap<String, String>, SqlxError> {
        Ok(Self::list(pool, meeting_id)
            .await?
            .into_iter()
            .map(|s| {
                let label = speaker_label(&s.speaker_key, s.display_name.as_deref());
                (s.speaker_key, label)
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

    /// Fold `from` into `into`: rows move, centroids combine weighted by speech time, and `into` keeps its name (or takes `from`'s when it has none).
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
        sqlx::query(
            "UPDATE meeting_speakers SET embedding = ?, speech_seconds = ?, display_name = COALESCE(display_name, ?)
             WHERE meeting_id = ? AND speaker_key = ?",
        )
        .bind(merged.as_deref().map(embedding_to_blob))
        .bind(a.speech_seconds + b.speech_seconds)
        .bind(a.display_name.as_deref())
        .bind(meeting_id)
        .bind(into)
        .execute(&mut *tx)
        .await?;
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
        let speakers = Self::list_conn(&mut tx, meeting_id).await?;
        let key = match target {
            ReassignTarget::Existing(key) => {
                if !speakers.iter().any(|s| s.speaker_key == key) {
                    return Err(not_found("speaker"));
                }
                key
            }
            ReassignTarget::New => {
                let used: Vec<Option<String>> =
                    sqlx::query_scalar("SELECT DISTINCT speaker FROM transcripts WHERE meeting_id = ?")
                        .bind(meeting_id)
                        .fetch_all(&mut *tx)
                        .await?;
                let next = speakers
                    .iter()
                    .map(|s| s.speaker_key.clone())
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
                "INSERT INTO meeting_speakers (meeting_id, speaker_key, display_name, embedding, speech_seconds, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(meeting_id)
            .bind(&s.key)
            .bind(&s.display_name)
            .bind(embedding_to_blob(&s.embedding))
            .bind(s.speech_seconds)
            .bind(&now)
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
                sqlx::query(
                    "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(format!("transcript-{}", Uuid::new_v4()))
                .bind(meeting_id)
                .bind(&piece.text)
                .bind(&timestamp)
                .bind(piece.start_s)
                .bind(piece.end_s)
                .bind(piece.end_s - piece.start_s)
                .bind(&piece.speaker)
                .execute(&mut *conn)
                .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_support::{migrated_pool, seed_meeting, SeedRow};

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
                    NewSpeaker { key: "spk_0".into(), display_name: None, embedding: vec![1.0, 0.0], speech_seconds: 3.0 },
                    NewSpeaker { key: "spk_1".into(), display_name: Some("Ana".into()), embedding: vec![0.0, 1.0], speech_seconds: 1.0 },
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
                speakers: vec![NewSpeaker { key: "spk_0".into(), display_name: Some("Noah".into()), embedding: vec![1.0, 0.0], speech_seconds: 6.0 }],
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
}
