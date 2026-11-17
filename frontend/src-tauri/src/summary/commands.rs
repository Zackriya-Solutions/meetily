use crate::audio::teams_recap::{link_kind, LinkKind};
use crate::audio::teams_recap_live_match::{match_live_meeting, thread_id_from_url, LiveMatch, RecapFacts};
use crate::database::models::MeetingModel;
use crate::database::repositories::{
    meeting::MeetingsRepository,
    meeting_calendar::MeetingCalendarRepository,
    meeting_sources::{MeetingSource, LIVE_RECORDING_SOURCE},
    summary::SummaryProcessesRepository, transcript_chunk::TranscriptChunksRepository,
};
use crate::state::AppState;
use crate::summary::metadata::{
    read_detected_summary_language_from_metadata, read_summary_language_from_metadata,
    write_detected_summary_language_to_metadata, write_summary_language_to_metadata,
};
use crate::summary::language_detection::{
    detect_summary_language, SummaryLanguageDetection,
};
use crate::summary::processor::{clean_llm_markdown_detailed, contains_reasoning_marker};
use crate::summary::service::{SummaryService, PRESERVED_RESULT_FIELDS};
use log::{error as log_error, info as log_info, warn as log_warn};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Runtime};


static SUMMARY_START_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));
static LAST_SUMMARY_START: LazyLock<Mutex<Option<DateTime<Utc>>>> =
    LazyLock::new(|| Mutex::new(None));
