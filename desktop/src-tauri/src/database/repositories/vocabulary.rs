use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct VocabularyEntry {
    pub id: String,
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub source_text: String,
    pub target_text: String,
    pub case_sensitive: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabularyUpsertInput {
    pub id: Option<String>,
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub source_text: String,
    pub target_text: String,
    pub case_sensitive: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct VocabularyRule {
    pub source_text: String,
    pub target_text: String,
    pub case_sensitive: bool,
}

pub struct VocabularyRepository;

impl VocabularyRepository {
    pub async fn list(
        pool: &SqlitePool,
        scope_type: Option<&str>,
        scope_id: Option<&str>,
    ) -> Result<Vec<VocabularyEntry>, sqlx::Error> {
        let rows = sqlx::query_as::<_, VocabularyEntry>(
            "SELECT id, scope_type, scope_id, source_text, target_text, case_sensitive, created_at, updated_at
             FROM vocabulary_entries
             WHERE (? IS NULL OR scope_type = ?)
               AND (? IS NULL OR COALESCE(scope_id, '') = ?)
             ORDER BY scope_type ASC, scope_id ASC, source_text ASC",
        )
        .bind(scope_type)
        .bind(scope_type)
        .bind(scope_id)
        .bind(scope_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn upsert(
        pool: &SqlitePool,
        input: &VocabularyUpsertInput,
    ) -> Result<VocabularyEntry, sqlx::Error> {
        let scope_type = input.scope_type.trim().to_lowercase();
        if scope_type != "global" && scope_type != "meeting" {
            return Err(sqlx::Error::Protocol(
                "scope_type must be either 'global' or 'meeting'".to_string(),
            ));
        }

        let scope_id = input
            .scope_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if scope_type == "meeting" && scope_id.is_none() {
            return Err(sqlx::Error::Protocol(
                "scope_id is required when scope_type is 'meeting'".to_string(),
            ));
        }

        let source_text = input.source_text.trim().to_string();
        let target_text = input.target_text.trim().to_string();
        if source_text.is_empty() || target_text.is_empty() {
            return Err(sqlx::Error::Protocol(
                "source_text and target_text are required".to_string(),
            ));
        }

        let id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("vocab-{}", Uuid::new_v4()));
        let now = Utc::now().to_rfc3339();
        let case_sensitive = input.case_sensitive.unwrap_or(false);

        sqlx::query(
            "INSERT INTO vocabulary_entries (
                id, scope_type, scope_id, source_text, target_text, case_sensitive, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                scope_type = excluded.scope_type,
                scope_id = excluded.scope_id,
                source_text = excluded.source_text,
                target_text = excluded.target_text,
                case_sensitive = excluded.case_sensitive,
                updated_at = excluded.updated_at",
        )
        .bind(&id)
        .bind(&scope_type)
        .bind(&scope_id)
        .bind(&source_text)
        .bind(&target_text)
        .bind(case_sensitive)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        Self::get_by_id(pool, &id).await?.ok_or_else(|| {
            sqlx::Error::Protocol("vocabulary entry was not found after upsert".to_string())
        })
    }

    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM vocabulary_entries WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_effective_rules_for_meeting(
        pool: &SqlitePool,
        meeting_id: Option<&str>,
    ) -> Result<Vec<VocabularyRule>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String, String, bool, i64)>(
            "SELECT source_text, target_text, case_sensitive,
                    CASE
                      WHEN scope_type = 'meeting' THEN 2
                      WHEN scope_type = 'global' THEN 1
                      ELSE 0
                    END AS scope_rank
             FROM vocabulary_entries
             WHERE scope_type = 'global'
                OR (scope_type = 'meeting' AND ? IS NOT NULL AND scope_id = ?)
             ORDER BY scope_rank DESC, case_sensitive DESC, length(source_text) DESC, source_text ASC",
        )
        .bind(meeting_id)
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(source_text, target_text, case_sensitive, _)| VocabularyRule {
                source_text,
                target_text,
                case_sensitive,
            })
            .collect())
    }

    async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<VocabularyEntry>, sqlx::Error> {
        sqlx::query_as::<_, VocabularyEntry>(
            "SELECT id, scope_type, scope_id, source_text, target_text, case_sensitive, created_at, updated_at
             FROM vocabulary_entries WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("failed to create in-memory sqlite pool");

        sqlx::query(
            "CREATE TABLE vocabulary_entries (
                id TEXT PRIMARY KEY NOT NULL,
                scope_type TEXT NOT NULL CHECK (scope_type IN ('global', 'meeting')),
                scope_id TEXT NULL,
                source_text TEXT NOT NULL,
                target_text TEXT NOT NULL,
                case_sensitive INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("failed to create vocabulary_entries table");

        sqlx::query(
            "CREATE INDEX idx_vocabulary_entries_scope
             ON vocabulary_entries(scope_type, scope_id)",
        )
        .execute(&pool)
        .await
        .expect("failed to create scope index");

        sqlx::query(
            "CREATE UNIQUE INDEX idx_vocab_unique_ci
             ON vocabulary_entries(scope_type, coalesce(scope_id, ''), lower(source_text))
             WHERE case_sensitive = 0",
        )
        .execute(&pool)
        .await
        .expect("failed to create ci unique index");

        sqlx::query(
            "CREATE UNIQUE INDEX idx_vocab_unique_cs
             ON vocabulary_entries(scope_type, coalesce(scope_id, ''), source_text)
             WHERE case_sensitive = 1",
        )
        .execute(&pool)
        .await
        .expect("failed to create cs unique index");

        pool
    }

    #[tokio::test]
    async fn upsert_requires_scope_id_for_meeting_scope() {
        let pool = setup_pool().await;
        let result = VocabularyRepository::upsert(
            &pool,
            &VocabularyUpsertInput {
                id: None,
                scope_type: "meeting".to_string(),
                scope_id: None,
                source_text: "open ai".to_string(),
                target_text: "OpenAI".to_string(),
                case_sensitive: Some(false),
            },
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_filters_scope_and_scope_id() {
        let pool = setup_pool().await;

        VocabularyRepository::upsert(
            &pool,
            &VocabularyUpsertInput {
                id: Some("global-1".to_string()),
                scope_type: "global".to_string(),
                scope_id: None,
                source_text: "meet free".to_string(),
                target_text: "MeetFree".to_string(),
                case_sensitive: Some(false),
            },
        )
        .await
        .expect("global upsert should succeed");

        VocabularyRepository::upsert(
            &pool,
            &VocabularyUpsertInput {
                id: Some("meeting-1".to_string()),
                scope_type: "meeting".to_string(),
                scope_id: Some("m-1".to_string()),
                source_text: "open ai".to_string(),
                target_text: "OpenAI".to_string(),
                case_sensitive: Some(false),
            },
        )
        .await
        .expect("meeting upsert should succeed");

        let globals = VocabularyRepository::list(&pool, Some("global"), None)
            .await
            .expect("global listing should succeed");
        assert_eq!(globals.len(), 1);
        assert_eq!(globals[0].scope_type, "global");

        let meeting_rules = VocabularyRepository::list(&pool, Some("meeting"), Some("m-1"))
            .await
            .expect("meeting listing should succeed");
        assert_eq!(meeting_rules.len(), 1);
        assert_eq!(meeting_rules[0].scope_id.as_deref(), Some("m-1"));
    }

    #[tokio::test]
    async fn effective_rules_obey_scope_case_and_length_order() {
        let pool = setup_pool().await;

        let rows = vec![
            VocabularyUpsertInput {
                id: Some("g-short".to_string()),
                scope_type: "global".to_string(),
                scope_id: None,
                source_text: "meet".to_string(),
                target_text: "MEET".to_string(),
                case_sensitive: Some(false),
            },
            VocabularyUpsertInput {
                id: Some("g-long".to_string()),
                scope_type: "global".to_string(),
                scope_id: None,
                source_text: "meetfree".to_string(),
                target_text: "MeetFree".to_string(),
                case_sensitive: Some(false),
            },
            VocabularyUpsertInput {
                id: Some("m-case-sensitive".to_string()),
                scope_type: "meeting".to_string(),
                scope_id: Some("meeting-a".to_string()),
                source_text: "OpenAI".to_string(),
                target_text: "OpenAI-CS".to_string(),
                case_sensitive: Some(true),
            },
            VocabularyUpsertInput {
                id: Some("m-case-insensitive".to_string()),
                scope_type: "meeting".to_string(),
                scope_id: Some("meeting-a".to_string()),
                source_text: "meetfree".to_string(),
                target_text: "MeetingScopedMeetFree".to_string(),
                case_sensitive: Some(false),
            },
        ];

        for input in rows {
            VocabularyRepository::upsert(&pool, &input)
                .await
                .expect("upsert should succeed");
        }

        let ordered = VocabularyRepository::get_effective_rules_for_meeting(&pool, Some("meeting-a"))
            .await
            .expect("rule lookup should succeed");

        assert_eq!(ordered.len(), 4);
        assert_eq!(ordered[0].source_text, "OpenAI");
        assert_eq!(ordered[0].target_text, "OpenAI-CS");
        assert!(ordered[0].case_sensitive);

        assert_eq!(ordered[1].source_text, "meetfree");
        assert_eq!(ordered[1].target_text, "MeetingScopedMeetFree");
        assert!(!ordered[1].case_sensitive);

        assert_eq!(ordered[2].source_text, "meetfree");
        assert_eq!(ordered[2].target_text, "MeetFree");
        assert_eq!(ordered[3].source_text, "meet");
    }
}
