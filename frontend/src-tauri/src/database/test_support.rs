//! Shared fixtures for database tests.
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

pub struct SeedRow {
    pub id: &'static str,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub speaker: Option<&'static str>,
    pub text: &'static str,
}

/// In-memory database with every real migration applied.
/// A single connection keeps the in-memory database alive for the pool's lifetime.
pub async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

pub async fn seed_meeting(pool: &SqlitePool, meeting_id: &str, rows: &[SeedRow]) {
    let now = chrono::Utc::now();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, NULL)")
        .bind(meeting_id)
        .bind("Test meeting")
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("insert meeting");
    for row in rows {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(row.id)
        .bind(meeting_id)
        .bind(row.text)
        .bind("2026-09-27T10:00:00Z")
        .bind(row.start)
        .bind(row.end)
        .bind(match (row.start, row.end) {
            (Some(s), Some(e)) => Some(e - s),
            _ => None,
        })
        .bind(row.speaker)
        .execute(pool)
        .await
        .expect("insert transcript");
    }
}
