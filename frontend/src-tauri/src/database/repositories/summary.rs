use crate::database::models::SummaryProcess;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::SqlitePool;
use log::{error, info as log_info};
use uuid::Uuid;

pub struct SummaryProcessesRepository;

impl SummaryProcessesRepository {
    /// Retrieves the current summary process state for a given meeting ID.
    pub async fn get_summary_data(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>("SELECT * FROM summary_processes WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
    }

    pub async fn update_meeting_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        summary: &Value,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = pool.begin().await?;

        let meeting_exists: bool = sqlx::query("SELECT 1 FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(&mut *transaction)
            .await?
            .is_some();

        if !meeting_exists {
            log_info!(
                "Attempted to save summary for a non-existent meeting_id: {}",
                meeting_id
            );
            transaction.rollback().await?;
            return Ok(false);
        }

        let result_json = serde_json::to_string(summary);
        if result_json.is_err() {
            error!("Can't convert the json to string for saving to Database");
            transaction.rollback().await?;
            return Ok(false);
        }
        let now = Utc::now();
        let update = sqlx::query("UPDATE summary_processes SET result = ?, updated_at = ? WHERE meeting_id = ?")
            .bind(&result_json.unwrap())
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;
        if update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }

        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;

        log_info!(
            "Successfully updated summary and timestamp for meeting_id: {}",
            meeting_id
        );
        Ok(true)
    }

