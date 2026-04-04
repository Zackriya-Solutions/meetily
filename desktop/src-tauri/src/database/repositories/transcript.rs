use crate::api::TranscriptSegment;
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

#[derive(Debug, Clone, Default)]
pub struct SaveTranscriptOptions {
    pub source_type: Option<String>,
    pub language: Option<String>,
    pub duration_seconds: Option<f64>,
    pub recording_started_at: Option<String>,
    pub recording_ended_at: Option<String>,
    pub markdown_export_path: Option<String>,
    pub processing_version: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TranscriptSearchFilters {
    pub query: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub source_type: Option<String>,
    pub has_summary: Option<bool>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone)]
pub struct TranscriptSearchHit {
    pub id: String,
    pub title: String,
    pub match_context: String,
    pub timestamp: String,
    pub score: f64,
    pub source_type: String,
    pub has_summary: bool,
}

#[derive(Debug, Clone)]
pub struct TranscriptSearchPage {
    pub items: Vec<TranscriptSearchHit>,
    pub total_count: i64,
    pub limit: i64,
    pub offset: i64,
}

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        options: SaveTranscriptOptions,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();
        let source_type = options.source_type.unwrap_or_else(|| "recorded".to_string());
        let processing_version = options
            .processing_version
            .unwrap_or_else(|| "v0.2.0".to_string());

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (
                id, title, created_at, updated_at, folder_path,
                source_type, language, duration_seconds, recording_started_at, recording_ended_at, markdown_export_path
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .bind(&source_type)
        .bind(&options.language)
        .bind(options.duration_seconds)
        .bind(&options.recording_started_at)
        .bind(&options.recording_ended_at)
        .bind(&options.markdown_export_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            let result = sqlx::query(
                "INSERT INTO transcripts (
                    id, meeting_id, transcript, raw_transcript, processing_version, timestamp,
                    audio_start_time, audio_end_time, duration
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(segment.raw_text.as_ref().unwrap_or(&segment.text))
            .bind(&processing_version)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Searches transcript content with optional filters and pagination.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        filters: &TranscriptSearchFilters,
    ) -> Result<TranscriptSearchPage, SqlxError> {
        let limit = filters.limit.clamp(1, 200);
        let offset = filters.offset.max(0);
        let trimmed_query = filters.query.clone().unwrap_or_default().trim().to_string();

        if trimmed_query.is_empty() {
            return Self::search_without_query(pool, filters, limit, offset).await;
        }

        let match_query = build_fts_match_query(&trimmed_query);

        let total_row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*)
             FROM transcripts_fts
             JOIN transcripts t ON t.rowid = transcripts_fts.rowid
             JOIN meetings m ON m.id = t.meeting_id
             WHERE transcripts_fts MATCH ?
               AND (? IS NULL OR m.created_at >= ?)
               AND (? IS NULL OR m.created_at <= ?)
               AND (? IS NULL OR m.source_type = ?)
               AND (
                    ? IS NULL OR
                    (? = 1 AND EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL)) OR
                    (? = 0 AND NOT EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL))
               )",
        )
        .bind(&match_query)
        .bind(&filters.date_from)
        .bind(&filters.date_from)
        .bind(&filters.date_to)
        .bind(&filters.date_to)
        .bind(&filters.source_type)
        .bind(&filters.source_type)
        .bind(filters.has_summary)
        .bind(filters.has_summary)
        .bind(filters.has_summary)
        .fetch_one(pool)
        .await?;

        let rows = sqlx::query_as::<_, (String, String, String, String, f64, String, i64)>(
            "SELECT
                m.id,
                m.title,
                snippet(transcripts_fts, 0, '[', ']', ' ... ', 18) AS match_context,
                t.timestamp,
                (-bm25(transcripts_fts)) AS score,
                m.source_type,
                CASE
                    WHEN EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL) THEN 1
                    ELSE 0
                END AS has_summary
             FROM transcripts_fts
             JOIN transcripts t ON t.rowid = transcripts_fts.rowid
             JOIN meetings m ON m.id = t.meeting_id
             WHERE transcripts_fts MATCH ?
               AND (? IS NULL OR m.created_at >= ?)
               AND (? IS NULL OR m.created_at <= ?)
               AND (? IS NULL OR m.source_type = ?)
               AND (
                    ? IS NULL OR
                    (? = 1 AND EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL)) OR
                    (? = 0 AND NOT EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL))
               )
             ORDER BY score DESC, t.timestamp DESC
             LIMIT ? OFFSET ?",
        )
        .bind(&match_query)
        .bind(&filters.date_from)
        .bind(&filters.date_from)
        .bind(&filters.date_to)
        .bind(&filters.date_to)
        .bind(&filters.source_type)
        .bind(&filters.source_type)
        .bind(filters.has_summary)
        .bind(filters.has_summary)
        .bind(filters.has_summary)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        let items = rows
            .into_iter()
            .map(
                |(id, title, match_context, timestamp, score, source_type, has_summary)| {
                    TranscriptSearchHit {
                        id,
                        title,
                        match_context,
                        timestamp,
                        score,
                        source_type,
                        has_summary: has_summary == 1,
                    }
                },
            )
            .collect::<Vec<_>>();

        Ok(TranscriptSearchPage {
            items,
            total_count: total_row.0,
            limit,
            offset,
        })
    }

    async fn search_without_query(
        pool: &SqlitePool,
        filters: &TranscriptSearchFilters,
        limit: i64,
        offset: i64,
    ) -> Result<TranscriptSearchPage, SqlxError> {
        let total_row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*)
             FROM transcripts t
             JOIN meetings m ON m.id = t.meeting_id
             WHERE (? IS NULL OR m.created_at >= ?)
               AND (? IS NULL OR m.created_at <= ?)
               AND (? IS NULL OR m.source_type = ?)
               AND (
                    ? IS NULL OR
                    (? = 1 AND EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL)) OR
                    (? = 0 AND NOT EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL))
               )",
        )
        .bind(&filters.date_from)
        .bind(&filters.date_from)
        .bind(&filters.date_to)
        .bind(&filters.date_to)
        .bind(&filters.source_type)
        .bind(&filters.source_type)
        .bind(filters.has_summary)
        .bind(filters.has_summary)
        .bind(filters.has_summary)
        .fetch_one(pool)
        .await?;

        let rows = sqlx::query_as::<_, (String, String, String, String, f64, String, i64)>(
            "SELECT
                m.id,
                m.title,
                substr(t.transcript, 1, 240) AS match_context,
                t.timestamp,
                0.0 AS score,
                m.source_type,
                CASE
                    WHEN EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL) THEN 1
                    ELSE 0
                END AS has_summary
             FROM transcripts t
             JOIN meetings m ON m.id = t.meeting_id
             WHERE (? IS NULL OR m.created_at >= ?)
               AND (? IS NULL OR m.created_at <= ?)
               AND (? IS NULL OR m.source_type = ?)
               AND (
                    ? IS NULL OR
                    (? = 1 AND EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL)) OR
                    (? = 0 AND NOT EXISTS(SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id AND sp.result IS NOT NULL))
               )
             ORDER BY m.updated_at DESC, t.timestamp DESC
             LIMIT ? OFFSET ?",
        )
        .bind(&filters.date_from)
        .bind(&filters.date_from)
        .bind(&filters.date_to)
        .bind(&filters.date_to)
        .bind(&filters.source_type)
        .bind(&filters.source_type)
        .bind(filters.has_summary)
        .bind(filters.has_summary)
        .bind(filters.has_summary)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        let items = rows
            .into_iter()
            .map(
                |(id, title, match_context, timestamp, score, source_type, has_summary)| {
                    TranscriptSearchHit {
                        id,
                        title,
                        match_context,
                        timestamp,
                        score,
                        source_type,
                        has_summary: has_summary == 1,
                    }
                },
            )
            .collect::<Vec<_>>();

        Ok(TranscriptSearchPage {
            items,
            total_count: total_row.0,
            limit,
            offset,
        })
    }

    pub async fn search_transcripts_legacy(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchHit>, SqlxError> {
        let filters = TranscriptSearchFilters {
            query: Some(query.to_string()),
            limit: 50,
            offset: 0,
            ..Default::default()
        };

        let page = Self::search_transcripts(pool, &filters).await?;
        Ok(page.items)
    }
}

