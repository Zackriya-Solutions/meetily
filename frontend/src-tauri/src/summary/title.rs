//! Auto title: names a meeting from its transcript as soon as the transcript
//! is saved, independent of summary generation.
//!
//! A meeting is only renamed while it still carries a name the app picked
//! (the "Meeting 11_11_26_14_05_09" timestamp, or an imported file's name), so
//! a name the user typed is never overwritten.

use crate::database::repositories::setting::SettingsRepository;
use crate::summary::llm_client::{generate_summary, LLMProvider};
use crate::summary::processor::{clean_llm_markdown_detailed, THINK_MARKER_REGEX};
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use log::{info, warn};

// Roughly 20 English words, or two or three sentences of Japanese.
const MIN_CHARS: usize = 80;
// About 1.5k tokens: enough to find the topic, and small enough that the title
// does not hold up the built-in model's summary, which shares one sidecar.
const HEAD_CHARS: usize = 4500;
const TAIL_CHARS: usize = 1500;
const MAX_TITLE_CHARS: usize = 80;

const SYSTEM_PROMPT: &str = "You name meeting recordings. Reply with only the title: \
3 to 8 words, in the same language as the transcript, describing the main topic. \
No quotes, no markdown, no date, no trailing punctuation.";

#[derive(Clone, Serialize)]
pub struct MeetingTitleGenerated {
    pub meeting_id: String,
    pub title: String,
}

fn is_digit_groups(s: &str, sep: char, widths: &[usize]) -> bool {
    let parts: Vec<&str> = s.split(sep).collect();
    parts.len() == widths.len()
        && parts
            .iter()
            .zip(widths)
            .all(|(p, w)| p.len() == *w && p.chars().all(|c| c.is_ascii_digit()))
}

/// True for the names the frontend gives new meetings: "Meeting 11_11_26_14_05_09"
/// (useRecordingStart), "Meeting 2026-11-11_14-05-09" (TranscriptContext) and the
/// "New Meeting" fallback (useRecordingStop).
pub fn is_default_meeting_title(title: &str) -> bool {
    let title = title.trim();
    if title == "New Meeting" {
        return true;
    }
    let Some(rest) = title.strip_prefix("Meeting ") else {
        return false;
    };
    if is_digit_groups(rest, '_', &[2, 2, 2, 2, 2, 2]) {
        return true;
    }
    match rest.split_once('_') {
        Some((date, time)) => {
            is_digit_groups(date, '-', &[4, 2, 2]) && is_digit_groups(time, '-', &[2, 2, 2])
        }
        None => false,
    }
}

/// Keeps the start and end of long transcripts so the prompt stays small.
fn transcript_excerpt(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= HEAD_CHARS + TAIL_CHARS {
        return text.to_string();
    }
    let head: String = chars[..HEAD_CHARS].iter().collect();
    let tail: String = chars[chars.len() - TAIL_CHARS..].iter().collect();
    format!("{head}\n[...]\n{tail}")
}

/// True for a title that names nothing, such as "Meeting Recording Title":
/// small models sometimes answer with a placeholder instead of a topic.
fn is_placeholder_title(title: &str) -> bool {
    const GENERIC: &[&str] = &[
        "a", "an", "the", "of", "for", "meeting", "meetings", "recording", "title", "transcript",
        "call", "untitled", "new", "conversation", "summary", "notes", "audio", "discussion", "here",
        "פגישה", "פגישת", "הקלטה", "הקלטת", "כותרת", "שיחה", "שיחת", "חדשה", "תמלול", "סיכום", "דיון",
        "של", "ללא",
    ];
    title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .all(|w| GENERIC.contains(&w.to_lowercase().as_str()))
}

/// The answer part of a model reply, without reasoning blocks, or None if the
/// reply was cut off while still reasoning.
pub(crate) fn visible_reply(raw: &str) -> Option<String> {
    let visible = clean_llm_markdown_detailed(raw).markdown;
    // Some chat templates open the think block inside the prompt, so the reply
    // is bare reasoning followed by "</think>" and then the answer.
    // A last marker that opens a block means the reply was cut off mid-reasoning.
    match THINK_MARKER_REGEX.find_iter(&visible).last() {
        Some(marker) if !marker.as_str().starts_with("</") => None,
        Some(marker) => Some(visible[marker.end()..].to_string()),
        None => Some(visible),
    }
}

