use crate::api::{Meeting, MeetingDetails, MeetingTranscript};
use crate::database::models::{MeetingModel, PendingMeetingModel, Transcript};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqliteConnection, SqlitePool};
use log::{error, info};

pub struct MeetingsRepository;

/// The columns of `transcripts` that [`MeetingsRepository::get_meeting`]
/// actually consumes; the summary/action_items/key_points TEXT columns are
/// left on disk.
#[derive(sqlx::FromRow)]
struct MeetingTranscriptRow {
    id: String,
    transcript: String,
    timestamp: String,
    audio_start_time: Option<f64>,
    audio_end_time: Option<f64>,
    duration: Option<f64>,
    speaker: Option<String>,
}

/// Named so `pending_meetings_query_uses_the_transcripts_index` can plan the
/// same text the pipeline tick runs.
const PENDING_MEETINGS_SQL: &str = r#"
    SELECT m.id, m.title, m.created_at, m.folder_path,
           CASE WHEN EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id)
                THEN 1 ELSE 0 END AS transcript_count,
           sp.status AS summary_status,
           m.transcription_incomplete
    FROM meetings m
    LEFT JOIN summary_processes sp ON sp.meeting_id = m.id
    WHERE (m.folder_path IS NOT NULL AND m.folder_path <> ''
           AND NOT EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id))
       OR m.transcription_incomplete = 1
       OR (EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id)
           AND (sp.status IS NULL OR sp.status IN ('failed', 'cancelled')))
    ORDER BY m.created_at DESC
"#;

/// `transcribed` excludes a transcript saved before live transcription
/// finished. `transcript_needed` is [`PENDING_MEETINGS_SQL`]'s test for what
/// still needs transcribing: a meeting with no recording to transcribe, such
/// as one made from a Teams recap alone, lacks a transcript without needing one.
const MEETING_LIST_SQL: &str = r#"
    SELECT m.id, m.title, m.created_at,
           (m.transcription_incomplete = 0
            AND EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id)) AS transcribed,
           ((m.folder_path IS NOT NULL AND m.folder_path <> ''
             AND NOT EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id))
            OR m.transcription_incomplete = 1) AS transcript_needed,
           EXISTS (SELECT 1 FROM summary_processes sp
                   WHERE sp.meeting_id = m.id AND sp.status = 'completed') AS summarized,
           EXISTS (SELECT 1 FROM obsidian_exports oe WHERE oe.meeting_id = m.id) AS obsidian_exported
    FROM meetings m
    ORDER BY m.created_at DESC
"#;

impl MeetingsRepository {
    pub async fn get_meetings(pool: &SqlitePool) -> Result<Vec<Meeting>, sqlx::Error> {
        sqlx::query_as::<_, Meeting>(MEETING_LIST_SQL)
            .fetch_all(pool)
            .await
    }

    /// Meetings with outstanding work: a recording folder but no transcripts
    /// (pending transcription), or transcripts but no completed/in-flight
    /// summary (pending AI summary). In-flight `PENDING` summaries are
    /// excluded so callers never offer to duplicate a running job.
    ///
    /// `transcript_count` is a 0/1 presence flag, not a row count — every
    /// caller only compares it against zero, and EXISTS lets the
    /// `idx_transcripts_meeting_id` lookup stop at the first matching row
    /// instead of aggregating the whole table.
    pub async fn get_pending_meetings(
        pool: &SqlitePool,
    ) -> Result<Vec<PendingMeetingModel>, sqlx::Error> {
        let pending = sqlx::query_as::<_, PendingMeetingModel>(PENDING_MEETINGS_SQL)
            .fetch_all(pool)
            .await?;
        Ok(pending)
    }

