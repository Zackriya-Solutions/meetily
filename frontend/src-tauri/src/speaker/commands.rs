//! Tauri commands for speaker identification settings and per-meeting speaker names.

use crate::database::repositories::speaker::{MeetingSpeakerName, SpeakersRepository};
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

#[derive(Serialize)]
pub struct SpeakerIdentificationStatus {
    pub enabled: bool,
    pub model_downloaded: bool,
    pub downloading: bool,
    pub model_size_bytes: u64,
}

#[tauri::command]
pub async fn speaker_get_status() -> Result<SpeakerIdentificationStatus, String> {
    Ok(SpeakerIdentificationStatus {
        enabled: super::is_enabled(),
        model_downloaded: super::is_model_downloaded(),
        downloading: super::is_downloading(),
        model_size_bytes: super::MODEL_SIZE_BYTES,
    })
}

/// Takes effect from the next recording, import or re-transcription.
#[tauri::command]
pub async fn speaker_set_enabled<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    super::set_enabled(&app, enabled).map_err(|e| {
        log::error!("Failed to save speaker identification preference: {:#}", e);
        "Could not save the speaker identification setting.".to_string()
    })
}

#[tauri::command]
pub async fn speaker_download_model<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    match super::download_model(&app).await {
        Ok(()) => {
            let _ = app.emit("speaker-model-download-complete", ());
            Ok(())
        }
        Err(e) => {
            log::error!("Speaker model download failed: {:#}", e);
            let message = format!("Voice model download failed: {}", e);
            let _ = app.emit("speaker-model-download-error", &message);
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn speaker_delete_model() -> Result<(), String> {
    super::delete_model().await.map_err(|e| {
        log::error!("Failed to delete speaker model: {:#}", e);
        "Could not delete the voice model.".to_string()
    })
}

#[tauri::command]
pub async fn api_get_meeting_speakers(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<MeetingSpeakerName>, String> {
    SpeakersRepository::list_names(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| {
            log::error!("Failed to load speaker names: {}", e);
            "Could not load speaker names.".to_string()
        })
}

/// Renames a speaker within one meeting. An empty name restores the default label.
#[tauri::command]
pub async fn api_rename_meeting_speaker(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    speaker_id: String,
    display_name: String,
) -> Result<(), String> {
    if display_name.chars().count() > 100 {
        return Err("Speaker name is too long (max 100 characters).".to_string());
    }
    SpeakersRepository::rename(state.db_manager.pool(), &meeting_id, &speaker_id, &display_name)
        .await
        .map_err(|e| {
            log::error!("Failed to rename speaker: {}", e);
            "Could not rename the speaker.".to_string()
        })
}

/// Moves every segment of `from_speaker_id` to `to_speaker_id` (fixes a voice split in two).
#[tauri::command]
pub async fn api_merge_meeting_speakers(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    from_speaker_id: String,
    to_speaker_id: String,
) -> Result<u64, String> {
    if from_speaker_id == to_speaker_id {
        return Ok(0);
    }
    SpeakersRepository::merge(state.db_manager.pool(), &meeting_id, &from_speaker_id, &to_speaker_id)
        .await
        .map_err(|e| {
            log::error!("Failed to merge speakers: {}", e);
            "Could not merge the speakers.".to_string()
        })
}

/// Reassigns one transcript segment to another speaker (or clears it with `None`).
#[tauri::command]
pub async fn api_set_transcript_speaker(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    transcript_id: String,
    speaker_id: Option<String>,
) -> Result<(), String> {
    match SpeakersRepository::set_segment_speaker(
        state.db_manager.pool(),
        &meeting_id,
        &transcript_id,
        speaker_id.as_deref(),
    )
    .await
    {
        Ok(true) => Ok(()),
        Ok(false) => Err("Transcript segment not found.".to_string()),
        Err(e) => {
            log::error!("Failed to set transcript speaker: {}", e);
            Err("Could not update the speaker.".to_string())
        }
    }
}