fn next_summary_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let mut last = LAST_SUMMARY_START
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let next = match *last {
        Some(previous) if now <= previous => previous + Duration::nanoseconds(1),
        _ => now,
    };
    *last = Some(next);
    next
}
#[derive(Debug, Serialize, Deserialize)]
pub struct SummaryResponse {
    pub status: String,
    #[serde(rename = "meetingName")]
    pub meeting_name: Option<String>,
    pub meeting_id: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessTranscriptResponse {
    pub message: String,
    pub process_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedRecapResponse {
    pub meeting_id: String,
    pub title: String,
    /// When the meeting took place (RFC 3339): the recording's stamp when
    /// known, otherwise the import time.
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedRecapFromLinkResponse {
    pub meeting_id: String,
    pub title: String,
    pub obsidian_file_path: String,
    /// The recap joined a meeting Meetily already had (from its recording,
    /// its live recording, or an earlier import of the same link) instead of
    /// creating one.
    pub attached: bool,
    /// That meeting already had a summary, which the recap replaced.
    pub replaced_summary: bool,
    /// The recap stayed its own meeting, but live recordings overlap it in
    /// time without being confirmed as the same meeting.
    pub possible_duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SummaryLanguageStorage {
    Metadata,
    LocalFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummaryLanguagePreference {
    pub language: Option<String>,
    pub storage: SummaryLanguageStorage,
}

impl MeetingSummaryLanguagePreference {
    fn metadata(language: Option<String>) -> Self {
        Self {
            language,
            storage: SummaryLanguageStorage::Metadata,
        }
    }

    fn local_fallback() -> Self {
        Self {
            language: None,
            storage: SummaryLanguageStorage::LocalFallback,
        }
    }
}

enum MeetingFolderResolution {
    Folder(PathBuf),
    NoFolder,
}

/// Validate recap text and shape the summary record imported recaps are
/// stored as. Shared by creating a meeting and attaching to an existing one.
fn imported_recap_summary(
    recap: &str,
    source_url: Option<&str>,
    recorded_at: Option<&str>,
) -> Result<serde_json::Value, String> {
    let recap = recap.trim();
    if recap.is_empty() {
        return Err("Copilot recap is required.".into());
    }
    if recap.chars().count() > 500_000 {
        return Err("Copilot recap is too large (maximum 500,000 characters).".into());
    }
    if contains_reasoning_marker(recap) {
        return Err("The recap contains a model reasoning marker and was not imported.".into());
    }
    let summary = serde_json::json!({
        "markdown": recap,
        "imported_from": "microsoft_copilot_recap",
        "source_url": source_url,
        "recorded_at": recorded_at,
    });
    if !summary_is_renderable(&summary) {
        return Err("Copilot recap contains no visible content and was not imported.".into());
    }
    Ok(summary)
}

/// `import_source` of a meeting created from a Teams recap alone.
const COPILOT_RECAP_SOURCE: &str = "copilot-recap";

/// Times this close to a recap's listed start are the same start read another
/// way: the recording's, the transcript's or the calendar slot. A meeting
/// dated further off was dated by something else, such as the import's clock.
const SAME_START_MINUTES: i64 = 15;

pub(crate) fn listed_instant(listed_start: Option<&str>) -> Option<DateTime<Utc>> {
    listed_start
        .and_then(|value| DateTime::parse_from_rfc3339(value.trim()).ok())
        .map(|value| value.with_timezone(&Utc))
}

/// When a recap's meeting took place: the start its Recap app row gave, when
/// the recap came from the list, else its title's or recording's stamp, else
/// now.
fn recap_meeting_date(title: &str, recorded_at: Option<&str>, listed_start: Option<&str>) -> DateTime<Utc> {
    listed_instant(listed_start)
        .unwrap_or_else(|| crate::audio::import::resolve_meeting_date(title, None, recorded_at))
}

/// The date a meeting made from a recap alone should move to, when it is not
/// dated by its Recap app row. Before the sync dated recaps by the row, one
/// with no recording in its link was dated when it was imported.
fn recap_date_correction(
    created_at: DateTime<Utc>,
    source: Option<&MeetingSource>,
    listed_start: Option<&str>,
) -> Option<DateTime<Utc>> {
    let listed = listed_instant(listed_start)?;
    let recap_only = source.is_some_and(|source| source.import_source == COPILOT_RECAP_SOURCE);
    (recap_only && (created_at - listed).num_minutes().abs() > SAME_START_MINUTES).then_some(listed)
}

/// Whether the sync should write a recap it already holds again, because its
/// meeting is not dated by the recap's Recap app row.
pub(crate) async fn recap_meeting_needs_redate(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    listed_start: Option<&str>,
) -> Result<bool, String> {
    if listed_instant(listed_start).is_none() {
        return Ok(false);
    }
    let Some(meeting) = load_meeting(pool, meeting_id).await? else {
        return Ok(false);
    };
    let source = MeetingSource::get(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to read the source of meeting {meeting_id}: {e}"))?;
    Ok(recap_date_correction(meeting.created_at.0, source.as_ref(), listed_start).is_some())
}

/// Persist a recap already extracted from the authenticated Teams webview.
/// This intentionally bypasses recording download, speech-to-text, and LLM
/// summary generation.
async fn save_extracted_copilot_recap(
    pool: &sqlx::SqlitePool,
    title: String,
    recap: String,
    source_url: Option<String>,
    recorded_at: Option<String>,
    created_at: DateTime<Utc>,
) -> Result<ImportedRecapResponse, String> {
    let title = title.trim();
    let recap = recap.trim();
    if title.is_empty() {
        return Err("Meeting title is required.".into());
    }
    if title.chars().count() > 500 {
        return Err("Meeting title is too long (maximum 500 characters).".into());
    }

    let source_url = source_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(url) = source_url.as_deref() {
        if url.len() > 8_192
            || !url::Url::parse(url)
                .ok()
                .is_some_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
        {
            return Err("Source link must be a valid HTTP or HTTPS URL.".into());
        }
    }

    let summary = imported_recap_summary(recap, source_url.as_deref(), recorded_at.as_deref())?;

    let meeting_id = SummaryProcessesRepository::create_meeting_with_imported_summary(
        pool,
        title,
        &summary,
        source_url.as_deref(),
        created_at,
    )
    .await
    .map_err(|e| format!("Failed to import Copilot recap: {e}"))?;

    let mut meeting_source = MeetingSource::for_import(
        COPILOT_RECAP_SOURCE,
        title,
        source_url.as_deref(),
        None,
        None,
    );
    if recorded_at.is_some() {
        meeting_source.recorded_at = recorded_at.clone();
    }
    if let Err(e) = meeting_source.insert(pool, &meeting_id).await {
        log::warn!("Failed to record source for imported recap {}: {}", meeting_id, e);
    }

    log_info!(
        "Imported Microsoft Copilot recap as transcript-free meeting {}",
        meeting_id
    );
    Ok(ImportedRecapResponse {
        meeting_id,
        title: title.to_string(),
        created_at: created_at.to_rfc3339(),
    })
}

/// Open a Teams recap in the existing SharePoint-authenticated WebView2
/// profile, read its AI summary, and write the imported meeting to Obsidian.
#[tauri::command]
pub async fn api_import_copilot_recap_from_link<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    title: String,
    source_url: String,
) -> Result<ImportedRecapFromLinkResponse, String> {
    let validated_url = crate::audio::teams_recap::validate_recap_url(source_url.trim())
        .map_err(|e| e.to_string())?;
    let recording = crate::audio::teams_recap::recording_metadata(&validated_url);
    let title = crate::audio::teams_recap::metadata::from_filename(&title)
        .map(|metadata| metadata.title)
        .unwrap_or(title);
    let source_url = validated_url.as_str();

    require_obsidian_vault(&app).await?;

    let recap = crate::audio::teams_recap::extract_teams_recap(&app, source_url)
        .await
        .map_err(|e| e.to_string())?;
    let recap_markdown = recap_markdown_with_source(&recap.markdown, source_url);
    persist_recap_from_link(
        &app,
        state.db_manager.pool(),
        title,
        source_url,
        recording.as_ref().map(|metadata| metadata.recorded_at.as_str()),
        None,
        &recap_markdown,
    )
    .await
}

/// Refuse recap-to-Obsidian work when the vault is not usable here: an
/// explicit "import to Obsidian" must not silently succeed only in Meetily.
pub(crate) async fn require_obsidian_vault<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    let settings = crate::obsidian::load_obsidian_settings(app)
        .await
        .map_err(|e| e.to_string())?;
    let vault_path = settings
        .vault_path
        .ok_or_else(|| "Set an Obsidian vault in Meetily Settings before importing a Teams recap link.".to_string())?;
    if !std::path::Path::new(&vault_path).is_dir() {
        return Err("The configured Obsidian vault folder is not available on this machine.".into());
    }
    Ok(vault_path)
}

/// The note body an extracted recap is stored and exported as.
pub(crate) fn recap_markdown_with_source(markdown: &str, source_url: &str) -> String {
    format!("{}\n[Open original Teams recap](<{}>)", markdown.trim(), source_url)
}

/// Which meeting an extracted recap belongs to.
#[derive(Debug)]
enum RecapTarget {
    /// The meeting holding this recording, or an earlier copy of this recap.
    Known(MeetingModel),
    /// A meeting Meetily recorded live, matched by its calendar window.
    Live(MeetingModel),
    /// No meeting yet. `overlapping` names live recordings that fit in time
    /// but could not be confirmed as this meeting.
    New { overlapping: Vec<String> },
}

async fn load_meeting(pool: &sqlx::SqlitePool, meeting_id: &str) -> Result<Option<MeetingModel>, String> {
    MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting {meeting_id}: {e}"))
}

/// Whether the meeting's calendar event names a Teams thread other than the
/// recap's. A meeting holding this recap's file can only be that wrong through
/// a recording matched on its title, which has no thread id to compare.
async fn thread_contradicts(pool: &sqlx::SqlitePool, meeting_id: &str, facts: &RecapFacts) -> Result<bool, String> {
    let Some(ours) = facts.thread_id.as_deref() else {
        return Ok(false);
    };
    let calendar = MeetingCalendarRepository::get(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to read the calendar event of meeting {meeting_id}: {e}"))?;
    Ok(calendar
        .and_then(|calendar| calendar.join_url)
        .as_deref()
        .and_then(thread_id_from_url)
        .is_some_and(|theirs| theirs != ours))
}

/// Identity first, because it is exact; then the live recording rule (see
/// docs/TEAMS_RECAP_SYNC_SPEC.md, "Matching a recap to a live recording").
async fn find_recap_target(
    pool: &sqlx::SqlitePool,
    source_url: &str,
    facts: &RecapFacts,
) -> Result<RecapTarget, String> {
    let identity = crate::audio::teams_recap::meeting_identity_key(source_url);
    while let Some(meeting_id) = MeetingSource::find_meeting_by_identity(pool, &identity)
        .await
        .map_err(|e| format!("Failed to check for an existing meeting: {e}"))?
    {
        if !thread_contradicts(pool, &meeting_id, facts).await? {
            match load_meeting(pool, &meeting_id).await? {
                Some(meeting) => return Ok(RecapTarget::Known(meeting)),
                None => break,
            }
        }
        // The removal also ends this loop: the same row cannot be found twice.
        log_warn!("Live meeting {meeting_id} gives up a recording attached by its title: its recap names another thread");
        MeetingSource::remove(pool, &meeting_id)
            .await
            .map_err(|e| format!("Failed to release the recording from meeting {meeting_id}: {e}"))?;
    }
    let windows = match link_kind(source_url) {
        LinkKind::RecapWithoutFile => MeetingCalendarRepository::live_windows_for_recap_without_file(pool).await,
        LinkKind::File | LinkKind::RecapOfFile => MeetingCalendarRepository::unclaimed_live_windows(pool).await,
    }
    .map_err(|e| format!("Failed to check for a live recording of this meeting: {e}"))?;
    match match_live_meeting(facts, &windows) {
        LiveMatch::None => Ok(RecapTarget::New { overlapping: Vec::new() }),
        LiveMatch::Ambiguous(overlapping) => Ok(RecapTarget::New { overlapping }),
        LiveMatch::Attach(meeting_id) => Ok(match load_meeting(pool, &meeting_id).await? {
            Some(meeting) => RecapTarget::Live(meeting),
            None => RecapTarget::New { overlapping: Vec::new() },
        }),
    }
}

/// Put the recap's key on the live recording it is about to attach to, so the
/// next import of this recap finds the meeting by identity and no other recap
/// can claim it. Done before the summary is replaced: without the key, a second
/// recording's recap could overwrite this one.
async fn claim_live_meeting(
    pool: &sqlx::SqlitePool,
    meeting: &MeetingModel,
    source_url: &str,
    recorded_at: Option<&str>,
) -> Result<(), String> {
    let mut source = MeetingSource::for_import(LIVE_RECORDING_SOURCE, &meeting.title, Some(source_url), None, None);
    if recorded_at.is_some() {
        source.recorded_at = recorded_at.map(str::to_owned);
    }
    source
        .insert(pool, &meeting.id)
        .await
        .map_err(|e| format!("Failed to link the recap to live meeting {}: {e}", meeting.id))
}

/// Store an extracted recap and write its vault note. The recap joins the
/// meeting Meetily already holds for the same recording, an earlier import of
/// the same link, or a live recording of the same meeting; otherwise it
/// becomes a new transcript-free meeting. `listed_start` is the start time the
/// Recap app row gave, when the recap came from the list; a meeting made from
/// the recap alone is dated by it. Shared by the link import and the recap
/// sync.
pub(crate) async fn persist_recap_from_link<R: Runtime>(
    app: &AppHandle<R>,
    pool: &sqlx::SqlitePool,
    title: String,
    source_url: &str,
    recorded_at: Option<&str>,
    listed_start: Option<&str>,
    recap_markdown: &str,
) -> Result<ImportedRecapFromLinkResponse, String> {
    let facts = RecapFacts::from_link(source_url, &title, recorded_at, listed_start);
    let target = find_recap_target(pool, source_url, &facts).await?;

    let (meeting, live) = match target {
        RecapTarget::Known(meeting) => (meeting, false),
        RecapTarget::Live(meeting) => (meeting, true),
        RecapTarget::New { overlapping } => {
            let created_at = recap_meeting_date(&title, recorded_at, listed_start);
            let imported = save_extracted_copilot_recap(
                pool,
                title,
                recap_markdown.to_string(),
                Some(source_url.to_string()),
                recorded_at.map(str::to_owned),
                created_at,
            )
            .await?;
            if !overlapping.is_empty() {
                log_warn!(
                    "Recap meeting {} may duplicate live meetings {:?}: they fit its time, but nothing confirmed one",
                    imported.meeting_id, overlapping
                );
            }
            crate::api::emit_meetings_changed(app, &imported.meeting_id);
            let exported = crate::obsidian::export_copilot_recap_note(
                app,
                &imported.meeting_id,
                &imported.title,
                &imported.created_at,
                recap_markdown,
                recorded_at,
            )
            .await
            .map_err(|e| format!(
                "The recap was saved in Meetily as meeting {}, but the Obsidian export failed: {e}",
                imported.meeting_id
            ))?;
            return Ok(ImportedRecapFromLinkResponse {
                meeting_id: imported.meeting_id,
                title: imported.title,
                obsidian_file_path: exported.file_path,
                attached: false,
                replaced_summary: false,
                possible_duplicate: !overlapping.is_empty(),
            });
        }
    };

    let summary = imported_recap_summary(recap_markdown, Some(source_url), recorded_at)?;
    let replaced_summary = SummaryProcessesRepository::get_summary_data(pool, &meeting.id)
        .await
        .map_err(|e| format!("Failed to read the summary of meeting {}: {e}", meeting.id))?
        .is_some_and(|row| row.result.is_some());
    if live {
        claim_live_meeting(pool, &meeting, source_url, recorded_at).await?;
    }
    let attached = SummaryProcessesRepository::attach_imported_summary(pool, &meeting.id, &summary)
        .await
        .map_err(|e| format!("Failed to attach the recap to meeting {}: {e}", meeting.id))?;
    if !attached {
        return Err(format!("Meeting {} no longer exists.", meeting.id));
    }
    log_info!(
        "Attached Microsoft Copilot recap to existing meeting {} ({})",
        meeting.id, meeting.title
    );
    crate::api::emit_meetings_changed(app, &meeting.id);

    // A meeting made from this recap alone moves to the recap's listed start
    // when it was dated otherwise, and its note to the name that date gives.
    let redated = if live {
        None
    } else {
        let source = MeetingSource::get(pool, &meeting.id)
            .await
            .map_err(|e| format!("Failed to read the source of meeting {}: {e}", meeting.id))?;
        recap_date_correction(meeting.created_at.0, source.as_ref(), listed_start)
    };
    if let Some(date) = redated {
        MeetingsRepository::update_meeting_date(pool, &meeting.id, date)
            .await
            .map_err(|e| format!("Failed to correct the date of meeting {}: {e}", meeting.id))?;
        log_info!(
            "Recap meeting {} moved from {} to its listed start {}",
            meeting.id,
            meeting.created_at.0.to_rfc3339(),
            date.to_rfc3339()
        );
    }

    // A meeting that has a transcript keeps it in its note; a recap-only
    // meeting keeps the readable date-and-title filename.
    let (segments, _) = MeetingsRepository::get_meeting_transcripts_paginated(pool, &meeting.id, i64::MAX, 0)
        .await
        .map_err(|e| format!("Failed to load the transcript of meeting {}: {e}", meeting.id))?;
    let created_at = redated.unwrap_or(meeting.created_at.0).to_rfc3339();
    let exported = if segments.is_empty() && redated.is_some() {
        crate::obsidian::move_copilot_recap_note(
            app, &meeting.id, &meeting.title, &created_at, recap_markdown, recorded_at,
        )
        .await
    } else if segments.is_empty() {
        crate::obsidian::export_copilot_recap_note(
            app, &meeting.id, &meeting.title, &created_at, recap_markdown, recorded_at,
        )
        .await
    } else {
        let transcript_markdown = crate::obsidian::format_transcript_markdown(&segments);
        crate::obsidian::export_meeting_note(
            app, &meeting.id, &meeting.title, &created_at, recap_markdown, &transcript_markdown,
        )
        .await
    }
    .map_err(|e| format!(
        "The recap was attached to meeting {} in Meetily, but the Obsidian export failed: {e}",
        meeting.id
    ))?;
    Ok(ImportedRecapFromLinkResponse {
        meeting_id: meeting.id,
        title: meeting.title,
        obsidian_file_path: exported.file_path,
        attached: true,
        replaced_summary,
        possible_duplicate: false,
    })
}

/// Saves a meeting summary (Native SQLx implementation)
///
/// Expected format: { "markdown": "...", "summary_json": [...BlockNote blocks...] }
#[tauri::command]
pub async fn api_save_meeting_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary: serde_json::Value,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_meeting_summary (native) called for meeting_id: {}",
        meeting_id
    );
    if !summary_is_renderable(&summary) {
        return Err("Summary contains no visible content or model reasoning markers and was not saved.".into());
    }
    let pool = state.db_manager.pool();

    // The editor only ever sends {markdown, summary_json}, but it overwrites the
    // whole `result` column. Carry the generation-owned fields across so saving
    // an edit does not discard the provenance stamp or the English cache.
    let summary = carry_forward_preserved_result_fields(pool, &meeting_id, summary).await;

    match SummaryProcessesRepository::update_meeting_summary(pool, &meeting_id, &summary).await {
        Ok(true) => {
            log_info!("Summary saved successfully for meeting_id: {}", meeting_id);
            Ok(serde_json::json!({
                "message": "Meeting summary saved successfully"
            }))
        }
        Ok(false) => {
            log_warn!(
                "Meeting not found or invalid JSON for meeting_id: {}",
                meeting_id
            );
            Err("Meeting not found or can't convert the json".into())
        }
        Err(e) => {
            log_error!("Failed to save meeting summary for {}: {}", meeting_id, e);
            Err(e.to_string())
        }
    }
}

/// Copies the generation-owned fields of the stored `result` blob into an
/// incoming editor payload, for any field the payload does not already set.
///
/// Best-effort: if the existing row is missing or unparseable there is nothing
/// to preserve, and the save proceeds with the payload as given.
async fn carry_forward_preserved_result_fields(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    summary: serde_json::Value,
) -> serde_json::Value {
    let existing_raw = match SummaryProcessesRepository::get_summary_data(pool, meeting_id).await {
        Ok(Some(process)) => process.result,
        Ok(None) => None,
        Err(e) => {
            log_warn!(
                "Failed to load existing summary result for {} ({}); saving without preserved fields",
                meeting_id,
                e
            );
            None
        }
    };

    merge_preserved_result_fields(summary, existing_raw.as_deref())
}

/// Pure half of [`carry_forward_preserved_result_fields`]: merges the preserved
/// fields of a previously stored `result` blob into an incoming editor payload.
///
/// The payload wins wherever it sets a field itself, so a later generation pass
/// can still replace the stamp.
fn merge_preserved_result_fields(
    mut summary: serde_json::Value,
    existing_raw: Option<&str>,
) -> serde_json::Value {
    let Some(existing_raw) = existing_raw else {
        return summary;
    };
    let Some(incoming) = summary.as_object_mut() else {
        return summary;
    };

    let existing: serde_json::Value = match serde_json::from_str(existing_raw) {
        Ok(value) => value,
        Err(e) => {
            log_warn!(
                "Existing summary result is not valid JSON ({}); saving without preserved fields",
                e
            );
            return summary;
        }
    };

    for field in PRESERVED_RESULT_FIELDS {
        if incoming.contains_key(*field) {
            continue;
        }
        if let Some(value) = existing.get(*field) {
            incoming.insert((*field).to_string(), value.clone());
        }
    }

    summary
}

/// Gets the per-meeting summary language override from metadata.json.
#[tauri::command]
pub async fn api_get_meeting_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_get_meeting_summary_language called for meeting_id: {}",
        meeting_id
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => read_summary_language_from_metadata(&folder)
            .map(MeetingSummaryLanguagePreference::metadata)
            .map_err(|e| e.to_string()),
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Saves or clears the per-meeting summary language override in metadata.json.
#[tauri::command]
pub async fn api_save_meeting_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary_language: Option<String>,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_save_meeting_summary_language called for meeting_id: {}, language: {:?}",
        meeting_id,
        summary_language
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => {
            write_summary_language_to_metadata(&folder, summary_language.as_deref())
                .map_err(|e| e.to_string())?;
            read_summary_language_from_metadata(&folder)
                .map(MeetingSummaryLanguagePreference::metadata)
                .map_err(|e| e.to_string())
        }
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Gets the cached Auto-detected summary language from metadata.json.
#[tauri::command]
pub async fn api_get_meeting_detected_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_get_meeting_detected_summary_language called for meeting_id: {}",
        meeting_id
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => read_detected_summary_language_from_metadata(&folder)
            .map(MeetingSummaryLanguagePreference::metadata)
            .map_err(|e| e.to_string()),
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Saves or clears the cached Auto-detected summary language in metadata.json.
#[tauri::command]
pub async fn api_save_meeting_detected_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    detected_summary_language: Option<String>,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_save_meeting_detected_summary_language called for meeting_id: {}, language: {:?}",
        meeting_id,
        detected_summary_language
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => {
            write_detected_summary_language_to_metadata(&folder, detected_summary_language.as_deref())
                .map_err(|e| e.to_string())?;
            read_detected_summary_language_from_metadata(&folder)
                .map(MeetingSummaryLanguagePreference::metadata)
                .map_err(|e| e.to_string())
        }
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Detects the dominant supported summary language from transcript segments.
#[tauri::command]
pub async fn api_detect_transcript_summary_language(
    transcript_texts: Vec<String>,
) -> Result<SummaryLanguageDetection, String> {
    Ok(detect_summary_language(&transcript_texts))
}

async fn resolve_meeting_folder(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<MeetingFolderResolution, String> {
    let meeting = MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting metadata: {}", e))?
        .ok_or_else(|| format!("Meeting not found: {}", meeting_id))?;

    let Some(folder_path) = meeting.folder_path.filter(|p| !p.trim().is_empty()) else {
        return Ok(MeetingFolderResolution::NoFolder);
    };

    Ok(MeetingFolderResolution::Folder(PathBuf::from(folder_path)))
}

fn blocknote_has_visible_text(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => items.iter().any(blocknote_has_visible_text),
        serde_json::Value::Object(object) => {
            object
                .get("text")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
                || object
                    .get("content")
                    .is_some_and(blocknote_has_visible_text)
                || object
                    .get("children")
                    .is_some_and(blocknote_has_visible_text)
        }
        _ => false,
    }
}

fn legacy_section_has_visible_text(value: &serde_json::Value) -> bool {
    value
        .get("blocks")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|blocks| {
            blocks.iter().any(|block| {
                block
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|content| !content.trim().is_empty())
            })
        })
}

fn summary_contains_reasoning_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => items.iter().any(summary_contains_reasoning_marker),
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| match key.as_str() {
            "markdown" | "text" | "content" => value
                .as_str()
                .is_some_and(contains_reasoning_marker)
                || value.is_array() && summary_contains_reasoning_marker(value),
            "summary_json" | "children" => summary_contains_reasoning_marker(value),
            _ => false,
        }),
        _ => false,
    }
}

