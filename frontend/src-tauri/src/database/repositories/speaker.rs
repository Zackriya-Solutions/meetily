use serde::{Deserialize, Serialize};
use sqlx::{Error as SqlxError, FromRow, SqlitePool};

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MeetingSpeakerName {
    pub speaker_id: String,
    pub display_name: String,
}

pub struct SpeakersRepository;

impl SpeakersRepository {
    pub async fn list_names(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<MeetingSpeakerName>, SqlxError> {
        sqlx::query_as::<_, MeetingSpeakerName>(
            "SELECT speaker_id, display_name FROM meeting_speakers WHERE meeting_id = ? ORDER BY speaker_id",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
    }

    /// Sets a display name; an empty name restores the default label.
    pub async fn rename(
        pool: &SqlitePool,
        meeting_id: &str,
        speaker_id: &str,
        display_name: &str,
    ) -> Result<(), SqlxError> {
        let name = display_name.trim();
        if name.is_empty() {
            sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ? AND speaker_id = ?")
                .bind(meeting_id)
                .bind(speaker_id)
                .execute(pool)
                .await?;
        } else {
            sqlx::query(
                "INSERT INTO meeting_speakers (meeting_id, speaker_id, display_name, updated_at)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(meeting_id, speaker_id)
                 DO UPDATE SET display_name = excluded.display_name, updated_at = excluded.updated_at",
            )
            .bind(meeting_id)
            .bind(speaker_id)
            .bind(name)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    /// Reassigns every transcript segment of `from` to `to` (merging two speakers)
    /// and drops the merged speaker's custom name.
    pub async fn merge(pool: &SqlitePool, meeting_id: &str, from: &str, to: &str) -> Result<u64, SqlxError> {
        let mut tx = pool.begin().await?;
        let updated = sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND speaker = ?")
            .bind(to)
            .bind(meeting_id)
            .bind(from)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ? AND speaker_id = ?")
            .bind(meeting_id)
            .bind(from)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(updated)
    }

    /// Sets the speaker of a single transcript segment (manual correction).
    pub async fn set_segment_speaker(
        pool: &SqlitePool,
        meeting_id: &str,
        transcript_id: &str,
        speaker_id: Option<&str>,
    ) -> Result<bool, SqlxError> {
        let result = sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND id = ?")
            .bind(speaker_id)
            .bind(meeting_id)
            .bind(transcript_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
