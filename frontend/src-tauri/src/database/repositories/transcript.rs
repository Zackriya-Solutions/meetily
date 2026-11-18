use crate::api::{TranscriptSearchResult, TranscriptSegment};
use crate::database::models::MeetingSpeaker;
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use log::{error, info, warn};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        transcription_incomplete: bool,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path, transcription_incomplete) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .bind(transcription_incomplete)
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
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .bind(&segment.speaker)
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

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// The distinct speakers of a meeting with their segment counts, ordered
    /// by first appearance (the transcript view's `audio_start_time` order).
    /// Rows with a NULL speaker are left out.
    pub async fn get_meeting_speakers(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<MeetingSpeaker>, SqlxError> {
        sqlx::query_as::<_, MeetingSpeaker>(
            "SELECT speaker, COUNT(*) AS segment_count, MIN(audio_start_time) AS first_start
             FROM transcripts
             WHERE meeting_id = ? AND speaker IS NOT NULL
             GROUP BY speaker
             ORDER BY first_start ASC, MIN(timestamp) ASC, speaker ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
    }

    /// Rewrite `old_speaker` to `new_speaker` on every segment of one meeting.
    /// Renaming onto a name another speaker already has merges the two.
    /// Bumps the meeting's `updated_at` when any row changed. Returns the
    /// number of segments rewritten.
    pub async fn rename_meeting_speaker(
        pool: &SqlitePool,
        meeting_id: &str,
        old_speaker: &str,
        new_speaker: &str,
    ) -> Result<u64, SqlxError> {
        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let rows = sqlx::query(
            "UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND speaker = ?",
        )
        .bind(new_speaker)
        .bind(meeting_id)
        .bind(old_speaker)
        .execute(&mut *transaction)
        .await?
        .rows_affected();

        if rows > 0 {
            sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
                .bind(Utc::now().naive_utc())
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
        }

        transaction.commit().await?;
        Ok(rows)
    }

    /// Searches for a query string within the transcripts.
    /// Returns at most one result per meeting (its best-matching segment),
    /// ranked by relevance. Uses the FTS5 index; if the query cannot be
    /// expressed as a MATCH expression or the FTS query fails, falls back to
    /// the original substring scan so search never breaks outright.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(match_expr) = Self::to_fts_match_expr(trimmed) {
            match Self::search_transcripts_fts(pool, &match_expr).await {
                Ok(results) => return Ok(results),
                Err(e) => {
                    warn!("FTS transcript search failed; falling back to substring scan: {e}")
                }
            }
        }

        Self::search_transcripts_like(pool, trimmed).await
    }

    /// Build an FTS5 MATCH expression from free-form user input: each
    /// whitespace-separated token becomes a quoted phrase (embedded `"`
    /// doubled), joined with implicit AND; the last token is prefix-matched so
    /// search-as-you-type works. Returns `None` for empty input.
    fn to_fts_match_expr(input: &str) -> Option<String> {
        let tokens: Vec<&str> = input.split_whitespace().collect();
        let last = tokens.len().checked_sub(1)?;
        let parts: Vec<String> = tokens
            .iter()
            .enumerate()
            .map(|(i, token)| {
                let escaped = token.replace('"', "\"\"");
                if i == last {
                    format!("\"{escaped}\"*")
                } else {
                    format!("\"{escaped}\"")
                }
            })
            .collect();
        Some(parts.join(" "))
    }

    /// FTS5-backed search. FTS auxiliary functions (`bm25`, `snippet`) are
    /// only valid in a plain SELECT over the FTS table with a MATCH
    /// constraint, so the query returns matching segments ranked by relevance
    /// and the one-result-per-meeting dedupe happens here: the first segment
    /// seen for a meeting is its best-ranked one. The row limit bounds the
    /// scan; 400 candidate segments is plenty to fill 50 meetings.
    async fn search_transcripts_fts(
        pool: &SqlitePool,
        match_expr: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        const MAX_MEETINGS: usize = 50;

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title,
                    snippet(transcripts_fts, 0, '', '', '…', 24),
                    t.timestamp
             FROM transcripts_fts
             JOIN transcripts t ON t.rowid = transcripts_fts.rowid
             JOIN meetings m ON m.id = t.meeting_id
             WHERE transcripts_fts MATCH ?
             ORDER BY bm25(transcripts_fts)
             LIMIT 400",
        )
        .bind(match_expr)
        .fetch_all(pool)
        .await?;

        let mut seen = std::collections::HashSet::new();
        Ok(rows
            .into_iter()
            .filter(|(id, _, _, _)| seen.insert(id.clone()))
            .take(MAX_MEETINGS)
            .map(|(id, title, match_context, timestamp)| TranscriptSearchResult {
                id,
                title,
                match_context,
                timestamp,
            })
            .collect())
    }

    /// Pre-FTS substring scan, kept as the fallback path.
    async fn search_transcripts_like(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
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

    fn seg(text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: String::new(),
            text: text.to_string(),
            timestamp: "2026-07-23T10:00:00Z".to_string(),
            audio_start_time: Some(0.0),
            audio_end_time: Some(1.0),
            duration: Some(1.0),
            speaker: None,
        }
    }

    async fn save_meeting(pool: &SqlitePool, title: &str, texts: &[&str]) -> String {
        let segments: Vec<TranscriptSegment> = texts.iter().map(|t| seg(t)).collect();
        TranscriptsRepository::save_transcript(pool, title, &segments, None, false)
            .await
            .expect("save_transcript")
    }

    #[tokio::test]
    async fn save_transcript_persists_speaker() {
        let pool = test_pool().await;
        let mut labeled = seg("hello from the mic");
        labeled.speaker = Some("mic".to_string());
        let unlabeled = seg("ambiguous crosstalk");

        let meeting_id =
            TranscriptsRepository::save_transcript(&pool, "Labeled", &[labeled, unlabeled], None, false)
                .await
                .unwrap();

        let rows: Vec<(Option<String>,)> = sqlx::query_as(
            "SELECT speaker FROM transcripts WHERE meeting_id = ? ORDER BY transcript",
        )
        .bind(&meeting_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![(None,), (Some("mic".to_string()),)],
            "speaker must round-trip; ambiguous segments stay NULL"
        );
    }

    #[tokio::test]
    async fn fts_table_and_triggers_exist_after_migrations() {
        let pool = test_pool().await;
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN
             ('transcripts_fts', 'transcripts_fts_ai', 'transcripts_fts_ad', 'transcripts_fts_au')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 4, "FTS5 table and its three sync triggers must exist");
    }

    /// Guards against the FTS path silently erroring and every other test
    /// passing via the LIKE fallback: this one calls the FTS query directly.
    #[tokio::test]
    async fn fts_path_itself_succeeds() {
        let pool = test_pool().await;
        save_meeting(&pool, "Direct", &["capacitor one", "capacitor two"]).await;

        let results = TranscriptsRepository::search_transcripts_fts(&pool, "\"capacitor\"*")
            .await
            .expect("the FTS query itself must not error");
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn finds_content_with_snippet_and_meeting_title() {
        let pool = test_pool().await;
        let id = save_meeting(
            &pool,
            "Quarterly Review",
            &["we discussed the flux capacitor budget at length"],
        )
        .await;

        let results = TranscriptsRepository::search_transcripts(&pool, "capacitor")
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
        assert_eq!(results[0].title, "Quarterly Review");
        assert!(
            results[0].match_context.contains("capacitor"),
            "snippet should contain the matched term: {}",
            results[0].match_context
        );
    }

    #[tokio::test]
    async fn returns_one_result_per_meeting() {
        let pool = test_pool().await;
        save_meeting(
            &pool,
            "Standup",
            &["capacitor talk one", "capacitor talk two", "unrelated"],
        )
        .await;

        let results = TranscriptsRepository::search_transcripts(&pool, "capacitor")
            .await
            .unwrap();
        assert_eq!(results.len(), 1, "multiple matching segments must dedupe to the meeting");
    }

    #[tokio::test]
    async fn prefix_matches_the_last_token() {
        let pool = test_pool().await;
        save_meeting(&pool, "Standup", &["reviewing the capacitor design"]).await;

        let results = TranscriptsRepository::search_transcripts(&pool, "capaci")
            .await
            .unwrap();
        assert_eq!(results.len(), 1, "search-as-you-type prefix should match");
    }

    #[tokio::test]
    async fn multi_word_query_requires_all_terms() {
        let pool = test_pool().await;
        save_meeting(&pool, "Budget", &["the flux capacitor budget is fine"]).await;

        let hit = TranscriptsRepository::search_transcripts(&pool, "flux budget")
            .await
            .unwrap();
        assert_eq!(hit.len(), 1);

        let miss = TranscriptsRepository::search_transcripts(&pool, "flux spaceship")
            .await
            .unwrap();
        assert!(miss.is_empty(), "a term that appears nowhere must exclude the meeting");
    }

    #[tokio::test]
    async fn retranscription_delete_and_reinsert_keeps_index_consistent() {
        let pool = test_pool().await;
        let meeting_id = save_meeting(&pool, "Sync", &["the old wording about zeppelins"]).await;

        // Mirror retranscription.rs: delete all segments, insert replacements,
        // in one transaction — the FTS triggers must track both operations.
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
            .bind(&meeting_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("transcript-new")
        .bind(&meeting_id)
        .bind("the new wording about dirigibles")
        .bind("2026-07-23T11:00:00Z")
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let old = TranscriptsRepository::search_transcripts(&pool, "zeppelins")
            .await
            .unwrap();
        assert!(old.is_empty(), "replaced text must leave no ghost hits");

        let new = TranscriptsRepository::search_transcripts(&pool, "dirigibles")
            .await
            .unwrap();
        assert_eq!(new.len(), 1);
    }

    #[tokio::test]
    async fn deleted_transcripts_leave_no_ghost_hits() {
        let pool = test_pool().await;
        let meeting_id = save_meeting(&pool, "Gone", &["ephemeral zeppelin content"]).await;

        sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
            .bind(&meeting_id)
            .execute(&pool)
            .await
            .unwrap();

        let results = TranscriptsRepository::search_transcripts(&pool, "zeppelin")
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn hostile_queries_return_ok() {
        let pool = test_pool().await;
        save_meeting(&pool, "Any", &["plain content"]).await;

        for query in ["\"unclosed", "a OR b", "*", "\"", "café *", "NEAR(", "-"] {
            let result = TranscriptsRepository::search_transcripts(&pool, query).await;
            assert!(result.is_ok(), "query {query:?} must not surface an error");
        }
    }

    #[tokio::test]
    async fn empty_query_returns_empty() {
        let pool = test_pool().await;
        save_meeting(&pool, "Any", &["content"]).await;

        for query in ["", "   ", "\t"] {
            let results = TranscriptsRepository::search_transcripts(&pool, query)
                .await
                .unwrap();
            assert!(results.is_empty());
        }
    }

    #[tokio::test]
    async fn best_matching_meeting_ranks_first() {
        let pool = test_pool().await;
        save_meeting(
            &pool,
            "Barely Mentions It",
            &["a very long discussion about many topics that once in passing mentions the budget among a sea of other words spanning the whole meeting"],
        )
        .await;
        let dense = save_meeting(&pool, "All About It", &["budget budget budget"]).await;

        let results = TranscriptsRepository::search_transcripts(&pool, "budget")
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, dense, "denser match must rank first");
    }

    fn spoken(text: &str, speaker: Option<&str>, start: f64) -> TranscriptSegment {
        TranscriptSegment {
            audio_start_time: Some(start),
            audio_end_time: Some(start + 1.0),
            speaker: speaker.map(str::to_string),
            ..seg(text)
        }
    }

    async fn speakers_by_row(pool: &SqlitePool, meeting_id: &str) -> Vec<(String, Option<String>)> {
        sqlx::query_as(
            "SELECT transcript, speaker FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn diarized_meeting(pool: &SqlitePool) -> String {
        TranscriptsRepository::save_transcript(
            pool,
            "Diarized",
            &[
                spoken("a", Some("Speaker 2"), 0.0),
                spoken("b", None, 1.0),
                spoken("c", Some("Speaker 1"), 2.0),
                spoken("d", Some("Speaker 2"), 3.0),
                spoken("e", Some("Speaker 1"), 4.0),
                spoken("f", Some("Speaker 2"), 5.0),
            ],
            None,
            false,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn meeting_speakers_are_listed_by_first_appearance_with_counts() {
        let pool = test_pool().await;
        let meeting_id = diarized_meeting(&pool).await;
        diarized_meeting(&pool).await; // another meeting's rows must not count

        let speakers = TranscriptsRepository::get_meeting_speakers(&pool, &meeting_id)
            .await
            .unwrap();
        assert_eq!(
            speakers,
            vec![
                MeetingSpeaker { speaker: "Speaker 2".into(), segment_count: 3, first_start: Some(0.0) },
                MeetingSpeaker { speaker: "Speaker 1".into(), segment_count: 2, first_start: Some(2.0) },
            ],
            "NULL speakers excluded; order is first appearance, not name"
        );
    }

    #[tokio::test]
    async fn meeting_speakers_of_unknown_meeting_is_empty() {
        let pool = test_pool().await;
        let speakers = TranscriptsRepository::get_meeting_speakers(&pool, "meeting-missing")
            .await
            .unwrap();
        assert!(speakers.is_empty());
    }

    #[tokio::test]
    async fn rename_touches_only_the_given_meeting_and_speaker() {
        let pool = test_pool().await;
        let meeting_id = diarized_meeting(&pool).await;
        let other_id = diarized_meeting(&pool).await;
        let other_before = speakers_by_row(&pool, &other_id).await;

        let rows = TranscriptsRepository::rename_meeting_speaker(&pool, &meeting_id, "Speaker 1", "Alice")
            .await
            .unwrap();

        assert_eq!(rows, 2);
        let s = |v: &str| Some(v.to_string());
        assert_eq!(
            speakers_by_row(&pool, &meeting_id).await,
            vec![
                ("a".into(), s("Speaker 2")),
                ("b".into(), None),
                ("c".into(), s("Alice")),
                ("d".into(), s("Speaker 2")),
                ("e".into(), s("Alice")),
                ("f".into(), s("Speaker 2")),
            ],
            "only Speaker 1 rows change; the NULL row stays NULL"
        );
        assert_eq!(speakers_by_row(&pool, &other_id).await, other_before);
    }

    #[tokio::test]
    async fn rename_onto_an_existing_name_merges_the_speakers() {
        let pool = test_pool().await;
        let meeting_id = diarized_meeting(&pool).await;

        TranscriptsRepository::rename_meeting_speaker(&pool, &meeting_id, "Speaker 1", "Speaker 2")
            .await
            .unwrap();

        let speakers = TranscriptsRepository::get_meeting_speakers(&pool, &meeting_id)
            .await
            .unwrap();
        assert_eq!(
            speakers,
            vec![MeetingSpeaker { speaker: "Speaker 2".into(), segment_count: 5, first_start: Some(0.0) }]
        );
    }

    #[tokio::test]
    async fn rename_matches_the_stored_value_exactly() {
        let pool = test_pool().await;
        let meeting_id = diarized_meeting(&pool).await;

        for near_miss in ["speaker 1", "Speaker 1 ", "Speaker"] {
            let rows = TranscriptsRepository::rename_meeting_speaker(&pool, &meeting_id, near_miss, "Alice")
                .await
                .unwrap();
            assert_eq!(rows, 0, "{near_miss:?} must not match \"Speaker 1\"");
        }
    }

    #[tokio::test]
    async fn rename_bumps_updated_at_only_when_rows_change() {
        let pool = test_pool().await;
        let meeting_id = diarized_meeting(&pool).await;
        let stamp = "2000-01-01 00:00:00";
        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(stamp)
            .bind(&meeting_id)
            .execute(&pool)
            .await
            .unwrap();
        let updated_at = || async {
            sqlx::query_as::<_, (String,)>("SELECT updated_at FROM meetings WHERE id = ?")
                .bind(&meeting_id)
                .fetch_one(&pool)
                .await
                .unwrap()
                .0
        };

        TranscriptsRepository::rename_meeting_speaker(&pool, &meeting_id, "Nobody", "Alice")
            .await
            .unwrap();
        assert_eq!(updated_at().await, stamp, "a no-op rename leaves updated_at alone");

        TranscriptsRepository::rename_meeting_speaker(&pool, &meeting_id, "Speaker 1", "Alice")
            .await
            .unwrap();
        assert_ne!(updated_at().await, stamp);
    }

    #[test]
    fn match_expr_quotes_tokens_and_prefixes_last() {
        assert_eq!(
            TranscriptsRepository::to_fts_match_expr("flux capac"),
            Some("\"flux\" \"capac\"*".to_string())
        );
        assert_eq!(
            TranscriptsRepository::to_fts_match_expr("say \"hi\""),
            Some("\"say\" \"\"\"hi\"\"\"*".to_string())
        );
        assert_eq!(TranscriptsRepository::to_fts_match_expr("   "), None);
        assert_eq!(TranscriptsRepository::to_fts_match_expr(""), None);
    }
}