fn summary_is_renderable(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if summary_contains_reasoning_marker(value) {
        return false;
    }
    if let Some(markdown) = object.get("markdown") {
        return markdown
            .as_str()
            .is_some_and(|markdown| !clean_llm_markdown_detailed(markdown).markdown.is_empty());
    }
    if let Some(blocks) = object.get("summary_json") {
        return blocknote_has_visible_text(blocks);
    }
    object
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "MeetingName" | "_section_order"))
        .any(|(_, value)| legacy_section_has_visible_text(value))
}

fn sanitize_markdown_value(value: Option<&mut serde_json::Value>) -> bool {
    let Some(serde_json::Value::String(markdown)) = value else {
        return false;
    };
    let cleaned = clean_llm_markdown_detailed(markdown);
    let reasoning_stripped = cleaned.reasoning_stripped;
    *markdown = cleaned.markdown;
    reasoning_stripped
}

fn redact_flagged_reasoning(mut result: serde_json::Value) -> serde_json::Value {
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    let mut reasoning_stripped = object
        .get("reasoning_stripped")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    if reasoning_stripped {
        object.remove("reasoning");
    }
    reasoning_stripped |= sanitize_markdown_value(object.get_mut("markdown"));
    if let Some(cache) = object
        .get_mut("english_cache")
        .and_then(serde_json::Value::as_object_mut)
    {
        reasoning_stripped |= sanitize_markdown_value(cache.get_mut("markdown"));
    }
    if reasoning_stripped {
        object.insert("reasoning_stripped".to_string(), serde_json::Value::Bool(true));
    }
    result
}