    pub async fn delete_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        match delete_meeting_with_transaction(&mut transaction, meeting_id).await {
            Ok(success) => {
                if success {
                    transaction.commit().await?;
                    info!(
                        "Successfully deleted meeting {} and all associated data",
                        meeting_id
                    );
                    Ok(true)
                } else {
                    transaction.rollback().await?;
                    Ok(false)
                }
            }
            Err(e) => {
                let _ = transaction.rollback().await;
                error!("Failed to delete meeting {}: {}", meeting_id, e);
                Err(e)
            }
        }
    }

    pub async fn get_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingDetails>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        // Get meeting details
        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(&mut *transaction)
                .await?;

        if meeting.is_none() {
            transaction.rollback().await?;
            return Err(SqlxError::RowNotFound);
        }

        if let Some(meeting) = meeting {
            // Get all transcripts for this meeting
            let transcripts = sqlx::query_as::<_, MeetingTranscriptRow>(
                "SELECT id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker
                 FROM transcripts WHERE meeting_id = ?",
            )
            .bind(meeting_id)
            .fetch_all(&mut *transaction)
            .await?;

            transaction.commit().await?;

            // Convert Transcript to MeetingTranscript
            let meeting_transcripts = transcripts
                .into_iter()
                .map(|t| MeetingTranscript {
                    id: t.id,
                    text: t.transcript,
                    timestamp: t.timestamp,
                    audio_start_time: t.audio_start_time,
                    audio_end_time: t.audio_end_time,
                    duration: t.duration,
                    speaker: t.speaker,
                })
                .collect::<Vec<_>>();

            Ok(Some(MeetingDetails {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                transcripts: meeting_transcripts,
            }))
        } else {
            transaction.rollback().await?;
            Ok(None)
        }
    }

    /// Get meeting metadata without transcripts (for pagination)
    pub async fn get_meeting_metadata(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingModel>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;

        Ok(meeting)
    }

    /// Get meeting transcripts with pagination support
    pub async fn get_meeting_transcripts_paginated(
        pool: &SqlitePool,
        meeting_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Transcript>, i64), SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        // Get total count of transcripts for this meeting
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?"
        )
        .bind(meeting_id)
        .fetch_one(pool)
        .await?;

        // Get paginated transcripts ordered by audio_start_time
        let transcripts = sqlx::query_as::<_, Transcript>(
            "SELECT * FROM transcripts
             WHERE meeting_id = ?
             ORDER BY audio_start_time ASC
             LIMIT ? OFFSET ?"
        )
        .bind(meeting_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok((transcripts, total.0))
    }

    /// Move a meeting to when it took place, for an import that learns the
    /// meeting's start only after it was saved. `false` when it is gone.
    pub async fn update_meeting_date(
        pool: &SqlitePool,
        meeting_id: &str,
        created_at: chrono::DateTime<Utc>,
    ) -> Result<bool, SqlxError> {
        let result = sqlx::query("UPDATE meetings SET created_at = ?, updated_at = ? WHERE id = ?")
            .bind(created_at)
            .bind(Utc::now())
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_meeting_title(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now().naive_utc();

        let rows_affected =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
        if rows_affected.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_meeting_name(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        let mut transaction = pool.begin().await?;
        let now = Utc::now();

        // Update meetings table
        let meeting_update =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;

        if meeting_update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false); // Meeting not found
        }

        // Update transcript_chunks table
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(new_title)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;
        Ok(true)
    }
}

