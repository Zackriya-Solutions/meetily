use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use tokio::fs;

use crate::{
    database::repositories::vocabulary::VocabularyRule, state::AppState,
    summary::contract::migrate_legacy_summary_payload,
};

#[derive(Debug, Clone)]
struct MeetingExportData {
    id: String,
    title: String,
    created_at: String,
    updated_at: String,
    folder_path: Option<String>,
    source_type: String,
    language: Option<String>,
    duration_seconds: Option<f64>,
}

#[derive(Debug, Clone)]
struct TranscriptExportRow {
    timestamp: String,
    text: String,
}

#[derive(Debug, Serialize)]
pub struct MeetingMarkdownExportResult {
    pub meeting_id: String,
    pub output_path: Option<String>,
    pub wrote_file: bool,
    pub markdown_preview: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MeetingMarkdownBatchExportResult {
    pub meeting_id: String,
    pub output_path: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MeetingsMarkdownBatchExportResponse {
    pub results: Vec<MeetingMarkdownBatchExportResult>,
}

#[tauri::command]
pub async fn meeting_export_markdown<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    destination_root: Option<String>,
    preview: Option<bool>,
) -> Result<MeetingMarkdownExportResult, String> {
    export_meeting_markdown(
        &app,
        state.db_manager.pool(),
        &meeting_id,
        destination_root,
        preview.unwrap_or(false),
    )
    .await
}

pub async fn export_meeting_markdown<R: Runtime>(
    app: &AppHandle<R>,
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    destination_root: Option<String>,
    preview_mode: bool,
) -> Result<MeetingMarkdownExportResult, String> {
    let meeting = fetch_meeting_export_data(pool, &meeting_id)
        .await?
        .ok_or_else(|| format!("Meeting not found: {}", meeting_id))?;

    let summary_markdown = fetch_summary_markdown(pool, &meeting_id).await?;
    let transcript_rows = fetch_transcript_rows(pool, &meeting_id).await?;
    let vocabulary = crate::vocabulary::get_effective_rules_for_meeting(pool, Some(meeting_id)).await?;
    let rendered_markdown = render_meeting_markdown(
        &meeting,
        &summary_markdown,
        &transcript_rows,
        &vocabulary,
    );

    if preview_mode {
        return Ok(MeetingMarkdownExportResult {
            meeting_id: meeting_id.to_string(),
            output_path: None,
            wrote_file: false,
            markdown_preview: Some(rendered_markdown),
        });
    }

    let destination_dir = resolve_single_destination_dir(app, &meeting, destination_root)?;
    let output_path = write_markdown_with_collision(&destination_dir, &meeting.title, &rendered_markdown)
        .await
        .map_err(|e| format!("Failed to write markdown export: {}", e))?;

    sqlx::query("UPDATE meetings SET markdown_export_path = ?, updated_at = ? WHERE id = ?")
        .bind(output_path.to_string_lossy().to_string())
        .bind(chrono::Utc::now())
        .bind(&meeting.id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to persist markdown export path: {}", e))?;

    Ok(MeetingMarkdownExportResult {
        meeting_id: meeting.id,
        output_path: Some(output_path.to_string_lossy().to_string()),
        wrote_file: true,
        markdown_preview: None,
    })
}

#[tauri::command]
pub async fn meetings_export_markdown_batch<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_ids: Vec<String>,
    destination_root: String,
    preview: Option<bool>,
) -> Result<MeetingsMarkdownBatchExportResponse, String> {
    if meeting_ids.is_empty() {
        return Ok(MeetingsMarkdownBatchExportResponse {
            results: Vec::new(),
        });
    }

    let pool = state.db_manager.pool();
    let root = PathBuf::from(destination_root);
    if !root.exists() {
        fs::create_dir_all(&root)
            .await
            .map_err(|e| format!("Failed to create destination root: {}", e))?;
    }

    let preview_mode = preview.unwrap_or(false);
    let mut results = Vec::with_capacity(meeting_ids.len());

    for meeting_id in meeting_ids {
        let export_result =
            export_single_batch_meeting(&app, pool, &meeting_id, &root, preview_mode).await;
        results.push(export_result);
    }

    Ok(MeetingsMarkdownBatchExportResponse { results })
}

async fn export_single_batch_meeting<R: Runtime>(
    app: &AppHandle<R>,
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    root: &Path,
    preview_mode: bool,
) -> MeetingMarkdownBatchExportResult {
    let _ = app;
    let result = async {
        let meeting = fetch_meeting_export_data(pool, meeting_id)
            .await?
            .ok_or_else(|| format!("Meeting not found: {}", meeting_id))?;
        let summary_markdown = fetch_summary_markdown(pool, meeting_id).await?;
        let transcript_rows = fetch_transcript_rows(pool, meeting_id).await?;
        let vocabulary =
            crate::vocabulary::get_effective_rules_for_meeting(pool, Some(meeting_id)).await?;
        let rendered_markdown =
            render_meeting_markdown(&meeting, &summary_markdown, &transcript_rows, &vocabulary);

        if preview_mode {
            return Ok::<Option<PathBuf>, String>(None);
        }

        let subfolder = root.join(format!(
            "{}-{}",
            sanitize_filename(&meeting.title),
            &meeting.id
        ));
        let output_path = write_markdown_with_collision(&subfolder, &meeting.title, &rendered_markdown)
            .await
            .map_err(|e| format!("Failed to write markdown export: {}", e))?;

        sqlx::query("UPDATE meetings SET markdown_export_path = ?, updated_at = ? WHERE id = ?")
            .bind(output_path.to_string_lossy().to_string())
            .bind(chrono::Utc::now())
            .bind(&meeting.id)
            .execute(pool)
            .await
            .map_err(|e| format!("Failed to persist markdown export path: {}", e))?;

        Ok(Some(output_path))
    }
    .await;

    match result {
        Ok(path) => MeetingMarkdownBatchExportResult {
            meeting_id: meeting_id.to_string(),
            output_path: path.map(|p| p.to_string_lossy().to_string()),
            success: true,
            error: None,
        },
        Err(error) => MeetingMarkdownBatchExportResult {
            meeting_id: meeting_id.to_string(),
            output_path: None,
            success: false,
            error: Some(error),
        },
    }
}

fn resolve_single_destination_dir<R: Runtime>(
    app: &AppHandle<R>,
    meeting: &MeetingExportData,
    destination_root: Option<String>,
) -> Result<PathBuf, String> {
    if let Some(root) = destination_root {
        return Ok(PathBuf::from(root));
    }

    if let Some(folder_path) = &meeting.folder_path {
        return Ok(PathBuf::from(folder_path));
    }

    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {}", e))?;
    Ok(app_data.join("exports").join(&meeting.id))
}

async fn write_markdown_with_collision(
    destination_dir: &Path,
    meeting_title: &str,
    markdown: &str,
) -> Result<PathBuf, std::io::Error> {
    fs::create_dir_all(destination_dir).await?;

    let base_name = if meeting_title.trim().is_empty() {
        "meeting".to_string()
    } else {
        sanitize_filename(meeting_title)
    };

    let mut candidate = destination_dir.join(format!("{}.md", base_name));
    let mut counter = 1usize;
    while candidate.exists() {
        candidate = destination_dir.join(format!("{}-{}.md", base_name, counter));
        counter += 1;
    }

    fs::write(&candidate, markdown).await?;
    Ok(candidate)
}

async fn fetch_meeting_export_data(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<Option<MeetingExportData>, String> {
    let row = sqlx::query_as::<_, (String, String, String, String, Option<String>, String, Option<String>, Option<f64>)>(
        "SELECT id, title, created_at, updated_at, folder_path, source_type, language, duration_seconds
         FROM meetings
         WHERE id = ?",
    )
    .bind(meeting_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch meeting metadata: {}", e))?;

    Ok(row.map(
        |(id, title, created_at, updated_at, folder_path, source_type, language, duration_seconds)| {
            MeetingExportData {
                id,
                title,
                created_at,
                updated_at,
                folder_path,
                source_type,
                language,
                duration_seconds,
            }
        },
    ))
}

async fn fetch_transcript_rows(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<Vec<TranscriptExportRow>, String> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT timestamp, transcript
         FROM transcripts
         WHERE meeting_id = ?
         ORDER BY audio_start_time ASC, timestamp ASC",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch transcripts for markdown export: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(timestamp, text)| TranscriptExportRow { timestamp, text })
        .collect())
}

async fn fetch_summary_markdown(pool: &sqlx::SqlitePool, meeting_id: &str) -> Result<String, String> {
    let row = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT result
         FROM summary_processes
         WHERE meeting_id = ?
         ORDER BY updated_at DESC
         LIMIT 1",
    )
    .bind(meeting_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch summary for markdown export: {}", e))?;

    let Some((raw_result,)) = row else {
        return Ok(String::new());
    };

    let Some(raw_result) = raw_result else {
        return Ok(String::new());
    };

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw_result) {
        if let Ok(payload) = migrate_legacy_summary_payload(&value) {
            return Ok(payload.markdown().trim().to_string());
        }

        if let Some(markdown) = value.get("markdown").and_then(|item| item.as_str()) {
            return Ok(markdown.trim().to_string());
        }
    }

    Ok(raw_result.trim().to_string())
}

fn render_meeting_markdown(
    meeting: &MeetingExportData,
    summary_markdown: &str,
    transcript_rows: &[TranscriptExportRow],
    vocabulary: &[VocabularyRule],
) -> String {
    let (summary_section, action_items_section, decisions_section) =
        split_summary_sections(summary_markdown);

    let transcript_markdown = if transcript_rows.is_empty() {
        "_No transcript available._".to_string()
    } else {
        transcript_rows
            .iter()
            .map(|row| {
                let corrected = crate::vocabulary::apply_vocabulary_rules(row.text.trim(), vocabulary);
                format!("- **{}** {}", row.timestamp, corrected)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut output = String::new();
    output.push_str("---\n");
    output.push_str(&format!("id: {}\n", yaml_quote(&meeting.id)));
    output.push_str(&format!("title: {}\n", yaml_quote(&meeting.title)));
    output.push_str(&format!("created_at: {}\n", yaml_quote(&meeting.created_at)));
    output.push_str(&format!("updated_at: {}\n", yaml_quote(&meeting.updated_at)));
    output.push_str(&format!("source_type: {}\n", yaml_quote(&meeting.source_type)));
    output.push_str(&format!(
        "language: {}\n",
        meeting
            .language
            .as_ref()
            .map(|language| yaml_quote(language))
            .unwrap_or_else(|| "null".to_string())
    ));
    output.push_str(&format!(
        "duration_seconds: {}\n",
        meeting
            .duration_seconds
            .map(|seconds| format!("{:.3}", seconds))
            .unwrap_or_else(|| "null".to_string())
    ));
    output.push_str(&format!(
        "transcript_count: {}\n",
        transcript_rows.len()
    ));
    output.push_str(&format!(
        "exported_at: {}\n",
        yaml_quote(&chrono::Utc::now().to_rfc3339())
    ));
    output.push_str("---\n\n");

    output.push_str("## Summary\n\n");
    output.push_str(&empty_section_fallback(&summary_section, "_No summary available._"));
    output.push_str("\n\n## Action Items\n\n");
    output.push_str(&empty_section_fallback(
        &action_items_section,
        "_No action items captured._",
    ));
    output.push_str("\n\n## Decisions\n\n");
    output.push_str(&empty_section_fallback(
        &decisions_section,
        "_No decisions captured._",
    ));
    output.push_str("\n\n## Transcript\n\n");
    output.push_str(&transcript_markdown);
    output.push('\n');
    output
}

fn split_summary_sections(summary_markdown: &str) -> (String, String, String) {
    let trimmed = summary_markdown.trim();
    if trimmed.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    let mut action_items = String::new();
    let mut decisions = String::new();
    let mut summary_lines = Vec::<String>::new();
    let mut current_section = "summary";

    for line in trimmed.lines() {
        let heading = parse_markdown_heading(line);
        if let Some(heading_text) = heading {
            let normalized = normalize_heading(&heading_text);
            if normalized.contains("action item") {
                current_section = "action";
                continue;
            }
            if normalized == "decisions"
                || normalized == "key decisions"
                || normalized.contains(" decision")
            {
                current_section = "decisions";
                continue;
            }
            current_section = "summary";
            summary_lines.push(line.to_string());
            continue;
        }

        match current_section {
            "action" => {
                if !action_items.is_empty() {
                    action_items.push('\n');
                }
                action_items.push_str(line);
            }
            "decisions" => {
                if !decisions.is_empty() {
                    decisions.push('\n');
                }
                decisions.push_str(line);
            }
            _ => summary_lines.push(line.to_string()),
        }
    }

    if action_items.trim().is_empty() && decisions.trim().is_empty() {
        return (trimmed.to_string(), String::new(), String::new());
    }

    (summary_lines.join("\n").trim().to_string(), action_items.trim().to_string(), decisions.trim().to_string())
}

fn parse_markdown_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return None;
    }
    let heading = trimmed.trim_start_matches('#').trim();
    if heading.is_empty() {
        return None;
    }
    Some(heading.to_string())
}

fn normalize_heading(input: &str) -> String {
    input
        .to_lowercase()
        .replace(':', "")
        .replace('-', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn empty_section_fallback(content: &str, fallback: &str) -> String {
    if content.trim().is_empty() {
        fallback.to_string()
    } else {
        content.trim().to_string()
    }
}

fn sanitize_filename(value: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string();

    sanitized = sanitized.replace(' ', "_");
    if sanitized.is_empty() {
        "meeting".to_string()
    } else {
        sanitized
    }
}

fn yaml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ");
    format!("\"{}\"", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::vocabulary::VocabularyRule;

    #[test]
    fn split_summary_sections_extracts_headings() {
        let input = r#"
## Summary
Status update
## Action Items
- Send notes
## Key Decisions
- Ship on Friday
"#;

        let (summary, actions, decisions) = split_summary_sections(input);
        assert!(summary.contains("Status update"));
        assert_eq!(actions.trim(), "- Send notes");
        assert_eq!(decisions.trim(), "- Ship on Friday");
    }

    #[test]
    fn render_meeting_markdown_includes_all_sections_and_vocabulary() {
        let meeting = MeetingExportData {
            id: "meeting-1".to_string(),
            title: "Weekly Sync".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            folder_path: None,
            source_type: "recorded".to_string(),
            language: Some("en".to_string()),
            duration_seconds: Some(123.0),
        };

        let transcript_rows = vec![TranscriptExportRow {
            timestamp: "2026-01-01T00:01:00Z".to_string(),
            text: "open ai roadmap".to_string(),
        }];

        let rules = vec![VocabularyRule {
            source_text: "open ai".to_string(),
            target_text: "OpenAI".to_string(),
            case_sensitive: false,
        }];

        let rendered = render_meeting_markdown(
            &meeting,
            "## Action Items\n- Follow up",
            &transcript_rows,
            &rules,
        );

        assert!(rendered.contains("## Summary"));
        assert!(rendered.contains("## Action Items"));
        assert!(rendered.contains("## Decisions"));
        assert!(rendered.contains("## Transcript"));
        assert!(rendered.contains("OpenAI roadmap"));
        assert!(rendered.contains("- Follow up"));
        assert!(rendered.contains("_No decisions captured._"));
    }

    #[tokio::test]
    async fn write_markdown_with_collision_appends_suffix() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let destination = dir.path().join("exports");

        let first = write_markdown_with_collision(&destination, "Weekly Sync", "# First")
            .await
            .expect("first write should succeed");
        let second = write_markdown_with_collision(&destination, "Weekly Sync", "# Second")
            .await
            .expect("second write should succeed");

        assert!(first.exists());
        assert!(second.exists());
        assert_ne!(first, second);
        assert!(
            second
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.contains("-1.md"))
                .unwrap_or(false),
            "second filename should include collision suffix: {}",
            second.display()
        );
    }
}
