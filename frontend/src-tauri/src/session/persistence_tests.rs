use super as lifecycle;
use crate::{
    audio::recording_saver::TranscriptSegment,
    database::repositories::{meeting::MeetingsRepository, transcript::TranscriptsRepository},
};
use sqlx::SqlitePool;

async fn database() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO recording_sessions (id, title, status) VALUES ('session', 'Test', 'recording')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

#[tokio::test]
async fn native_save_preserves_buffered_results_empty_meetings_and_is_idempotent() {
    let pool = database().await;
    let collector = std::sync::Arc::new(std::sync::Mutex::new(Vec::<TranscriptSegment>::new()));
    let worker = collector.clone();
    worker.lock().unwrap().push(TranscriptSegment {
        id: "late".into(),
        text: "Buffered final words".into(),
        audio_start_time: 30.0,
        audio_end_time: 31.0,
        duration: 1.0,
        display_time: "12:00:31".into(),
        confidence: 0.9,
        sequence_id: 1,
    });
    let segments: Vec<_> = collector
        .lock()
        .unwrap()
        .iter()
        .map(|s| crate::api::TranscriptSegment {
            id: s.id.clone(),
            text: s.text.clone(),
            timestamp: s.display_time.clone(),
            audio_start_time: Some(s.audio_start_time),
            audio_end_time: Some(s.audio_end_time),
            duration: Some(s.duration),
        })
        .collect();
    let id = TranscriptsRepository::save_session_transcript(
        &pool,
        "Test",
        &segments,
        None,
        Some("session"),
    )
    .await
    .unwrap();
    let again =
        TranscriptsRepository::save_session_transcript(&pool, "Test", &[], None, Some("session"))
            .await
            .unwrap();
    assert_eq!(id, again);
    let text: String = sqlx::query_scalar("SELECT transcript FROM transcripts WHERE meeting_id=?")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(text, "Buffered final words");
    sqlx::query(
        "INSERT INTO recording_sessions (id, title, status) VALUES ('silent', 'Silent', 'recording')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let empty =
        TranscriptsRepository::save_session_transcript(&pool, "Silent", &[], None, Some("silent"))
            .await
            .unwrap();
    assert!(MeetingsRepository::get_meeting(&pool, &empty)
        .await
        .unwrap()
        .is_some());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn concurrent_save_and_delayed_stop_results_keep_session_ownership() {
    let pool = database().await;
    let (a, b) = tokio::join!(
        TranscriptsRepository::save_session_transcript(&pool, "Test", &[], None, Some("session")),
        TranscriptsRepository::save_session_transcript(&pool, "Test", &[], None, Some("session"))
    );
    assert_eq!(a.unwrap(), b.unwrap());
    sqlx::query("UPDATE recording_sessions SET status='completed' WHERE id='session'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO recording_sessions (id, title, status) VALUES ('newer', 'Newer meeting', 'recording')").execute(&pool).await.unwrap();
    assert!(lifecycle::completion_result(&pool, "newer").await.is_err());
    let completion = lifecycle::completion_result(&pool, "session")
        .await
        .unwrap();
    assert_eq!(completion["sessionId"], "session");
    assert_eq!(completion["meetingId"], "meeting-session");
    assert_eq!(completion["phase"], "completed");
    sqlx::query(
        "UPDATE recording_sessions SET status='error', error='Save failed' WHERE id='newer'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        lifecycle::completion_result(&pool, "newer")
            .await
            .unwrap_err(),
        "Save failed"
    );
    assert_eq!(
        lifecycle::completion_result(&pool, "session")
            .await
            .unwrap(),
        completion
    );
}

#[tokio::test]
async fn recording_save_waits_for_background_writer_before_reading_session() {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::time::Duration;

    let directory = tempfile::tempdir().unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(3)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(directory.path().join("recording.sqlite"))
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(2)),
        )
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("INSERT INTO recording_sessions (id, title, status) VALUES ('session', 'Test', 'processing')")
        .execute(&pool).await.unwrap();

    let mut writer = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    sqlx::query("UPDATE recording_sessions SET status='saving' WHERE id='session'")
        .execute(&mut *writer)
        .await
        .unwrap();
    let save =
        TranscriptsRepository::save_session_transcript(&pool, "Test", &[], None, Some("session"));
    tokio::pin!(save);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut save)
            .await
            .is_err(),
        "save must wait for the writer instead of upgrading a stale read transaction"
    );
    writer.commit().await.unwrap();
    let meeting = save.await.unwrap();
    let retry =
        TranscriptsRepository::save_session_transcript(&pool, "Test", &[], None, Some("session"))
            .await
            .unwrap();
    assert_eq!(meeting, retry);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool.close().await;
}