/// Turns raw model output into a usable title, or None if it is unusable.
fn clean_title(raw: &str) -> Option<String> {
    let visible = visible_reply(raw)?;
    let line = visible.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.trim_start_matches('#').trim();
    let line = line
        .strip_prefix("Title:")
        .or_else(|| line.strip_prefix("title:"))
        .unwrap_or(line)
        .trim();
    let line = line
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '*' | '“' | '”'))
        .trim_end_matches(|c: char| matches!(c, '.' | ':' | ';' | ','))
        .trim();

    if line.is_empty() || line.contains('<') || is_default_meeting_title(line) || is_placeholder_title(line) {
        return None;
    }
    let title: String = line.chars().take(MAX_TITLE_CHARS).collect();
    Some(title.trim().to_string())
}

/// Renames the meeting only if its title is still `expected_old`, so a rename
/// the user makes while the LLM is running wins.
async fn rename_if_unchanged(
    pool: &SqlitePool,
    meeting_id: &str,
    expected_old: &str,
    new_title: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let updated = sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ? AND title = ?")
        .bind(new_title)
        .bind(chrono::Utc::now())
        .bind(meeting_id)
        .bind(expected_old)
        .execute(&mut *tx)
        .await?;
    if updated.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
        .bind(new_title)
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// One completion from the configured summary model.
pub(crate) async fn ask_llm(
    pool: &SqlitePool,
    app_data_dir: Option<&PathBuf>,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let config = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|e| format!("failed to read model config: {e}"))?
        .ok_or("no summary model configured")?;
    let provider = LLMProvider::from_str(&config.provider)?;

    let api_key = match provider {
        LLMProvider::Ollama | LLMProvider::BuiltInAI | LLMProvider::CustomOpenAI => String::new(),
        _ => SettingsRepository::get_api_key(pool, &config.provider)
            .await
            .map_err(|e| format!("failed to read API key: {e}"))?
            .filter(|k| !k.is_empty())
            .ok_or_else(|| format!("no API key for {}", config.provider))?,
    };

    let ollama_endpoint = if provider == LLMProvider::Ollama {
        config.ollama_endpoint.clone()
    } else {
        None
    };

    let (custom_endpoint, custom_key) = if provider == LLMProvider::CustomOpenAI {
        let custom = SettingsRepository::get_custom_openai_config(pool)
            .await
            .map_err(|e| format!("failed to read custom OpenAI config: {e}"))?
            .ok_or("custom OpenAI provider selected but not configured")?;
        (Some(custom.endpoint), custom.api_key.unwrap_or_default())
    } else {
        (None, String::new())
    };
    let api_key = if provider == LLMProvider::CustomOpenAI { custom_key } else { api_key };

    let completion = generate_summary(
        &reqwest::Client::new(),
        &provider,
        &config.model,
        &api_key,
        system_prompt,
        user_prompt,
        ollama_endpoint.as_deref(),
        custom_endpoint.as_deref(),
        None,
        None,
        None,
        app_data_dir,
        None,
    )
    .await?;
    Ok(completion.content)
}

async fn current_title(pool: &SqlitePool, meeting_id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT title FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
}

/// Sets `new_title` if the meeting still has a default name, or exactly
/// `replaceable` (an imported file's name). Returns whether it was applied.
/// Shared with the summary path so neither overwrites a name the user (or the
/// other path) already set.
pub async fn apply_title_if_default(
    pool: &SqlitePool,
    meeting_id: &str,
    new_title: &str,
    replaceable: Option<&str>,
) -> Result<bool, sqlx::Error> {
    match current_title(pool, meeting_id).await? {
        Some(old) if is_default_meeting_title(&old) || replaceable == Some(old.as_str()) => {
            rename_if_unchanged(pool, meeting_id, &old, new_title).await
        }
        _ => Ok(false),
    }
}