fn build_fts_match_query(query: &str) -> String {
    let tokens = query
        .split_whitespace()
        .map(|token| token.replace('"', "\"\""))
        .filter(|token| !token.trim().is_empty())
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        return String::new();
    }

    tokens
        .iter()
        .map(|token| format!("\"{}\"*", token))
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::{build_fts_match_query, TranscriptSearchFilters, TranscriptsRepository};
    use sqlx::SqlitePool;

    #[test]
    fn build_fts_match_query_quotes_and_prefixes_tokens() {
        let query = build_fts_match_query("weekly planning");
        assert_eq!(query, "\"weekly\"* AND \"planning\"*");
    }

    #[test]
    fn build_fts_match_query_escapes_quotes() {
        let query = build_fts_match_query("foo \"bar\"");
        assert_eq!(query, "\"foo\"* AND \"\"\"bar\"\"\"*");
    }

    #[test]
    fn build_fts_match_query_handles_empty_input() {
        let query = build_fts_match_query("   ");
        assert!(query.is_empty());
    }

    async fn setup_search_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("failed to create sqlite memory pool");

        sqlx::query(
            "CREATE TABLE meetings (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                source_type TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("failed to create meetings table");

        sqlx::query(
            "CREATE TABLE transcripts (
                id TEXT PRIMARY KEY NOT NULL,
                meeting_id TEXT NOT NULL,
                transcript TEXT NOT NULL,
                timestamp TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("failed to create transcripts table");

        sqlx::query(
            "CREATE TABLE summary_processes (
                id TEXT PRIMARY KEY NOT NULL,
                meeting_id TEXT NOT NULL,
                result TEXT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("failed to create summary_processes table");

        sqlx::query(
            "CREATE VIRTUAL TABLE transcripts_fts USING fts5(
                transcript,
                content='transcripts',
                content_rowid='rowid'
            )",
        )
        .execute(&pool)
        .await
        .expect("failed to create transcripts_fts table");

        pool
    }

    #[tokio::test]
    async fn search_transcripts_uses_fts_match_and_filters() {
        let pool = setup_search_pool().await;

        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, source_type)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind("meeting-1")
        .bind("Planning")
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .bind("recorded")
        .execute(&pool)
        .await
        .expect("failed to insert meeting row");

        let insert = sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp)
             VALUES (?, ?, ?, ?)",
        )
        .bind("tx-1")
        .bind("meeting-1")
        .bind("open ai roadmap discussion")
        .bind("2026-01-01T00:00:01Z")
        .execute(&pool)
        .await
        .expect("failed to insert transcript row");

        sqlx::query("INSERT INTO transcripts_fts (rowid, transcript) VALUES (?, ?)")
            .bind(insert.last_insert_rowid())
            .bind("open ai roadmap discussion")
            .execute(&pool)
            .await
            .expect("failed to seed fts row");

        sqlx::query(
            "INSERT INTO summary_processes (id, meeting_id, result)
             VALUES (?, ?, ?)",
        )
        .bind("sp-1")
        .bind("meeting-1")
        .bind("{\"ok\":true}")
        .execute(&pool)
        .await
        .expect("failed to insert summary row");

        let page = TranscriptsRepository::search_transcripts(
            &pool,
            &TranscriptSearchFilters {
                query: Some("open ai".to_string()),
                source_type: Some("recorded".to_string()),
                has_summary: Some(true),
                limit: 20,
                offset: 0,
                ..Default::default()
            },
        )
        .await
        .expect("search_transcripts should succeed");

        assert_eq!(page.total_count, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, "meeting-1");
        assert!(page.items[0].match_context.to_lowercase().contains("open"));
        assert!(page.items[0].has_summary);
        assert_eq!(page.items[0].source_type, "recorded");
    }
}
