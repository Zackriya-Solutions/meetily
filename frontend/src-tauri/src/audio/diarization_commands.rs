// Tauri commands for diarization model management (download, status check, etc.)

use crate::audio::diarization;
use tauri::{AppHandle, Runtime};

#[tauri::command]
pub fn diarization_is_available() -> bool {
    diarization::diarization_models_available()
}

#[tauri::command]
pub fn diarization_check_models() -> serde_json::Value {
    use crate::audio::diarization::{SEGMENTATION_MODEL_FILE, EMBEDDING_MODEL_FILE};

    // Check each model individually
    let dir = {
        let guard = crate::audio::diarization::models_dir_pub();
        guard
    };

    let (seg_ok, emb_ok) = if let Some(ref dir) = dir {
        let seg = dir.join(SEGMENTATION_MODEL_FILE);
        let emb = dir.join(EMBEDDING_MODEL_FILE);
        let seg_ok = seg.exists() && seg.metadata().map(|m| m.len() > 0).unwrap_or(false);
        let emb_ok = emb.exists() && emb.metadata().map(|m| m.len() > 0).unwrap_or(false);
        (seg_ok, emb_ok)
    } else {
        (false, false)
    };

    serde_json::json!({
        "segmentation_available": seg_ok,
        "embedding_available": emb_ok,
        "both_available": seg_ok && emb_ok,
    })
}

#[tauri::command]
pub async fn diarization_download_model<R: Runtime>(
    app: AppHandle<R>,
    model_type: String,
) -> Result<(), String> {
    diarization::download_diarization_model(app, &model_type)
        .await
        .map_err(|e| e.to_string())
}