/// Gets summary status and data (Native SQLx implementation)
///
/// Returns summary status (pending/processing/completed/failed) and parsed result data
#[tauri::command]
pub async fn api_get_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    _auth_token: Option<String>,
) -> Result<SummaryResponse, String> {
    log_info!(
        "api_get_summary (native) called for meeting_id: {}",
        meeting_id
    );
    let pool = state.db_manager.pool();

    match SummaryProcessesRepository::get_summary_data_for_meeting(pool, &meeting_id).await {
        Ok(Some(process)) => {
            let mut status = process.status.to_lowercase();
            let mut error = process.error;

            // Parse result data if it exists (regardless of status) so restored
            // summaries remain available after cancellation or failure.
            let mut data = if let Some(result_str) = process.result {
                match serde_json::from_str::<serde_json::Value>(&result_str) {
                    Ok(parsed) => Some(redact_flagged_reasoning(parsed)),
                    Err(e) => {
                        log_error!("Failed to parse summary result JSON: {}", e);
                        None
                    }
                }
            } else {
                None
            };
            if status == "completed" && !data.as_ref().is_some_and(summary_is_renderable) {
                status = "error".to_string();
                error = Some(
                    "Stored summary data is missing or invalid. Generate the summary again."
                        .to_string(),
                );
                data = None;
            }

            // Fetch meeting title from database
            let meeting_name = match MeetingsRepository::get_meeting_metadata(pool, &meeting_id).await {
                Ok(Some(meeting)) => Some(meeting.title),
                Ok(None) => {
                    log_warn!("Meeting not found for meeting_id: {}", meeting_id);
                    None
                }
                Err(e) => {
                    log_error!("Failed to fetch meeting title: {}", e);
                    None
                }
            };

            let response = SummaryResponse {
                status: status.clone(),
                meeting_name,
                meeting_id: meeting_id.clone(),
                start: process
                    .start_time
                    .map(|time| time.to_rfc3339_opts(SecondsFormat::Nanos, true)),
                end: process.end_time.map(|time| time.to_rfc3339()),
                data,
                error,
            };

            log_info!(
                "Summary status for {}: {}, has_data: {}, meeting_name: {:?}",
                meeting_id,
                status,
                response.data.is_some(),
                response.meeting_name
            );
            Ok(response)
        }
        Ok(None) => {
            log_info!("No summary process found for meeting_id: {}", meeting_id);

            // Still fetch meeting title for idle state
            let meeting_name = match MeetingsRepository::get_meeting_metadata(pool, &meeting_id).await {
                Ok(Some(meeting)) => Some(meeting.title),
                _ => None,
            };

            Ok(SummaryResponse {
                status: "idle".to_string(),
                meeting_name,
                meeting_id,
                start: None,
                end: None,
                data: None,
                error: None,
            })
        }
        Err(e) => {
            log_error!("Error retrieving summary for {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve summary: {}", e))
        }
    }
}