/// Generates a title and applies it if the meeting still has a default name
/// (or `replaceable`). Failures are logged and otherwise ignored: a missing
/// title is not an error, and the summary heading can still fill it in later.
pub async fn generate_meeting_title<R: Runtime>(
    app: AppHandle<R>,
    pool: SqlitePool,
    meeting_id: String,
    transcript: String,
    replaceable: Option<String>,
) {
    match current_title(&pool, &meeting_id).await {
        Ok(Some(title))
            if is_default_meeting_title(&title)
                || replaceable.as_deref() == Some(title.as_str()) => {}
        Ok(_) => return,
        Err(e) => {
            warn!("Auto title for {meeting_id}: failed to read meeting: {e}");
            return;
        }
    }
    // Counted in characters because Chinese, Japanese and Thai don't separate words with spaces.
    if transcript.chars().filter(|c| !c.is_whitespace()).count() < MIN_CHARS {
        info!("Auto title skipped for {meeting_id}: transcript too short");
        return;
    }

    let app_data_dir = app.path().app_data_dir().ok();
    let user_prompt = format!("Transcript:\n{}", transcript_excerpt(&transcript));
    let raw = match ask_llm(&pool, app_data_dir.as_ref(), SYSTEM_PROMPT, &user_prompt).await {
        Ok(raw) => raw,
        Err(e) => {
            warn!("Auto title failed for {meeting_id}: {e}");
            return;
        }
    };
    let Some(title) = clean_title(&raw) else {
        warn!("Auto title for {meeting_id}: unusable model output ({} chars)", raw.chars().count());
        return;
    };

    match apply_title_if_default(&pool, &meeting_id, &title, replaceable.as_deref()).await {
        Ok(true) => {
            info!("Auto title saved for {meeting_id}");
            let _ = app.emit(
                "meeting-title-generated",
                MeetingTitleGenerated { meeting_id, title },
            );
        }
        Ok(false) => info!("Auto title for {meeting_id} dropped: meeting was renamed meanwhile"),
        Err(e) => warn!("Auto title for {meeting_id}: failed to save: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_titles_are_recognised() {
        assert!(is_default_meeting_title("Meeting 11_11_26_14_05_09"));
        assert!(is_default_meeting_title("Meeting 2026-11-11_14-05-09"));
        assert!(!is_default_meeting_title("Meeting with Alex"));
        assert!(!is_default_meeting_title("Q4 planning"));
        assert!(!is_default_meeting_title("Meeting "));
        assert!(!is_default_meeting_title("Meeting ___"));
        assert!(is_default_meeting_title("New Meeting"));
        assert!(!is_default_meeting_title("Meeting 3"));
        assert!(!is_default_meeting_title("Meeting 2026"));
        assert!(!is_default_meeting_title("Meeting 11_11_26"));
    }

    #[test]
    fn clean_title_strips_decoration() {
        assert_eq!(clean_title("# \"Q4 Budget Review.\"\n").as_deref(), Some("Q4 Budget Review"));
        assert_eq!(clean_title("\n\nTitle: Hiring plan for Berlin").as_deref(), Some("Hiring plan for Berlin"));
        assert_eq!(clean_title("**Launch retro**").as_deref(), Some("Launch retro"));
    }

    #[test]
    fn clean_title_rejects_junk() {
        assert_eq!(clean_title(""), None);
        assert_eq!(clean_title("   \n "), None);
        assert_eq!(clean_title("<Add Title here>"), None);
        assert_eq!(clean_title("Meeting 11_11_26_14_05_09"), None);
        assert_eq!(clean_title("Meeting Recording Title"), None);
        assert_eq!(clean_title("Title: Untitled meeting"), None);
        assert_eq!(clean_title("סיכום פגישה"), None);
        assert_eq!(clean_title("תכנון ספרינט").as_deref(), Some("תכנון ספרינט"));
        assert_eq!(clean_title("Discussion of Sports Parachute Maneuverability").as_deref(),
                   Some("Discussion of Sports Parachute Maneuverability"));
        assert_eq!(clean_title("<think>long reasoning</think>"), None);
        // Cut off at the token limit while still reasoning.
        assert_eq!(clean_title("<think>\nOkay, the user wants a short title. Let me"), None);
    }

    #[test]
    fn clean_title_skips_reasoning() {
        assert_eq!(
            clean_title("<think>The user wants a title.\nLet me look.</think>\nQ4 Budget Review").as_deref(),
            Some("Q4 Budget Review")
        );
        // The template opened the think block in the prompt: only the closing tag arrives.
        assert_eq!(
            clean_title("Okay, the user wants a short title.\nLet me look at it.\n</think>\n\nHiring plan for Berlin").as_deref(),
            Some("Hiring plan for Berlin")
        );
    }

    #[test]
    fn clean_title_caps_length() {
        let long = "word ".repeat(40);
        assert!(clean_title(&long).unwrap().chars().count() <= MAX_TITLE_CHARS);
    }

    #[test]
    fn excerpt_keeps_head_and_tail() {
        let text = format!("{}{}", "a".repeat(HEAD_CHARS + 5000), "END");
        let excerpt = transcript_excerpt(&text);
        assert!(excerpt.starts_with('a'));
        assert!(excerpt.ends_with("END"));
        assert!(excerpt.chars().count() < HEAD_CHARS + TAIL_CHARS + 10);
        assert_eq!(transcript_excerpt("short"), "short");
    }
}
