// Transcript import - create a meeting from an existing transcript file (.txt/.md/.srt/.vtt)
// so it can be summarized without any audio or transcription step.

use crate::api::TranscriptSegment;
use crate::state::AppState;
use anyhow::{anyhow, Result};
use log::{info, warn};
use std::path::Path;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use super::audio_processing::create_meeting_folder;
use super::common::write_transcripts_json;
use super::import::{create_meeting_with_transcripts, write_import_metadata, ImportResult};
use super::recording_preferences::get_default_recordings_folder;

const TRANSCRIPT_EXTENSIONS: &[&str] = &["txt", "md", "srt", "vtt"];

/// Parse transcript text into (text, start_seconds, end_seconds) entries.
/// Subtitle formats keep their cue times; plain text becomes one entry per non-empty line.
fn parse_transcript(content: &str, extension: &str) -> Vec<(String, Option<f64>, Option<f64>)> {
    let content = content.trim_start_matches('\u{feff}');
    if !matches!(extension, "srt" | "vtt") {
        return content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| (l.to_string(), None, None))
            .collect();
    }

    // Cues are separated by blank lines; some exporters write whitespace on those lines
    let mut blocks: Vec<Vec<&str>> = vec![Vec::new()];
    for line in content.lines().map(str::trim) {
        if line.is_empty() {
            blocks.push(Vec::new());
        } else if let Some(block) = blocks.last_mut() {
            block.push(line);
        }
    }
    blocks
        .into_iter()
        .filter_map(|block| {
            let mut lines = block.into_iter();
            // Skip the cue number / cue id / header lines until the timing line
            let timing = lines.by_ref().find(|l| l.contains("-->"))?;
            let mut times = timing
                .split("-->")
                .map(|t| parse_timestamp(t.split_whitespace().next().unwrap_or("")));
            let start = times.next().flatten();
            let end = times.next().flatten();
            let text = lines.map(strip_cue_tags).collect::<Vec<_>>().join(" ");
            let text = text.trim().to_string();
            (!text.is_empty()).then_some((text, start, end))
        })
        .collect()
}

/// "HH:MM:SS,mmm", "HH:MM:SS.mmm" or "MM:SS.mmm" -> seconds
fn parse_timestamp(ts: &str) -> Option<f64> {
    let parts: Vec<f64> = ts
        .trim()
        .replace(',', ".")
        .split(':')
        .map(|p| p.parse::<f64>().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [h, m, s] => Some(h * 3600.0 + m * 60.0 + s),
        [m, s] => Some(m * 60.0 + s),
        _ => None,
    }
}