async fn delete_meeting_with_transaction(
    transaction: &mut SqliteConnection,
    meeting_id: &str,
) -> Result<bool, SqlxError> {
    // Check if meeting exists
    let meeting_exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

    if meeting_exists.is_none() {
        error!("Meeting {} not found for deletion", meeting_id);
        return Ok(false);
    }

    // Delete from related tables in proper order
    // 1. Delete from transcript_chunks
    sqlx::query("DELETE FROM transcript_chunks WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 2. Delete from summary_processes
    sqlx::query("DELETE FROM summary_processes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 3. Delete from transcripts
    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 4. Delete user tags
    sqlx::query("DELETE FROM meeting_tags WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 5. Delete import provenance
    sqlx::query("DELETE FROM meeting_sources WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 6. Obsidian filename pin and pipeline retry bookkeeping (no FK, so no cascade)
    sqlx::query("DELETE FROM obsidian_exports WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    sqlx::query("DELETE FROM pipeline_meta WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 7. Finally, delete the meeting
    let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply to a fresh database");
        pool
    }

    async fn insert_meeting(pool: &SqlitePool, id: &str, folder_path: Option<&str>) {
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path)
             VALUES (?, ?, '2026-07-24T10:00:00Z', '2026-07-24T10:00:00Z', ?)",
        )
        .bind(id)
        .bind(format!("Meeting {}", id))
        .bind(folder_path)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_transcript(pool: &SqlitePool, meeting_id: &str) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ROW: AtomicUsize = AtomicUsize::new(0);

        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp)
             VALUES (?, ?, 'hello', '2026-07-24T10:00:00Z')",
        )
        .bind(format!(
            "t-{}-{}",
            meeting_id,
            NEXT_ROW.fetch_add(1, Ordering::Relaxed)
        ))
        .bind(meeting_id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_summary_process(pool: &SqlitePool, meeting_id: &str, status: &str) {
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, status, created_at, updated_at)
             VALUES (?, ?, '2026-07-24T10:00:00Z', '2026-07-24T10:00:00Z')",
        )
        .bind(meeting_id)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn pending_meetings_covers_each_derived_state() {
        let pool = test_pool().await;

        // Recording but no transcripts -> pending transcription.
        insert_meeting(&pool, "needs-transcript", Some("C:/rec/a")).await;

        // Transcripts but no summary process row -> pending summary.
        insert_meeting(&pool, "needs-summary", None).await;
        insert_transcript(&pool, "needs-summary").await;

        // Transcripts with a failed summary -> pending summary (retry).
        // Two rows: transcript_count must stay a 0/1 flag, not become 2.
        insert_meeting(&pool, "failed-summary", Some("C:/rec/b")).await;
        insert_transcript(&pool, "failed-summary").await;
        insert_transcript(&pool, "failed-summary").await;
        insert_summary_process(&pool, "failed-summary", "failed").await;

        // Completed summary -> not pending.
        insert_meeting(&pool, "all-done", Some("C:/rec/c")).await;
        insert_transcript(&pool, "all-done").await;
        insert_summary_process(&pool, "all-done", "completed").await;

        // Summary already running -> excluded so it is never started twice.
        insert_meeting(&pool, "summary-running", None).await;
        insert_transcript(&pool, "summary-running").await;
        insert_summary_process(&pool, "summary-running", "PENDING").await;

        // Neither recording nor transcripts -> excluded.
        insert_meeting(&pool, "empty", None).await;

        let pending = MeetingsRepository::get_pending_meetings(&pool).await.unwrap();
        let mut ids: Vec<&str> = pending.iter().map(|p| p.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["failed-summary", "needs-summary", "needs-transcript"]);

        let by_id = |id: &str| pending.iter().find(|p| p.id == id).unwrap();
        assert_eq!(by_id("needs-transcript").transcript_count, 0);
        assert_eq!(by_id("needs-summary").transcript_count, 1);
        assert_eq!(by_id("failed-summary").transcript_count, 1);
        assert_eq!(by_id("needs-summary").summary_status, None);
        assert_eq!(
            by_id("failed-summary").summary_status.as_deref(),
            Some("failed")
        );
    }

    #[tokio::test]
    async fn meeting_list_reports_finished_stages() {
        let pool = test_pool().await;

        insert_meeting(&pool, "recorded", Some("C:/rec/a")).await;

        insert_meeting(&pool, "partial", Some("C:/rec/p")).await;
        insert_transcript(&pool, "partial").await;
        mark_incomplete(&pool, "partial").await;

        insert_meeting(&pool, "summary-running", None).await;
        insert_transcript(&pool, "summary-running").await;
        insert_summary_process(&pool, "summary-running", "PENDING").await;

        // Made from a recap alone: a summary, and no recording to transcribe.
        insert_meeting(&pool, "recap-only", None).await;
        insert_summary_process(&pool, "recap-only", "completed").await;

        insert_meeting(&pool, "done", None).await;
        insert_transcript(&pool, "done").await;
        insert_transcript(&pool, "done").await;
        insert_summary_process(&pool, "done", "completed").await;
        sqlx::query(
            "INSERT INTO obsidian_exports (meeting_id, filename, exported_at)
             VALUES ('done', 'done.md', '2026-07-24T10:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let meetings = MeetingsRepository::get_meetings(&pool).await.unwrap();
        let mut stages: Vec<(&str, bool, bool, bool, bool)> = meetings
            .iter()
            .map(|m| (m.id.as_str(), m.transcribed, m.transcript_needed, m.summarized, m.obsidian_exported))
            .collect();
        stages.sort();
        assert_eq!(
            stages,
            vec![
                ("done", true, false, true, true),
                ("partial", false, true, false, false),
                ("recap-only", false, false, true, false),
                ("recorded", false, true, false, false),
                ("summary-running", true, false, false, false),
            ]
        );
    }

    async fn mark_incomplete(pool: &SqlitePool, meeting_id: &str) {
        sqlx::query("UPDATE meetings SET transcription_incomplete = 1 WHERE id = ?")
            .bind(meeting_id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn incomplete_transcription_stays_pending_until_cleared() {
        let pool = test_pool().await;

        // Partial transcript saved at Stop: pending even with segments.
        insert_meeting(&pool, "partial", Some("C:/rec/p")).await;
        insert_transcript(&pool, "partial").await;
        mark_incomplete(&pool, "partial").await;

        // Zero transcripts and incomplete.
        insert_meeting(&pool, "zero", Some("C:/rec/z")).await;
        mark_incomplete(&pool, "zero").await;

        // Even a summary someone ran manually does not hide it.
        insert_meeting(&pool, "summarized-partial", Some("C:/rec/s")).await;
        insert_transcript(&pool, "summarized-partial").await;
        insert_summary_process(&pool, "summarized-partial", "completed").await;
        mark_incomplete(&pool, "summarized-partial").await;

        let pending = MeetingsRepository::get_pending_meetings(&pool).await.unwrap();
        for id in ["partial", "zero", "summarized-partial"] {
            let item = pending.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("{id} pending"));
            assert!(item.transcription_incomplete, "{id}");
        }

        // Retranscription replaces the transcript and clears the marker in one
        // transaction (mirrors audio/retranscription.rs).
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
            .bind("summarized-partial")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE meetings SET transcription_incomplete = 0 WHERE id = ?")
            .bind("summarized-partial")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        insert_transcript(&pool, "summarized-partial").await;

        let pending = MeetingsRepository::get_pending_meetings(&pool).await.unwrap();
        assert!(!pending.iter().any(|p| p.id == "summarized-partial"));
    }

    #[tokio::test]
    async fn save_persists_incomplete_marker_atomically_with_meeting() {
        use crate::database::repositories::transcript::TranscriptsRepository;
        let pool = test_pool().await;
        let id = TranscriptsRepository::save_transcript(&pool, "Cut short", &[], Some("C:/rec/c".into()), true)
            .await
            .unwrap();
        let flag: bool = sqlx::query_scalar("SELECT transcription_incomplete FROM meetings WHERE id = ?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(flag);
        let complete = TranscriptsRepository::save_transcript(&pool, "Normal", &[], None, false)
            .await
            .unwrap();
        let flag: bool = sqlx::query_scalar("SELECT transcription_incomplete FROM meetings WHERE id = ?")
            .bind(&complete)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(!flag);
    }

    /// The derived table this query replaced aggregated all of `transcripts` on
    /// every pipeline tick, which showed up in the field as `sqlx` slow-statement
    /// warnings of 2-40s. Asserting the plan keeps it from silently regressing.
    #[tokio::test]
    async fn pending_meetings_query_uses_the_transcripts_index() {
        use sqlx::Row;

        let pool = test_pool().await;
        insert_meeting(&pool, "m1", Some("C:/rec/a")).await;
        insert_meeting(&pool, "m2", None).await;
        insert_transcript(&pool, "m2").await;

        let plan: Vec<String> = sqlx::query(&format!("EXPLAIN QUERY PLAN {}", PENDING_MEETINGS_SQL))
            .fetch_all(&pool)
            .await
            .unwrap()
            .iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect();

        assert!(
            plan.iter()
                .any(|step| step.contains("idx_transcripts_meeting_id")),
            "transcripts lookup should use the index; plan was:\n{}",
            plan.join("\n")
        );
        assert!(
            !plan.iter().any(|step| step.starts_with("SCAN t")),
            "transcripts is still being scanned; plan was:\n{}",
            plan.join("\n")
        );
    }

    #[tokio::test]
    async fn delete_meeting_removes_obsidian_export_and_pipeline_meta_rows() {
        let pool = test_pool().await;
        for id in ["doomed", "survivor"] {
            insert_meeting(&pool, id, Some("C:/rec/x")).await;
            sqlx::query(
                "INSERT INTO obsidian_exports (meeting_id, filename, exported_at)
                 VALUES (?, 'Note.md', '2026-07-24T10:00:00Z')",
            )
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO pipeline_meta (meeting_id, attempts, suppressed, updated_at)
                 VALUES (?, 3, 1, '2026-07-24T10:00:00Z')",
            )
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        }

        assert!(MeetingsRepository::delete_meeting(&pool, "doomed").await.unwrap());

        for table in ["obsidian_exports", "pipeline_meta"] {
            let sql = format!("SELECT COUNT(*) FROM {} WHERE meeting_id = ?", table);
            for (id, expected) in [("doomed", 0), ("survivor", 1)] {
                let count: i64 = sqlx::query_scalar(&sql)
                    .bind(id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(count, expected, "{} rows for meeting {}", table, id);
            }
        }
    }
}