/// Processes transcript and generates summary (Native SQLx implementation)
///
/// Spawns a background task and returns immediately with process_id
#[tauri::command]
pub async fn api_process_transcript<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    text: String,
    model: String,
    model_name: String,
    meeting_id: Option<String>,
    _chunk_size: Option<i32>,
    _overlap: Option<i32>,
    custom_prompt: Option<String>,
    template_id: Option<String>,
    summary_language: Option<String>,
    _auth_token: Option<String>,
) -> Result<ProcessTranscriptResponse, String> {
    use uuid::Uuid;

    let m_id = meeting_id.unwrap_or_else(|| format!("meeting-{}", Uuid::new_v4()));
    log_info!(
        "api_process_transcript (native) called for meeting_id: {}, model: {}",
        &m_id,
        &model
    );

    let pool = state.db_manager.pool().clone();
    let final_prompt = custom_prompt.unwrap_or_else(|| "".to_string());
    let final_template_id = template_id.unwrap_or_else(|| "standard_meeting".to_string());

    // Normalise empty / whitespace-only to None so "" and null behave identically
    let summary_language = summary_language.and_then(|s| {
        let t = s.trim();
        if t.is_empty() { None } else { Some(t.to_string()) }
    });

    // ponytail: summary starts are rare; use per-meeting locks only if start contention is measured.
    let _start_guard = SUMMARY_START_LOCK.lock().await;
    let started_at = next_summary_start(Utc::now());
    SummaryProcessesRepository::create_or_reset_process(&pool, &m_id, started_at)
        .await
        .map_err(|e| format!("Failed to initialize process: {}", e))?;
    // The reset row is pending, so the meeting no longer counts as summarized.
    crate::api::emit_meetings_changed(&app, &m_id);

    let chunk_size = _chunk_size.unwrap_or(40000);
    let overlap = _overlap.unwrap_or(1000);
    if let Err(error) = TranscriptChunksRepository::save_transcript_data(
        &pool,
        &m_id,
        &text,
        &model,
        &model_name,
        chunk_size,
        overlap,
    )
    .await
    {
        let message = format!("Failed to save transcript data: {}", error);
        let _ = SummaryProcessesRepository::update_process_failed(
            &pool,
            &m_id,
            started_at,
            &message,
        )
        .await;
        crate::api::emit_meetings_changed(&app, &m_id);
        return Err(message);
    }

    let cancellation_token =
        SummaryService::register_cancellation_token(&m_id, started_at);
    let meeting_id_clone = m_id.clone();
    let supervisor_pool = pool.clone();
    let supervisor_meeting_id = m_id.clone();
    let supervisor_app = app.clone();
    tauri::async_runtime::spawn(async move {
        // The supervisor turns a panicking attempt into a terminal 'failed'
        // row for this attempt only, and always releases its cancel token.
        let attempt = SummaryService::process_transcript_background(
            app,
            pool,
            meeting_id_clone,
            started_at,
            cancellation_token,
            text,
            model,
            model_name,
            final_prompt,
            final_template_id,
            summary_language,
        );
        SummaryService::supervise_summary_attempt(
            supervisor_pool,
            supervisor_meeting_id.clone(),
            started_at,
            attempt,
        )
        .await;
        // Completed, failed, cancelled or crashed: the process row is terminal.
        crate::api::emit_meetings_changed(&supervisor_app, &supervisor_meeting_id);
    });

    log_info!("🚀 Background task spawned for meeting_id: {}", &m_id);
    Ok(ProcessTranscriptResponse {
        message: "Summary generation started".to_string(),
        process_id: started_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
    })
}

