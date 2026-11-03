use crate::diarization_engine::{
    DiarizationConfig, DiarizationEngine, DiarizationModelInfo, StreamingPreset,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{command, AppHandle, Manager, Runtime};
use tauri_plugin_store::StoreExt;

// Global diarization engine instance
pub static DIARIZATION_ENGINE: Mutex<Option<Arc<DiarizationEngine>>> = Mutex::new(None);

// Global models directory path
static MODELS_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Load diarization preferences from store
pub fn load_diarization_preferences<R: Runtime>(app: &AppHandle<R>) -> DiarizationConfig {
    let store = match app.store("diarization_preferences.json") {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to open diarization_preferences store: {}, using defaults", e);
            return DiarizationConfig::default();
        }
    };

    if let Some(val) = store.get("config") {
        match serde_json::from_value::<DiarizationConfig>(val.clone()) {
            Ok(c) => {
                log::info!(
                    "📋 Loaded diarization preferences: enabled={}, max_speakers={}, preset={:?}",
                    c.enabled,
                    c.max_speakers,
                    c.preset
                );
                return c;
            }
            Err(e) => {
                log::warn!("Failed to deserialize diarization preferences: {}, using defaults", e);
            }
        }
    }

    let mut default_cfg = DiarizationConfig::default();
    if let Some(models_dir) = get_models_directory() {
        let model_path = models_dir.join("diarization").join("nemotron3_diar_v3.onnx");
        if model_path.exists() {
            log::info!("🎤 Nemotron-3 model found on disk, auto-enabling diarization by default");
            default_cfg.enabled = true;
        }
    }
    default_cfg
}

/// Save diarization preferences to store
pub fn save_diarization_preferences<R: Runtime>(
    app: &AppHandle<R>,
    config: &DiarizationConfig,
) -> Result<(), String> {
    let store = app
        .store("diarization_preferences.json")
        .map_err(|e| format!("Failed to access store: {}", e))?;

    let val = serde_json::to_value(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    store.set("config", val);
    store
        .save()
        .map_err(|e| format!("Failed to save store to disk: {}", e))?;
    log::info!("💾 Successfully persisted diarization preferences to disk");
    Ok(())
}

/// Initialize the models directory path using app_data_dir
pub fn set_models_directory<R: Runtime>(app: &AppHandle<R>) {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .expect("Failed to get app data dir");

    let models_dir = app_data_dir.join("models");

    if !models_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&models_dir) {
            log::error!("Failed to create models directory: {}", e);
            return;
        }
    }

    log::info!(
        "Diarization models directory set to: {}",
        models_dir.display()
    );

    let mut guard = MODELS_DIR.lock().unwrap();
    *guard = Some(models_dir);
}

fn get_models_directory() -> Option<PathBuf> {
    MODELS_DIR.lock().unwrap().clone()
}

pub fn get_diarization_engine() -> Option<Arc<DiarizationEngine>> {
    let guard = DIARIZATION_ENGINE.lock().unwrap();
    guard.as_ref().cloned()
}

#[command]
pub async fn diarization_init<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let existing_engine = {
        let guard = DIARIZATION_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    let config = load_diarization_preferences(&app);

    if let Some(engine) = existing_engine {
        // Ensure latest config from store is active
        engine.update_config(config).await;
        return Ok(());
    }

    let models_dir = get_models_directory().unwrap_or_else(|| PathBuf::from("models"));
    let engine = Arc::new(DiarizationEngine::new(models_dir));
    engine.update_config(config).await;

    {
        let mut guard = DIARIZATION_ENGINE.lock().unwrap();
        *guard = Some(engine);
    }
    log::info!("✅ Nemotron-3 Diarization engine initialized with stored preferences");
    Ok(())
}

#[command]
pub async fn diarization_get_model_info() -> Result<DiarizationModelInfo, String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    Ok(engine.get_model_info().await)
}

#[command]
pub async fn diarization_is_model_loaded() -> Result<bool, String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    Ok(engine.is_model_loaded().await)
}

#[command]
pub async fn diarization_is_model_available() -> Result<bool, String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    Ok(engine.is_model_available().await)
}

#[command]
pub async fn diarization_load_model() -> Result<(), String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    engine.load_model().await.map_err(|e| e.to_string())
}

#[command]
pub async fn diarization_unload_model() -> Result<(), String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    engine.unload_model().await;
    Ok(())
}

#[command]
pub async fn diarization_download_model<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    engine.download_model(app).await.map_err(|e| e.to_string())
}

#[command]
pub async fn diarization_cancel_download() -> Result<bool, String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    Ok(engine.cancel_download().await)
}

#[command]
pub async fn diarization_delete_model() -> Result<(), String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    engine.delete_model().await.map_err(|e| e.to_string())
}

#[command]
pub async fn diarization_get_settings() -> Result<DiarizationConfig, String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    Ok(engine.get_config().await)
}

#[command]
pub async fn diarization_save_settings<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
    preset: Option<String>,
    max_speakers: Option<usize>,
    #[allow(non_snake_case)]
    maxSpeakers: Option<usize>,
    threshold: Option<f32>,
) -> Result<(), String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;

    let streaming_preset = match preset.as_deref().map(|s| s.to_lowercase().replace("-", "_").replace(" ", "_")) {
        Some(ref s) if s == "lowlatency" || s == "low_latency" => StreamingPreset::LowLatency,
        Some(ref s) if s == "verylowlatency" || s == "very_low_latency" => StreamingPreset::VeryLowLatency,
        Some(ref s) if s == "ultralowlatency" || s == "ultra_low_latency" => StreamingPreset::UltraLowLatency,
        _ => StreamingPreset::Offline,
    };

    let effective_max_speakers = max_speakers.or(maxSpeakers).unwrap_or(4).clamp(1, 8);

    let new_config = DiarizationConfig {
        enabled,
        preset: streaming_preset,
        max_speakers: effective_max_speakers,
        threshold: threshold.unwrap_or(0.40).clamp(0.1, 0.9),
    };

    engine.update_config(new_config.clone()).await;
    let _ = save_diarization_preferences(&app, &new_config);
    log::info!(
        "Saved diarization settings: enabled={}, max_speakers={}, preset={:?}",
        enabled,
        effective_max_speakers,
        streaming_preset
    );
    Ok(())
}

#[command]
pub async fn diarization_is_ready() -> Result<bool, String> {
    let engine = match get_diarization_engine() {
        Some(e) => e,
        None => return Ok(false),
    };

    let config = engine.get_config().await;
    if !config.enabled {
        return Ok(false);
    }

    Ok(engine.is_model_available().await)
}

#[command]
pub async fn open_diarization_models_folder() -> Result<(), String> {
    let engine = get_diarization_engine()
        .ok_or_else(|| "Diarization engine not initialized".to_string())?;
    let path = engine.models_dir();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}
