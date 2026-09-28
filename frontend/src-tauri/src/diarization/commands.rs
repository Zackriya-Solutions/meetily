//! Tauri commands for speaker identification.
use super::models::{self, DownloadProgress, ModelsStatus};
use tauri::{AppHandle, Emitter, Runtime};

pub const MODEL_DOWNLOAD_PROGRESS_EVENT: &str = "diarization-model-download-progress";

pub(crate) fn emit_download_progress<R: Runtime>(app: &AppHandle<R>, progress: DownloadProgress) {
    let _ = app.emit(MODEL_DOWNLOAD_PROGRESS_EVENT, progress);
}

#[tauri::command]
pub async fn diarization_models_status() -> Result<ModelsStatus, String> {
    let dir = models::models_directory().map_err(|e| e.to_string())?;
    Ok(models::status(&dir))
}

#[tauri::command]
pub async fn diarization_download_models<R: Runtime>(app: AppHandle<R>) -> Result<ModelsStatus, String> {
    let dir = models::models_directory().map_err(|e| e.to_string())?;
    models::ensure_models(&dir, |p| emit_download_progress(&app, p), || false)
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(models::status(&dir))
}

#[tauri::command]
pub async fn diarization_delete_models() -> Result<ModelsStatus, String> {
    let dir = models::models_directory().map_err(|e| e.to_string())?;
    models::delete_models(&dir).map_err(|e| e.to_string())?;
    Ok(models::status(&dir))
}

use super::jobs::{self, IdentifyRequest, JobStatus};
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::speaker::{MeetingSpeaker, ReassignTarget, SpeakersRepository};
use crate::state::AppState;
use std::path::PathBuf;

#[tauri::command]
pub async fn start_speaker_identification<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    num_speakers: Option<u32>,
) -> Result<(), String> {
    // enqueue refuses a meeting that already has a job or is being retranscribed. Batch engine
    // use is serialised by the engine lock, so other meetings' jobs do not block this one.
    jobs::enqueue(
        &app,
        IdentifyRequest {
            meeting_id,
            folder_path: PathBuf::from(meeting_folder_path),
            num_speakers: num_speakers.map(|n| n as usize),
            automatic: false,
        },
    )
}

#[tauri::command]
pub async fn cancel_speaker_identification<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<(), String> {
    jobs::cancel(&app, &meeting_id)
}

#[tauri::command]
pub async fn get_speaker_identification_status(meeting_id: String) -> Result<Option<JobStatus>, String> {
    Ok(jobs::status(&meeting_id))
}

/// Refuse speaker edits that a running identification or retranscription would overwrite.
fn ensure_editable(meeting_id: &str) -> Result<(), String> {
    if jobs::is_active(meeting_id) {
        return Err("Speaker identification is running for this meeting; try again when it finishes".into());
    }
    if jobs::is_retranscribing(meeting_id) {
        return Err("This meeting is being retranscribed; try again when it finishes".into());
    }
    Ok(())
}

async fn refresh_json(state: &AppState, meeting_id: &str) {
    let pool = state.db_manager.pool();
    if let Ok(Some(meeting)) = MeetingsRepository::get_meeting_metadata(pool, meeting_id).await {
        if let Some(folder) = meeting.folder_path {
            if let Err(e) = jobs::rewrite_transcripts_json(pool, meeting_id, std::path::Path::new(&folder)).await {
                log::warn!("Failed to rewrite transcripts.json for {}: {:#}", meeting_id, e);
            }
        }
    }
}

#[tauri::command]
pub async fn api_list_meeting_speakers(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<MeetingSpeaker>, String> {
    SpeakersRepository::list(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to load speakers: {}", e))
}

#[tauri::command]
pub async fn api_rename_meeting_speaker(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    speaker_key: String,
    display_name: String,
) -> Result<(), String> {
    ensure_editable(&meeting_id)?;
    SpeakersRepository::rename(state.db_manager.pool(), &meeting_id, &speaker_key, &display_name)
        .await
        .map_err(|e| format!("Failed to rename speaker: {}", e))?;
    refresh_json(&state, &meeting_id).await;
    Ok(())
}

#[tauri::command]
pub async fn api_merge_meeting_speakers(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    from_key: String,
    into_key: String,
) -> Result<(), String> {
    ensure_editable(&meeting_id)?;
    SpeakersRepository::merge(state.db_manager.pool(), &meeting_id, &from_key, &into_key)
        .await
        .map_err(|e| format!("Failed to merge speakers: {}", e))?;
    refresh_json(&state, &meeting_id).await;
    Ok(())
}

/// `speaker_key` None assigns the row to a new speaker.
#[tauri::command]
pub async fn api_set_transcript_speaker(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    transcript_id: String,
    speaker_key: Option<String>,
) -> Result<String, String> {
    ensure_editable(&meeting_id)?;
    let target = match speaker_key {
        Some(k) => ReassignTarget::Existing(k),
        None => ReassignTarget::New,
    };
    let key = SpeakersRepository::reassign_row(state.db_manager.pool(), &meeting_id, &transcript_id, target)
        .await
        .map_err(|e| format!("Failed to change speaker: {}", e))?;
    refresh_json(&state, &meeting_id).await;
    Ok(key)
}
