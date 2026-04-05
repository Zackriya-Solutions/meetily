use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

use crate::{
    database::repositories::vocabulary::{
        VocabularyRepository, VocabularyRule, VocabularyUpsertInput,
    },
    preferences::{self, AppPreferences},
    state::AppState,
    transcript_processing,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptPostprocessPreviewResponse {
    pub cleaned_text: String,
    pub corrected_text: String,
    pub rule_count: usize,
}

pub fn apply_vocabulary_rules(text: &str, rules: &[VocabularyRule]) -> String {
    if text.trim().is_empty() || rules.is_empty() {
        return text.to_string();
    }

    let mut output = text.to_string();
    let mut protected_replacements: Vec<String> = Vec::new();
    for rule in rules {
        let source = rule.source_text.trim();
        let target = rule.target_text.trim();
        if source.is_empty() || target.is_empty() {
            continue;
        }

        let pattern = format!(r"\b{}\b", regex::escape(source));
        let mut builder = RegexBuilder::new(&pattern);
        builder.case_insensitive(!rule.case_sensitive);
        let Ok(regex) = builder.build() else {
            continue;
        };

        output = regex
            .replace_all(&output, |_captures: &regex::Captures| {
                let index = protected_replacements.len();
                protected_replacements.push(target.to_string());
                format!("<<MFV:{}>>", index)
            })
            .to_string();
    }

    for (index, replacement) in protected_replacements.iter().enumerate() {
        output = output.replace(&format!("<<MFV:{}>>", index), replacement);
    }

    output
}

pub async fn get_effective_rules_for_meeting(
    pool: &sqlx::SqlitePool,
    meeting_id: Option<&str>,
) -> Result<Vec<VocabularyRule>, String> {
    VocabularyRepository::get_effective_rules_for_meeting(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load vocabulary rules: {}", e))
}

pub async fn apply_to_meeting_text(
    pool: &sqlx::SqlitePool,
    meeting_id: Option<&str>,
    text: &str,
) -> Result<String, String> {
    let rules = get_effective_rules_for_meeting(pool, meeting_id).await?;
    Ok(apply_vocabulary_rules(text, &rules))
}

#[tauri::command]
pub async fn vocabulary_list<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    scope_type: Option<String>,
    scope_id: Option<String>,
) -> Result<Vec<crate::database::repositories::vocabulary::VocabularyEntry>, String> {
    let pool = state.db_manager.pool();
    VocabularyRepository::list(pool, scope_type.as_deref(), scope_id.as_deref())
        .await
        .map_err(|e| format!("Failed to list vocabulary entries: {}", e))
}

#[tauri::command]
pub async fn vocabulary_upsert<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    entry: VocabularyUpsertInput,
) -> Result<crate::database::repositories::vocabulary::VocabularyEntry, String> {
    let pool = state.db_manager.pool();
    VocabularyRepository::upsert(pool, &entry)
        .await
        .map_err(|e| format!("Failed to save vocabulary entry: {}", e))
}

#[tauri::command]
pub async fn vocabulary_delete<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let deleted = VocabularyRepository::delete(pool, &id)
        .await
        .map_err(|e| format!("Failed to delete vocabulary entry: {}", e))?;
    if deleted {
        Ok(())
    } else {
        Err("Vocabulary entry not found".to_string())
    }
}

#[tauri::command]
pub async fn transcript_postprocess_preview<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    text: String,
    meeting_id: Option<String>,
) -> Result<TranscriptPostprocessPreviewResponse, String> {
    let preferences: AppPreferences = preferences::load_app_preferences(&app)
        .await
        .unwrap_or_default();
    let cleaned = transcript_processing::clean_for_storage(&text, &preferences.transcript_cleanup);
    let rules =
        get_effective_rules_for_meeting(state.db_manager.pool(), meeting_id.as_deref()).await?;
    let corrected = apply_vocabulary_rules(&cleaned, &rules);

    Ok(TranscriptPostprocessPreviewResponse {
        cleaned_text: cleaned,
        corrected_text: corrected,
        rule_count: rules.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::apply_vocabulary_rules;
    use crate::database::repositories::vocabulary::VocabularyRule;

    #[test]
    fn apply_vocabulary_rules_respects_word_boundaries() {
        let corrected = apply_vocabulary_rules(
            "meetfree app meets meetfreedom",
            &[VocabularyRule {
                source_text: "meetfree".to_string(),
                target_text: "MeetFree".to_string(),
                case_sensitive: false,
            }],
        );

        assert_eq!(corrected, "MeetFree app meets meetfreedom");
    }

    #[test]
    fn apply_vocabulary_rules_respects_case_sensitivity() {
        let rules = vec![
            VocabularyRule {
                source_text: "OpenAI".to_string(),
                target_text: "OpenAI-CS".to_string(),
                case_sensitive: true,
            },
            VocabularyRule {
                source_text: "openai".to_string(),
                target_text: "OpenAI-CI".to_string(),
                case_sensitive: false,
            },
        ];

        let corrected = apply_vocabulary_rules("OpenAI and openai", &rules);
        assert_eq!(corrected, "OpenAI-CS and OpenAI-CI");
    }
}
