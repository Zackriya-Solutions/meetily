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