/// Cancels an ongoing summary generation process
///
/// This command triggers the cancellation token for the specified meeting,
/// stopping the summary generation gracefully.
#[tauri::command]
pub async fn api_cancel_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    process_id: String,
) -> Result<serde_json::Value, String> {
    let started_at = DateTime::parse_from_rfc3339(&process_id)
        .map_err(|_| "Invalid summary process ID".to_string())?
        .with_timezone(&Utc);
    log_info!("api_cancel_summary called for meeting_id: {}", meeting_id);

    let cancellation_requested = SummaryService::cancel_summary(&meeting_id, started_at);
    let cancelled = if cancellation_requested {
        let pool = state.db_manager.pool();
        match SummaryProcessesRepository::update_process_cancelled(pool, &meeting_id, started_at)
            .await
        {
            Ok(true) => {
                log_info!("Successfully cancelled summary generation for meeting_id: {}", meeting_id);
                true
            }
            Ok(false) => {
                log_info!("Summary generation was already terminal for meeting_id: {}", meeting_id);
                false
            }
            Err(error) => {
                log_error!("Failed to update cancellation status for {}: {}", meeting_id, error);
                return Err(format!("Failed to update cancellation status: {}", error));
            }
        }
    } else {
        false
    };

    Ok(serde_json::json!({
        "cancelled": cancelled,
        "message": if cancelled {
            "Summary generation cancelled successfully"
        } else if cancellation_requested {
            "Summary generation was already terminal"
        } else {
            "No active summary generation to cancel"
        },
        "meeting_id": meeting_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_only_flagged_historical_reasoning() {
        assert_eq!(
            redact_flagged_reasoning(json!({
                "reasoning_stripped": true,

                "reasoning": "private",
                "markdown": "visible",
            })),
            json!({"reasoning_stripped": true, "markdown": "visible"})
        );
        assert_eq!(
            redact_flagged_reasoning(json!({"reasoning": "custom", "markdown": "visible"})),
            json!({"reasoning": "custom", "markdown": "visible"})
        );
    }

    #[test]
    fn summary_start_tokens_are_monotonic_with_identical_clock_inputs() {
        let first = next_summary_start(Utc::now());
        assert_eq!(
            next_summary_start(first),
            first + Duration::nanoseconds(1)
        );
    }

    #[test]
    fn summary_validation_rejects_blank_and_reasoning_content() {
        assert!(!summary_is_renderable(&json!({"markdown": "   "})));
        assert!(!summary_is_renderable(&json!({"markdown": "```\r\n```"})));
        assert!(!summary_is_renderable(&json!({
            "summary_json": [{"content": [{"type": "text", "text": "<think>private</think>"}]}]
        })));
        assert!(summary_is_renderable(&json!({
            "summary_json": [{"content": [{"type": "text", "text": "Visible"}]}]
        })));
    }

    #[test]
    fn response_redaction_strips_closed_markdown_envelopes() {
        let redacted = redact_flagged_reasoning(json!({
            "markdown": "Visible\n<think>private</think>\nTail"
        }));
        assert_eq!(redacted["markdown"], "Visible\n\nTail");
        assert_eq!(redacted["reasoning_stripped"], true);
    }
}

#[cfg(test)]
mod recap_target_tests {
    use super::*;
    use crate::calendar::types::{CalendarEvent, SourceKind};
    use chrono::TimeZone;
    use serde_json::json;

    const JOIN: &str = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_AAA%40thread.v2/0";
    const RECAP: &str = "https://teams.microsoft.com/l/meetingrecap?threadId=19%3Ameeting_AAA%40thread.v2&fileUrl=https%3A%2F%2Fc-my.sharepoint.com%2Fpersonal%2Fj%2FDocuments%2FRecordings%2FWeekly%20sync-20261103_100200-Meeting%20Recording.mp4";

    async fn test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite pool");
        sqlx::migrate!("./migrations").run(&pool).await.expect("migrations apply");
        pool
    }

    async fn insert_meeting(pool: &sqlx::SqlitePool, id: &str, title: &str) {
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) \
             VALUES (?, ?, '2026-11-03T16:30:00Z', '2026-11-03T16:30:00Z')",
        )
        .bind(id)
        .bind(title)
        .execute(pool)
        .await
        .unwrap();
    }

    /// A meeting recorded live against a 10:00 to 10:30 UTC calendar event.
    async fn live_meeting(pool: &sqlx::SqlitePool, id: &str, title: &str, join: Option<&str>) {
        insert_meeting(pool, id, title).await;
        let event = CalendarEvent {
            id: format!("event-{id}"),
            source: SourceKind::OutlookCom,
            title: title.to_owned(),
            start: Utc.with_ymd_and_hms(2026, 11, 3, 10, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 11, 3, 10, 30, 0).unwrap(),
            organizer: None,
            attendees: vec![],
            location: None,
            body: None,
            teams_join_url: join.map(str::to_owned),
            is_recurring_instance: false,
        };
        MeetingCalendarRepository::attach(pool, id, &event).await.unwrap();
    }

    /// What the recap at 10:02 UTC says about itself, independent of the
    /// machine's zone: the sync loop's listed start, not the file stamp.
    fn facts(title: &str) -> RecapFacts {
        RecapFacts::from_link(RECAP, title, None, Some("2026-11-03T10:02:00.000Z"))
    }

    #[tokio::test]
    async fn a_confirmed_live_recording_is_the_target_then_a_known_meeting() {
        let pool = test_pool().await;
        live_meeting(&pool, "live", "Weekly sync", Some(JOIN)).await;
        SummaryProcessesRepository::create_or_reset_process(&pool, "live", Utc::now()).await.unwrap();
        SummaryProcessesRepository::update_meeting_summary(&pool, "live", &json!({"markdown": "- from Meetily"}))
            .await
            .unwrap();

        let RecapTarget::Live(meeting) = find_recap_target(&pool, RECAP, &facts("Weekly sync")).await.unwrap() else {
            panic!("expected the live recording");
        };
        assert_eq!(meeting.id, "live");

        // What the attach path does to it.
        let recap = imported_recap_summary("- recap", Some(RECAP), Some("2026-11-03T04:02:00")).unwrap();
        claim_live_meeting(&pool, &meeting, RECAP, Some("2026-11-03T04:02:00")).await.unwrap();
        assert!(SummaryProcessesRepository::attach_imported_summary(&pool, "live", &recap).await.unwrap());

        let source = MeetingSource::get(&pool, "live").await.unwrap().unwrap();
        assert_eq!(source.import_source, LIVE_RECORDING_SOURCE);
        assert_eq!(source.recorded_at.as_deref(), Some("2026-11-03T04:02:00"));
        let stored = load_meeting(&pool, "live").await.unwrap().unwrap();
        assert_eq!(stored.title, "Weekly sync");
        let summary = SummaryProcessesRepository::get_summary_data(&pool, "live").await.unwrap().unwrap();
        assert!(summary.result.unwrap().contains("- recap"));
        assert!(summary.result_backup.unwrap().contains("from Meetily"));

        // The next import of this recap is an identity hit, whatever the clock
        // or the window says, and no other recap can claim this meeting.
        let no_clock = RecapFacts::from_link(RECAP, "Weekly sync", None, None);
        let RecapTarget::Known(again) = find_recap_target(&pool, RECAP, &no_clock).await.unwrap() else {
            panic!("expected an identity hit");
        };
        assert_eq!(again.id, "live");
        assert!(MeetingCalendarRepository::unclaimed_live_windows(&pool).await.unwrap().is_empty());
        let other = "https://teams.microsoft.com/l/meetingrecap?threadId=19%3Ameeting_AAA%40thread.v2&fileUrl=https%3A%2F%2Fc-my.sharepoint.com%2Fpart2.mp4";
        let part_two = RecapFacts::from_link(other, "Weekly sync", None, Some("2026-11-03T10:20:00.000Z"));
        assert!(matches!(
            find_recap_target(&pool, other, &part_two).await.unwrap(),
            RecapTarget::New { overlapping } if overlapping.is_empty()
        ));
    }

    #[tokio::test]
    async fn identity_wins_over_a_live_recording_that_also_fits() {
        let pool = test_pool().await;
        live_meeting(&pool, "live", "Weekly sync", Some(JOIN)).await;
        insert_meeting(&pool, "imported", "Weekly sync").await;
        MeetingSource::for_import("sharepoint", "Weekly sync", Some(RECAP), None, None)
            .insert(&pool, "imported")
            .await
            .unwrap();

        let RecapTarget::Known(meeting) = find_recap_target(&pool, RECAP, &facts("Weekly sync")).await.unwrap() else {
            panic!("expected the imported meeting");
        };
        assert_eq!(meeting.id, "imported");
    }

    #[tokio::test]
    async fn an_unconfirmed_overlap_stays_a_new_meeting_and_names_the_candidate() {
        let pool = test_pool().await;
        live_meeting(&pool, "live", "Budget review", None).await;

        let RecapTarget::New { overlapping } = find_recap_target(&pool, RECAP, &facts("Weekly sync")).await.unwrap() else {
            panic!("expected a new meeting");
        };
        assert_eq!(overlapping, vec!["live".to_string()]);
    }

    #[tokio::test]
    async fn a_meeting_with_no_calendar_match_and_a_recap_outside_every_window_are_new() {
        let pool = test_pool().await;
        insert_meeting(&pool, "no-calendar", "Weekly sync").await;
        live_meeting(&pool, "live", "Weekly sync", Some(JOIN)).await;
        let later = RecapFacts::from_link(RECAP, "Weekly sync", None, Some("2026-11-04T10:02:00.000Z"));

        let RecapTarget::New { overlapping } = find_recap_target(&pool, RECAP, &later).await.unwrap() else {
            panic!("expected a new meeting");
        };
        assert!(overlapping.is_empty());
    }

    /// Attached by the recording rule, which had only the title to go on.
    fn file_of_recap() -> &'static str {
        "https://c-my.sharepoint.com/personal/j/Documents/Recordings/Weekly%20sync-20261103_100200-Meeting%20Recording.mp4"
    }

    fn recap_of_file(thread: &str) -> String {
        format!(
            "https://teams.microsoft.com/l/meetingrecap?threadId={}&fileUrl={}",
            thread,
            url::form_urlencoded::byte_serialize(file_of_recap().as_bytes()).collect::<String>()
        )
    }

    fn recap_facts(link: &str) -> RecapFacts {
        RecapFacts::from_link(link, "Weekly sync", None, Some("2026-11-03T10:02:00.000Z"))
    }

    async fn attach_recording_to(pool: &sqlx::SqlitePool, id: &str) {
        crate::audio::sharepoint_live_match::attach_recording(pool, id, file_of_recap()).await.unwrap();
    }

    #[tokio::test]
    async fn a_recap_of_an_attached_recording_is_a_known_meeting() {
        let pool = test_pool().await;
        live_meeting(&pool, "live", "Weekly sync", Some(JOIN)).await;
        attach_recording_to(&pool, "live").await;

        let link = recap_of_file("19%3Ameeting_AAA%40thread.v2");
        let RecapTarget::Known(meeting) = find_recap_target(&pool, &link, &recap_facts(&link)).await.unwrap() else {
            panic!("expected an identity hit");
        };
        assert_eq!(meeting.id, "live");
        assert_eq!(
            MeetingSource::get(&pool, "live").await.unwrap().unwrap().recording_url.as_deref(),
            Some(file_of_recap()),
            "the row keeps the file: attaching to a known meeting does not claim it again"
        );
    }

    #[tokio::test]
    async fn a_recap_from_another_thread_takes_the_recording_back_from_a_wrong_match() {
        let pool = test_pool().await;
        live_meeting(&pool, "live", "Weekly sync", Some(JOIN)).await;
        attach_recording_to(&pool, "live").await;

        let link = recap_of_file("19%3Ameeting_OTHER%40thread.v2");
        let RecapTarget::New { overlapping } = find_recap_target(&pool, &link, &recap_facts(&link)).await.unwrap() else {
            panic!("the live meeting is another meeting");
        };
        assert!(overlapping.is_empty(), "a differing thread is certain, so nothing to report");
        assert_eq!(MeetingSource::get(&pool, "live").await.unwrap(), None);
        // The meeting is open to its own recap again.
        assert_eq!(MeetingCalendarRepository::unclaimed_live_windows(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_known_thread_on_only_one_side_does_not_take_the_recording_back() {
        let pool = test_pool().await;
        live_meeting(&pool, "meet-link", "Weekly sync", Some("https://teams.microsoft.com/meet/123?p=abc")).await;
        attach_recording_to(&pool, "meet-link").await;

        let link = recap_of_file("19%3Ameeting_OTHER%40thread.v2");
        let RecapTarget::Known(meeting) = find_recap_target(&pool, &link, &recap_facts(&link)).await.unwrap() else {
            panic!("nothing contradicts the match");
        };
        assert_eq!(meeting.id, "meet-link");
    }

    #[tokio::test]
    async fn a_recap_naming_no_file_can_attach_to_a_meeting_holding_a_recording() {
        let pool = test_pool().await;
        live_meeting(&pool, "live", "Weekly sync", Some(JOIN)).await;
        attach_recording_to(&pool, "live").await;

        let without_file = "https://teams.microsoft.com/l/meetingrecap?threadId=19%3Ameeting_AAA%40thread.v2";
        let RecapTarget::Live(meeting) = find_recap_target(&pool, without_file, &recap_facts(without_file)).await.unwrap()
        else {
            panic!("nothing ties this recap to another recording");
        };
        assert_eq!(meeting.id, "live");

        // A recap naming another file is the recap of another recording.
        let other_file = "https://teams.microsoft.com/l/meetingrecap?threadId=19%3Ameeting_AAA%40thread.v2&fileUrl=https%3A%2F%2Fc-my.sharepoint.com%2Fother-20261103_101500-Meeting%20Recording.mp4";
        assert!(matches!(
            find_recap_target(&pool, other_file, &recap_facts(other_file)).await.unwrap(),
            RecapTarget::New { overlapping } if overlapping.is_empty()
        ));
    }

    #[tokio::test]
    async fn a_generated_title_does_not_stop_a_recap_from_confirming_by_the_subject() {
        let pool = test_pool().await;
        live_meeting(&pool, "live", "Weekly sync", Some("https://teams.microsoft.com/meet/123?p=abc")).await;
        sqlx::query("UPDATE meetings SET title = 'Generated heading' WHERE id = 'live'")
            .execute(&pool)
            .await
            .unwrap();
        let RecapTarget::Live(meeting) = find_recap_target(&pool, RECAP, &facts("Weekly sync")).await.unwrap() else {
            panic!("expected the live recording");
        };
        assert_eq!(meeting.id, "live");
    }

    const LISTED: &str = "2026-11-03T10:02:00.000Z";

    fn listed() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 3, 10, 2, 0).unwrap()
    }

    fn source(import_source: &str) -> MeetingSource {
        MeetingSource::for_import(import_source, "Weekly sync", Some(RECAP), None, None)
    }

    #[test]
    fn a_listed_recap_is_dated_by_its_row_and_a_pasted_one_by_its_stamp() {
        let stamp = Some("2026-11-03T04:02:00");
        assert_eq!(recap_meeting_date("Weekly sync", stamp, Some(LISTED)), listed());
        assert_eq!(recap_meeting_date("Weekly sync", None, Some(LISTED)), listed());
        assert_eq!(
            recap_meeting_date("Weekly sync", stamp, None),
            crate::audio::import::resolve_meeting_date("Weekly sync", None, stamp)
        );
        // Neither: the import's own time, as before.
        let now = Utc::now();
        let dated = recap_meeting_date("Weekly sync", None, Some("not a time"));
        assert!((dated - now).num_seconds().abs() < 5, "{dated}");
    }

    #[test]
    fn only_a_meeting_made_from_a_recap_alone_and_dated_off_its_row_moves() {
        let imported_at = Utc.with_ymd_and_hms(2026, 11, 7, 6, 1, 14).unwrap();
        let recap = source(COPILOT_RECAP_SOURCE);
        assert_eq!(recap_date_correction(imported_at, Some(&recap), Some(LISTED)), Some(listed()));

        // The same start read another way stays put.
        let recording_start = listed() + Duration::minutes(SAME_START_MINUTES);
        assert_eq!(recap_date_correction(recording_start, Some(&recap), Some(LISTED)), None);
        // Meetily's own recordings and meetings with no source keep their date.
        let live = source(LIVE_RECORDING_SOURCE);
        assert_eq!(recap_date_correction(imported_at, Some(&live), Some(LISTED)), None);
        assert_eq!(recap_date_correction(imported_at, None, Some(LISTED)), None);
        // A pasted link has no row to go by.
        assert_eq!(recap_date_correction(imported_at, Some(&recap), None), None);
    }

    #[tokio::test]
    async fn the_sync_writes_a_held_recap_again_only_when_its_meeting_is_misdated() {
        let pool = test_pool().await;
        // insert_meeting dates both 2026-11-03 16:30 UTC.
        insert_meeting(&pool, "recap", "Weekly sync").await;
        source(COPILOT_RECAP_SOURCE).insert(&pool, "recap").await.unwrap();
        insert_meeting(&pool, "live", "Weekly sync").await;
        source(LIVE_RECORDING_SOURCE).insert(&pool, "live").await.unwrap();

        assert!(recap_meeting_needs_redate(&pool, "recap", Some(LISTED)).await.unwrap());
        assert!(!recap_meeting_needs_redate(&pool, "recap", Some("2026-11-03T16:40:00.000Z")).await.unwrap());
        assert!(!recap_meeting_needs_redate(&pool, "recap", None).await.unwrap());
        assert!(!recap_meeting_needs_redate(&pool, "live", Some(LISTED)).await.unwrap());
        assert!(!recap_meeting_needs_redate(&pool, "gone", Some(LISTED)).await.unwrap());

        assert!(MeetingsRepository::update_meeting_date(&pool, "recap", listed()).await.unwrap());
        assert!(!recap_meeting_needs_redate(&pool, "recap", Some(LISTED)).await.unwrap(), "moved once, then settled");
        assert!(!MeetingsRepository::update_meeting_date(&pool, "gone", listed()).await.unwrap());
    }
}