/// Turn VTT voice tags into "Speaker: " prefixes and drop all other markup tags.
fn strip_cue_tags(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let tag = &rest[open + 1..open + close];
        if let Some(speaker) = tag.strip_prefix("v ") {
            // "<v.loud Name>" / "<v Name>" -> "Name: "
            out.push_str(speaker.trim());
            out.push_str(": ");
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

fn to_segments(entries: Vec<(String, Option<f64>, Option<f64>)>) -> Vec<TranscriptSegment> {
    let now = chrono::Utc::now().to_rfc3339();
    entries
        .into_iter()
        .map(|(text, start, end)| TranscriptSegment {
            id: format!("transcript-{}", Uuid::new_v4()),
            text,
            timestamp: now.clone(),
            audio_start_time: start,
            audio_end_time: end,
            duration: start.zip(end).map(|(s, e)| e - s),
        })
        .collect()
}

async fn import_transcript_file<R: Runtime>(
    app: &AppHandle<R>,
    path: &Path,
) -> Result<ImportResult> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    if !TRANSCRIPT_EXTENSIONS.contains(&extension.as_str()) {
        return Err(anyhow!("Unsupported transcript format: .{}", extension));
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Could not read {} as UTF-8 text: {}", path.display(), e))?;
    let segments = to_segments(parse_transcript(&content, &extension));
    if segments.is_empty() {
        return Err(anyhow!("No transcript text found in {}", path.display()));
    }

    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported transcript")
        .to_string();
    let duration_seconds = segments
        .iter()
        .filter_map(|s| s.audio_end_time)
        .fold(0.0, f64::max);

    let meeting_folder = create_meeting_folder(&get_default_recordings_folder(), &title, false)?;
    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;
    let meeting_id = create_meeting_with_transcripts(
        app_state.db_manager.pool(),
        &title,
        &segments,
        meeting_folder.to_string_lossy().to_string(),
    )
    .await?;

    if let Err(e) = write_transcripts_json(&meeting_folder, &segments) {
        warn!("Failed to write transcripts.json: {}", e);
    }
    if let Err(e) = write_import_metadata(
        &meeting_folder,
        &meeting_id,
        &title,
        duration_seconds,
        "",
        "transcript-import",
    ) {
        warn!("Failed to write metadata.json: {}", e);
    }

    info!(
        "Imported transcript as {} ({} segments)",
        meeting_id,
        segments.len()
    );
    Ok(ImportResult {
        meeting_id,
        title,
        segments_count: segments.len(),
        duration_seconds,
    })
}

/// Pick a transcript file and import it as a new meeting. Returns None if the user cancels the dialog.
#[tauri::command]
pub async fn import_transcript_command<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<ImportResult>, String> {
    let app_clone = app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        app_clone
            .dialog()
            .file()
            .add_filter("Transcripts", TRANSCRIPT_EXTENSIONS)
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("File dialog task failed: {}", e))?;

    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = std::path::PathBuf::from(picked.to_string());
    import_transcript_file(&app, &path)
        .await
        .map(Some)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt_with_times() {
        // Second separator line holds whitespace, as some exporters write it
        let srt = "1\r\n00:00:01,500 --> 00:00:04,000\r\nHello there\r\nsecond line\r\n  \r\n2\r\n00:01:00,000 --> 00:01:02,250\r\nBye\r\n";
        let out = parse_transcript(srt, "srt");
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0],
            ("Hello there second line".to_string(), Some(1.5), Some(4.0))
        );
        assert_eq!(out[1], ("Bye".to_string(), Some(60.0), Some(62.25)));
    }

    #[test]
    fn parses_vtt_with_speakers_and_short_times() {
        let vtt = "\u{feff}WEBVTT\n\nNOTE exported from Teams\n\ncue-1\n00:05.000 --> 00:07.500 align:start\n<v Saleh Ahmed>Let's <b>start</b></v>\n";
        let out = parse_transcript(vtt, "vtt");
        assert_eq!(
            out,
            vec![("Saleh Ahmed: Let's start".to_string(), Some(5.0), Some(7.5))]
        );
    }

    #[test]
    fn parses_plain_text_per_line() {
        let out = parse_transcript("Alice: hi\n\n  Bob: hello  \n", "txt");
        assert_eq!(
            out,
            vec![
                ("Alice: hi".to_string(), None, None),
                ("Bob: hello".to_string(), None, None)
            ]
        );
    }

    #[test]
    fn segments_get_duration_only_with_both_times() {
        let segs = to_segments(vec![
            ("a".into(), Some(1.0), Some(3.5)),
            ("b".into(), None, None),
        ]);
        assert_eq!(segs[0].duration, Some(2.5));
        assert_eq!(segs[1].duration, None);
    }

    #[tokio::test]
    async fn imported_transcript_is_readable_as_a_meeting() {
        use crate::database::repositories::meeting::MeetingsRepository;

        // One connection: each in-memory SQLite connection is a separate database
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let srt = "1\n00:00:02,000 --> 00:00:07,500\nAlex: Hello\n\n2\n00:00:08,000 --> 00:00:15,000\nPriya: Hi\n";
        let segments = to_segments(parse_transcript(srt, "srt"));
        let meeting_id =
            create_meeting_with_transcripts(&pool, "Product sync", &segments, String::new())
                .await
                .unwrap();

        let meeting = MeetingsRepository::get_meeting(&pool, &meeting_id)
            .await
            .unwrap()
            .expect("imported meeting should exist");
        assert_eq!(meeting.title, "Product sync");
        let mut rows: Vec<_> = meeting
            .transcripts
            .iter()
            .map(|t| (t.text.clone(), t.audio_start_time))
            .collect();
        rows.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        assert_eq!(
            rows,
            vec![
                ("Alex: Hello".to_string(), Some(2.0)),
                ("Priya: Hi".to_string(), Some(8.0))
            ]
        );
    }
}