    pub async fn get_summary_data_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        // Imported recaps intentionally have no transcript_chunks row.
        Self::get_summary_data(pool, meeting_id).await
    }

    /// Atomically create a transcript-free meeting with an already-completed
    /// summary. No speech-to-text or summary model is involved.
    pub async fn create_meeting_with_imported_summary(
        pool: &SqlitePool,
        title: &str,
        summary: &Value,
        source_url: Option<&str>,
        created_at: chrono::DateTime<Utc>,
    ) -> Result<String, sqlx::Error> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());
        let now = Utc::now();
        let result_json = serde_json::to_string(summary)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize recap: {e}")))?;
        let metadata = serde_json::to_string(&serde_json::json!({
            "source": "microsoft_copilot_recap",
            "source_url": source_url,
        }))
        .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize recap metadata: {e}")))?;

        let mut transaction = pool.begin().await?;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(title)
        .bind(created_at)
        .bind(now)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO summary_processes (
                meeting_id, status, created_at, updated_at, start_time, end_time,
                result, chunk_count, processing_time, metadata
            ) VALUES (?, 'completed', ?, ?, ?, ?, ?, 0, 0.0, ?)
            "#,
        )
        .bind(&meeting_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(result_json)
        .bind(metadata)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(meeting_id)
    }

    /// Store an imported recap as the completed summary of an existing
    /// meeting, whether or not it was ever summarized. A row in progress is
    /// replaced too: the recap is the summary the user asked for. The summary
    /// it replaces stays in `result_backup` unless that is itself a recap, so
    /// repeated recap updates keep the meeting's own summary recoverable.
    /// Returns false when the meeting does not exist.
    pub async fn attach_imported_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        summary: &Value,
    ) -> Result<bool, sqlx::Error> {
        let result_json = serde_json::to_string(summary)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize recap: {e}")))?;
        let metadata = serde_json::to_string(&serde_json::json!({
            "source": "microsoft_copilot_recap",
            "source_url": summary.get("source_url"),
        }))
        .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize recap metadata: {e}")))?;
        let now = Utc::now();

        let mut transaction = pool.begin().await?;
        let meeting_exists: bool = sqlx::query("SELECT 1 FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(&mut *transaction)
            .await?
            .is_some();
        if !meeting_exists {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            r#"
            INSERT INTO summary_processes (
                meeting_id, status, created_at, updated_at, start_time, end_time,
                result, chunk_count, processing_time, metadata
            ) VALUES (?, 'completed', ?, ?, ?, ?, ?, 0, 0.0, ?)
            ON CONFLICT(meeting_id) DO UPDATE SET
                status = 'completed',
                updated_at = excluded.updated_at,
                end_time = excluded.end_time,
                result = excluded.result,
                error = NULL,
                result_backup = CASE
                    WHEN INSTR(COALESCE(summary_processes.result, ''), '"imported_from":"microsoft_copilot_recap"') > 0
                        THEN summary_processes.result_backup
                    ELSE COALESCE(summary_processes.result, summary_processes.result_backup)
                END,
                result_backup_timestamp = CASE
                    WHEN INSTR(COALESCE(summary_processes.result, ''), '"imported_from":"microsoft_copilot_recap"') > 0
                        THEN summary_processes.result_backup_timestamp
                    ELSE excluded.updated_at
                END,
                metadata = excluded.metadata
            "#,
        )
        .bind(meeting_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(result_json)
        .bind(metadata)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn create_or_reset_process(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        log_info!(
            "Creating or resetting summary process for meeting_id: {}",
            meeting_id
        );
        sqlx::query(
            r#"
            INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, start_time, result, error)
            VALUES (?, 'PENDING', ?, ?, ?, NULL, NULL)
            ON CONFLICT(meeting_id) DO UPDATE SET
                status = 'PENDING',
                updated_at = excluded.updated_at,
                start_time = excluded.start_time,
                result_backup = result,
                result_backup_timestamp = excluded.updated_at,
                result = result,
                error = NULL
            "#
        )
        .bind(meeting_id)
        .bind(started_at)
        .bind(started_at)
        .bind(started_at)
        .execute(pool)
        .await?;
        log_info!(
            "Backed up existing summary before regeneration for meeting_id: {}",
            meeting_id
        );
        Ok(())
    }

    pub async fn update_process_completed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        result: Value,
        chunk_count: i64,
        processing_time: f64,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();
        let result_str = serde_json::to_string(&result)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize result: {}", e)))?;

        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET status = 'completed', result = ?, updated_at = ?, end_time = ?, chunk_count = ?, processing_time = ?, error = NULL, result_backup = NULL, result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#
        )
        .bind(result_str)
        .bind(now)
        .bind(now)
        .bind(chunk_count)
        .bind(processing_time)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?;
        Ok(update.rows_affected() == 1)
    }

    pub async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        error: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'failed',
                error = ?,
                updated_at = ?,
                end_time = ?,
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#,
        )
        .bind(error)
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?;
        Ok(update.rows_affected() == 1)
    }

    /// Fail every attempt still marked pending. Call only at startup: no
    /// summary task survives a restart, so such rows are orphaned (for example
    /// after a native crash) and would otherwise show "processing" forever.
    /// The previous summary, if this was a regeneration, is restored.
    pub async fn fail_interrupted_processes(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
        let now = Utc::now();
        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'failed',
                error = 'Summary generation was interrupted when Meetily closed. Please try again.',
                updated_at = ?,
                end_time = ?,
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE LOWER(status) IN ('pending', 'processing')
            "#,
        )
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(update.rows_affected())
    }

    pub async fn update_process_cancelled(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'cancelled',
                updated_at = ?,
                end_time = ?,
                error = 'Generation was cancelled by user',
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?;
        Ok(update.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE summary_processes (
                meeting_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                created_at TEXT,
                updated_at TEXT,
                start_time TEXT,
                result TEXT,
                result_backup TEXT,
                result_backup_timestamp TEXT,
                end_time TEXT,
                chunk_count INTEGER,
                processing_time REAL,
                error TEXT,
                metadata TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn attach_imported_summary_creates_or_replaces_the_summary_row() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'T', '2026-07-21T15:00:00Z', '2026-07-21T15:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let recap = json!({"markdown": "- first", "source_url": "https://t/r"});

        // Never summarized: a completed row appears.
        assert!(SummaryProcessesRepository::attach_imported_summary(&pool, "m1", &recap).await.unwrap());
        let row = SummaryProcessesRepository::get_summary_data(&pool, "m1").await.unwrap().unwrap();
        assert_eq!(row.status, "completed");
        assert!(row.result.unwrap().contains("first"));

        // Summarizing right now: the recap replaces the pending attempt.
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1", Utc::now()).await.unwrap();
        let again = json!({"markdown": "- second"});
        assert!(SummaryProcessesRepository::attach_imported_summary(&pool, "m1", &again).await.unwrap());
        let row = SummaryProcessesRepository::get_summary_data(&pool, "m1").await.unwrap().unwrap();
        assert_eq!(row.status, "completed");
        assert!(row.result.unwrap().contains("second"));

        assert!(!SummaryProcessesRepository::attach_imported_summary(&pool, "missing", &again).await.unwrap());
    }

    #[tokio::test]
    async fn attaching_a_recap_keeps_the_meetings_own_summary_recoverable() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'T', '2026-07-21T15:00:00Z', '2026-07-21T15:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let recap = |text: &str| json!({"markdown": text, "imported_from": "microsoft_copilot_recap"});

        // No summary yet: nothing to keep.
        assert!(SummaryProcessesRepository::attach_imported_summary(&pool, "m1", &recap("- first recap")).await.unwrap());
        let row = SummaryProcessesRepository::get_summary_data(&pool, "m1").await.unwrap().unwrap();
        assert!(row.result_backup.is_none());
        sqlx::query("DELETE FROM summary_processes WHERE meeting_id = 'm1'").execute(&pool).await.unwrap();

        // The meeting's own summary moves aside for the recap.
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1", Utc::now()).await.unwrap();
        SummaryProcessesRepository::update_meeting_summary(&pool, "m1", &json!({"markdown": "- from Meetily"}))
            .await
            .unwrap();
        assert!(SummaryProcessesRepository::attach_imported_summary(&pool, "m1", &recap("- recap")).await.unwrap());
        let row = SummaryProcessesRepository::get_summary_data(&pool, "m1").await.unwrap().unwrap();
        assert!(row.result.unwrap().contains("- recap"));
        assert!(row.result_backup.unwrap().contains("from Meetily"));
        assert!(row.result_backup_timestamp.is_some());

        // A changed recap replaces the recap and leaves the kept summary alone.
        assert!(SummaryProcessesRepository::attach_imported_summary(&pool, "m1", &recap("- changed recap")).await.unwrap());
        let row = SummaryProcessesRepository::get_summary_data(&pool, "m1").await.unwrap().unwrap();
        assert!(row.result.unwrap().contains("changed recap"));
        assert!(row.result_backup.unwrap().contains("from Meetily"));
    }

    #[tokio::test]
    async fn imported_recap_is_retrievable_without_transcript_chunks() {
        let pool = test_pool().await;
        let summary = json!({
            "markdown": "# Decisions\n\n- Use the existing treasury workflow.",
            "imported_from": "microsoft_copilot_recap"
        });

        let meeting_id = SummaryProcessesRepository::create_meeting_with_imported_summary(
            &pool,
            "Treasury kick-off",
            &summary,
            Some("https://tenant.sharepoint.com/recording"),
            Utc::now(),
        )
        .await
        .unwrap();

        let stored = SummaryProcessesRepository::get_summary_data_for_meeting(&pool, &meeting_id)
            .await
            .unwrap()
            .expect("imported recap should be returned without a transcript chunk");
        assert_eq!(stored.status, "completed");
        assert_eq!(
            serde_json::from_str::<Value>(stored.result.as_deref().unwrap()).unwrap(),
            summary
        );
        let title: String = sqlx::query_scalar("SELECT title FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(title, "Treasury kick-off");
    }

    async fn seed_pending(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        result: Option<&str>,
        result_backup: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO summary_processes (
                meeting_id, status, created_at, updated_at, start_time, result, result_backup
            ) VALUES (?, 'PENDING', ?, ?, ?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(started_at)
        .bind(started_at)
        .bind(started_at)
        .bind(result)
        .bind(result_backup)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn terminal_compare_and_set_preserves_first_writer() {
        let pool = test_pool().await;
        let completed_start = Utc::now();
        seed_pending(&pool, "completed-first", completed_start, None, None).await;
        assert!(
            SummaryProcessesRepository::update_process_completed(
                &pool,
                "completed-first",
                completed_start,
                json!({"markdown": "completed"}),
                1,
                1.0,
            )
            .await
            .unwrap()
        );
        assert!(
            !SummaryProcessesRepository::update_process_cancelled(
                &pool,
                "completed-first",
                completed_start,
            )
            .await
            .unwrap()
        );
        let completed_status: String = sqlx::query_scalar(
            "SELECT status FROM summary_processes WHERE meeting_id = 'completed-first'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(completed_status, "completed");

        let cancelled_start = completed_start + chrono::Duration::nanoseconds(1);
        let previous = r#"{"markdown":"previous"}"#;
        seed_pending(
            &pool,
            "cancelled-first",
            cancelled_start,
            Some(previous),
            Some(previous),
        )
        .await;
        assert!(
            SummaryProcessesRepository::update_process_cancelled(
                &pool,
                "cancelled-first",
                cancelled_start,
            )
            .await
            .unwrap()
        );
        assert!(
            !SummaryProcessesRepository::update_process_completed(
                &pool,
                "cancelled-first",
                cancelled_start,
                json!({"markdown": "completed"}),
                1,
                1.0,
            )
            .await
            .unwrap()
        );
        let cancelled: (String, String) = sqlx::query_as(
            "SELECT status, result FROM summary_processes WHERE meeting_id = 'cancelled-first'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cancelled, ("cancelled".to_string(), previous.to_string()));
    }

    #[tokio::test]
    async fn cancelled_compare_and_set_rejects_stale_start_time() {
        let pool = test_pool().await;
        let current_start = Utc::now();
        seed_pending(&pool, "stale-cancel", current_start, None, None).await;
        assert!(
            !SummaryProcessesRepository::update_process_cancelled(
                &pool,
                "stale-cancel",
                current_start - chrono::Duration::nanoseconds(1),
            )
            .await
            .unwrap()
        );
        let status: String =
            sqlx::query_scalar("SELECT status FROM summary_processes WHERE meeting_id = 'stale-cancel'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "PENDING");
    }
}
