use crate::api::{TranscriptSearchResult, TranscriptSegment};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqlitePool};
use log::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Browser recovery can augment audio recovery, but it must reuse an
    /// already recovered native meeting and never race an active session.
    pub async fn recover_session_transcript(
        pool: &SqlitePool,
        title: &str,
        transcripts: &[TranscriptSegment],
        folder: Option<String>,
        session: &str,
    ) -> Result<String, SqlxError> {
        let (status,native_folder): (String,Option<String>) = sqlx::query_as("SELECT status,folder_path FROM recording_sessions WHERE id=?")
            .bind(session).fetch_one(pool).await?;
        if !matches!(status.as_str(), "completed" | "error" | "interrupted") {
            return Err(SqlxError::Protocol("This native recording is active or has been deleted; it cannot be recovered again.".into()));
        }
        Self::save_session_transcript(pool,title,transcripts,native_folder.or(folder),Some(session)).await
    }

    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
    ) -> Result<String, SqlxError> {
        Self::save_session_transcript(pool, meeting_title, transcripts, folder_path, None).await
    }

    /// Recording finalization uses a stable session mapping in the same
    /// transaction as meeting creation; retrying a save cannot duplicate it.
    pub async fn save_session_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        session_id: Option<&str>,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", session_id.map(str::to_owned).unwrap_or_else(|| Uuid::new_v4().to_string()));

        // Reserve the writer before reading the session mapping. A deferred
        // transaction can read an old WAL snapshot while phase persistence
        // commits, then fail its first write with SQLITE_BUSY_SNAPSHOT.
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;

        if let Some(session) = session_id {
            let (status,existing): (String,Option<String>) = sqlx::query_as("SELECT status,meeting_id FROM recording_sessions WHERE id=?")
                .bind(session).fetch_one(&mut *transaction).await?;
            if status == "deleted" { return Err(SqlxError::RowNotFound); }
            if let Some(existing) = existing { transaction.commit().await?; return Ok(existing); }
        }

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting with id: {}", meeting_id);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!("Failed to save transcript segment for meeting {}", meeting_id);
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        if let Some(session) = session_id {
            let mapped = sqlx::query("UPDATE recording_sessions SET meeting_id=? WHERE id=?")
                .bind(&meeting_id).bind(session).execute(&mut *transaction).await?;
            if mapped.rows_affected() != 1 { return Err(SqlxError::RowNotFound); }
        }

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let search_query = format!("%{}%", query.to_lowercase());

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id
             WHERE LOWER(t.transcript) LIKE ?",
        )
        .bind(&search_query)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    fn get_match_context(transcript: &str, query: &str) -> String {
        let transcript_lower = transcript.to_lowercase();
        let query_lower = query.to_lowercase();

        match transcript_lower.find(&query_lower) {
            Some(match_index) => {
                let start_index = match_index.saturating_sub(100);
                let end_index = (match_index + query.len() + 100).min(transcript.len());

                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.push_str(&transcript[start_index..end_index]);
                if end_index < transcript.len() {
                    context.push_str("...");
                }
                context
            }
            None => transcript.chars().take(200).collect(), // Fallback to the start of the transcript
        }
    }
}