#[cfg(test)]
mod preserved_field_tests {
    use super::*;
    use serde_json::json;

    fn stored_result() -> String {
        json!({
            "markdown": "old body",
            "english_cache": { "markdown": "english body" },
            "provenance": { "provider": "anthropic", "model": "claude-opus-5" },
        })
        .to_string()
    }

    #[test]
    fn editor_save_keeps_provenance_and_english_cache() {
        let payload = json!({ "markdown": "edited body", "summary_json": [] });

        let merged = merge_preserved_result_fields(payload, Some(&stored_result()));

        assert_eq!(merged["markdown"], "edited body");
        assert_eq!(merged["provenance"]["model"], "claude-opus-5");
        assert_eq!(merged["english_cache"]["markdown"], "english body");
    }

    #[test]
    fn incoming_payload_wins_over_stored_preserved_fields() {
        let payload = json!({
            "markdown": "regenerated",
            "provenance": { "provider": "ollama", "model": "llama3" },
        });

        let merged = merge_preserved_result_fields(payload, Some(&stored_result()));

        assert_eq!(merged["provenance"]["model"], "llama3");
    }

    #[test]
    fn missing_or_unparseable_existing_result_leaves_payload_untouched() {
        let payload = json!({ "markdown": "edited body" });
        assert_eq!(
            merge_preserved_result_fields(payload.clone(), None),
            payload
        );
        assert_eq!(
            merge_preserved_result_fields(payload.clone(), Some("{not json")),
            payload
        );
    }

    #[test]
    fn non_object_payload_is_passed_through() {
        let payload = json!("just a string");
        assert_eq!(
            merge_preserved_result_fields(payload.clone(), Some(&stored_result())),
            payload
        );
    }
}