#[tokio::test]
async fn browser_recovery_reuses_native_mapping_and_rejects_active_or_deleted_sessions() {
    let pool = database().await;
    assert!(
        TranscriptsRepository::recover_session_transcript(&pool, "Test", &[], None, "session")
            .await
            .is_err()
    );
    sqlx::query("UPDATE recording_sessions SET status='interrupted' WHERE id='session'")
        .execute(&pool)
        .await
        .unwrap();
    let recovered =
        TranscriptsRepository::save_session_transcript(&pool, "Test", &[], None, Some("session"))
            .await
            .unwrap();
    for _ in 0..3 {
        assert_eq!(
            TranscriptsRepository::recover_session_transcript(
                &pool,
                "Browser checkpoint",
                &[],
                None,
                "session"
            )
            .await
            .unwrap(),
            recovered
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    MeetingsRepository::delete_meeting(&pool, &recovered)
        .await
        .unwrap();
    assert!(
        TranscriptsRepository::recover_session_transcript(&pool, "Test", &[], None, "session")
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn interrupted_native_transcript_checkpoint_recovers_exactly_once() {
    let pool = database().await;
    let folder = tempfile::tempdir().unwrap();
    let segment = TranscriptSegment {
        id: "checkpoint".into(),
        text: "Recovered buffered words".into(),
        audio_start_time: 4.0,
        audio_end_time: 5.0,
        duration: 1.0,
        display_time: "12:00:05".into(),
        confidence: 0.9,
        sequence_id: 1,
    };
    tokio::fs::write(
        folder.path().join("transcripts.json"),
        serde_json::to_vec(&serde_json::json!({"segments":[segment]})).unwrap(),
    )
    .await
    .unwrap();
    tokio::fs::write(
        folder.path().join("metadata.json"),
        b"{\"status\":\"recording\"}",
    )
    .await
    .unwrap();
    sqlx::query("UPDATE recording_sessions SET folder_path=? WHERE id='session'")
        .bind(folder.path().to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    lifecycle::reconcile(&pool).await.unwrap();
    lifecycle::reconcile(&pool).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let text: String = sqlx::query_scalar("SELECT transcript FROM transcripts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(text, "Recovered buffered words");
    let metadata: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(folder.path().join("metadata.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(metadata["meeting_id"], "meeting-session");
    assert_eq!(metadata["status"], "interrupted");
    assert!(folder.path().join("transcripts.json").exists());
}

#[tokio::test]
async fn recovery_status_reports_saved_active_interrupted_and_unknown_sessions_without_writing() {
    let pool = database().await;
    let active = lifecycle::persisted_session_status(&pool, "session")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.status, "recording");
    assert!(active.meeting_id.is_none());
    assert!(lifecycle::persisted_session_status(&pool, "unknown")
        .await
        .unwrap()
        .is_none());
    let meeting =
        TranscriptsRepository::save_session_transcript(&pool, "Empty", &[], None, Some("session"))
            .await
            .unwrap();
    for status in ["completed", "interrupted", "error"] {
        sqlx::query("UPDATE recording_sessions SET status=? WHERE id='session'")
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        let native = lifecycle::persisted_session_status(&pool, "session")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(native.status, status);
        assert_eq!(native.meeting_id.as_deref(), Some(meeting.as_str()));
        let wire = serde_json::to_value(native).unwrap();
        assert_eq!(wire["meetingId"], meeting);
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn session_migration_preserves_existing_meetings_and_rejects_unsafe_downgrade() {
    const SESSION_VERSION: i64 = 20261006000000;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let mut legacy = sqlx::migrate!("./migrations");
    legacy.migrations = std::borrow::Cow::Owned(
        legacy
            .migrations
            .iter()
            .filter(|migration| migration.version < SESSION_VERSION)
            .cloned()
            .collect(),
    );
    legacy.run(&pool).await.unwrap();
    let original = TranscriptsRepository::save_transcript(
        &pool,
        "Existing meeting",
        &[crate::api::TranscriptSegment {
            id: "legacy-segment".into(),
            text: "Existing transcript remains intact".into(),
            timestamp: "12:00:00".into(),
            audio_start_time: Some(0.0),
            audio_end_time: Some(1.0),
            duration: Some(1.0),
        }],
        None,
    )
    .await
    .unwrap();

    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    assert_eq!(
        MeetingsRepository::get_meeting(&pool, &original)
            .await
            .unwrap()
            .unwrap()
            .title,
        "Existing meeting"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT transcript FROM transcripts WHERE meeting_id=?")
            .bind(&original)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "Existing transcript remains intact"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert!(matches!(
        legacy.run(&pool).await,
        Err(sqlx::migrate::MigrateError::VersionMissing(SESSION_VERSION))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
}
