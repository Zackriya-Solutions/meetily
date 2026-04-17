//! Tauri commands exposing the Cohere ONNX engine to the frontend.
//!
//! The surface intentionally mirrors the existing `whisper_*` / `parakeet_*`
//! commands (same names with the `cohere_` prefix) so the UI layer can swap
//! providers with minimal churn.

use anyhow::anyhow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{command, AppHandle, Emitter, Manager, Runtime};

use super::downloader;
use super::engine::{CohereEngine, ModelInfo, ModelStatus};

// Singleton Cohere engine.
static COHERE_ENGINE: Mutex<Option<Arc<CohereEngine>>> = Mutex::new(None);
// Cached models directory (set at app startup).
static MODELS_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
// Cancellation flag for the active download.
static DOWNLOAD_CANCEL: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// Resolve `<app_data>/cohere_models` and persist it for subsequent calls.
/// Should be invoked exactly once, during Tauri setup.
pub fn set_models_directory<R: Runtime>(app: &AppHandle<R>) {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .expect("app_data_dir unavailable");
    let dir = app_data_dir.join("cohere_models");
    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::error!("Failed to create cohere models directory: {}", e);
            return;
        }
    }
    log::info!("Cohere models directory set to {}", dir.display());
    *MODELS_DIR.lock().unwrap() = Some(dir);
}

/// Return a cloned `Arc` to the current engine if one has been constructed.
/// Used by batch code paths (import, retranscription) that need to unload the
/// model without re-resolving the app handle.
pub fn current_engine() -> Option<Arc<CohereEngine>> {
    COHERE_ENGINE.lock().unwrap().as_ref().cloned()
}

fn models_dir_or_err() -> Result<PathBuf, String> {
    MODELS_DIR
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Cohere models directory not initialised".to_string())
}

/// Fetch or lazily instantiate the singleton engine.
pub async fn get_or_init_engine<R: Runtime>(_app: &AppHandle<R>) -> Result<Arc<CohereEngine>, String> {
    {
        let guard = COHERE_ENGINE.lock().unwrap();
        if let Some(eng) = guard.as_ref() {
            return Ok(eng.clone());
        }
    }
    let dir = models_dir_or_err()?;
    let engine = CohereEngine::new_with_models_dir(dir)
        .map_err(|e| format!("initialise CohereEngine: {e}"))?;
    *COHERE_ENGINE.lock().unwrap() = Some(engine.clone());
    Ok(engine)
}

#[command]
pub async fn cohere_init<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    get_or_init_engine(&app_handle).await?;
    Ok(())
}

#[command]
pub async fn cohere_get_available_models<R: Runtime>(
    app_handle: AppHandle<R>,
) -> Result<Vec<ModelInfo>, String> {
    let engine = get_or_init_engine(&app_handle).await?;
    Ok(engine.discover_models())
}

#[command]
pub async fn cohere_get_model_status<R: Runtime>(
    app_handle: AppHandle<R>,
) -> Result<ModelStatus, String> {
    let engine = get_or_init_engine(&app_handle).await?;
    Ok(engine.status().await)
}

#[command]
pub async fn cohere_is_model_loaded<R: Runtime>(app_handle: AppHandle<R>) -> Result<bool, String> {
    let engine = get_or_init_engine(&app_handle).await?;
    Ok(engine.is_model_loaded().await)
}

#[command]
pub async fn cohere_get_current_model<R: Runtime>(
    app_handle: AppHandle<R>,
) -> Result<Option<String>, String> {
    let engine = get_or_init_engine(&app_handle).await?;
    Ok(engine.get_current_model().await)
}

#[command]
pub async fn cohere_load_model<R: Runtime>(
    app_handle: AppHandle<R>,
    model_name: String,
) -> Result<(), String> {
    let engine = get_or_init_engine(&app_handle).await?;
    let _ = app_handle.emit(
        "cohere-model-loading-started",
        serde_json::json!({ "modelName": model_name }),
    );
    let result = engine
        .load_model(&model_name)
        .await
        .map_err(|e| format!("load cohere model: {e}"));
    match &result {
        Ok(_) => {
            let _ = app_handle.emit(
                "cohere-model-loading-completed",
                serde_json::json!({ "modelName": model_name }),
            );
        }
        Err(err) => {
            let _ = app_handle.emit(
                "cohere-model-loading-failed",
                serde_json::json!({
                    "modelName": model_name,
                    "error": err,
                }),
            );
        }
    }
    result
}

#[command]
pub async fn cohere_unload_model<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    let engine = get_or_init_engine(&app_handle).await?;
    engine.unload_model().await;
    Ok(())
}

#[command]
pub async fn cohere_download_model<R: Runtime>(
    app_handle: AppHandle<R>,
    model_name: String,
) -> Result<(), String> {
    let engine = get_or_init_engine(&app_handle).await?;
    let cancel = Arc::new(AtomicBool::new(false));
    *DOWNLOAD_CANCEL.lock().unwrap() = Some(cancel.clone());
    let result = downloader::download_model(app_handle.clone(), engine, model_name, cancel)
        .await
        .map_err(|e| format!("download cohere model: {e}"));
    *DOWNLOAD_CANCEL.lock().unwrap() = None;
    result
}

#[command]
pub async fn cohere_cancel_download() -> Result<(), String> {
    if let Some(flag) = DOWNLOAD_CANCEL.lock().unwrap().as_ref() {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[command]
pub async fn cohere_transcribe_audio<R: Runtime>(
    app_handle: AppHandle<R>,
    samples: Vec<f32>,
    sample_rate: u32,
    language: Option<String>,
) -> Result<String, String> {
    let engine = get_or_init_engine(&app_handle).await?;
    engine
        .transcribe_audio(samples, sample_rate, language)
        .await
        .map_err(|e| format!("transcribe: {e}"))
}

#[command]
pub async fn cohere_validate_model_ready<R: Runtime>(
    app_handle: AppHandle<R>,
) -> Result<(), String> {
    let engine = get_or_init_engine(&app_handle).await?;
    if engine.is_model_loaded().await {
        Ok(())
    } else {
        Err("Cohere ONNX model is not ready. Open Settings → Transcript and download the model.".to_string())
    }
}

#[command]
pub async fn cohere_get_models_directory() -> Result<String, String> {
    models_dir_or_err().map(|p| p.to_string_lossy().to_string())
}

// Re-export anyhow so modules within cohere_engine can import it without adding
// it to their own scope repeatedly.
#[allow(dead_code)]
fn _use_anyhow(_: anyhow::Error) {}

// Suppress an "unused" warning for `anyhow::anyhow!` in this module; the macro
// is routinely imported here as part of the public contract.
#[allow(dead_code)]
fn _anyhow_macro_anchor() -> anyhow::Error {
    anyhow!("anchor")
}
