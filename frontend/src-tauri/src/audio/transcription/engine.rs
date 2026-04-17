// audio/transcription/engine.rs
//
// TranscriptionEngine routing for the Cohere ONNX build.
//
// The Whisper and Parakeet providers are being removed; for this milestone
// the enum carries only a single `CohereOnnx` variant. Keeping it as an
// enum (rather than a type alias) leaves room for adding more providers
// later without a signature churn on the callers.

use log::{info, warn};
use std::sync::Arc;
use tauri::{AppHandle, Runtime};

// ============================================================================
// TRANSCRIPTION ENGINE ENUM
// ============================================================================

pub enum TranscriptionEngine {
    CohereOnnx(Arc<crate::cohere_engine::CohereEngine>),
}

impl TranscriptionEngine {
    pub async fn is_model_loaded(&self) -> bool {
        match self {
            Self::CohereOnnx(engine) => engine.is_model_loaded().await,
        }
    }

    pub async fn get_current_model(&self) -> Option<String> {
        match self {
            Self::CohereOnnx(engine) => engine.get_current_model().await,
        }
    }

    pub fn provider_name(&self) -> &'static str {
        match self {
            Self::CohereOnnx(_) => "Cohere ONNX",
        }
    }
}

// ============================================================================
// MODEL VALIDATION AND INITIALIZATION
// ============================================================================

/// Confirm the Cohere ONNX model is loaded before starting a recording.
/// Emits a user-actionable error if the model has not been downloaded yet.
pub async fn validate_transcription_model_ready<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), String> {
    info!("🔍 Validating Cohere ONNX model...");
    let engine = crate::cohere_engine::commands::get_or_init_engine(app).await?;
    if engine.is_model_loaded().await {
        let model_name = engine
            .get_current_model()
            .await
            .unwrap_or_else(|| "cohere-transcribe-03-2026".to_string());
        info!("✅ Cohere model validation successful: {} is ready", model_name);
        Ok(())
    } else {
        warn!("❌ Cohere model is not loaded");
        Err(
            "Cohere ONNX model is not ready. Open Settings → Transcript and download the model."
                .to_string(),
        )
    }
}

/// Return the singleton Cohere engine wrapped in the `TranscriptionEngine` enum,
/// attempting to auto-load the configured model on the way if it isn't loaded yet.
pub async fn get_or_init_transcription_engine<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<TranscriptionEngine, String> {
    info!("🎙️  Initializing Cohere ONNX transcription engine");

    let engine = crate::cohere_engine::commands::get_or_init_engine(app).await?;

    if !engine.is_model_loaded().await {
        let model_name = crate::config::DEFAULT_COHERE_MODEL.to_string();
        info!("📥 No Cohere model loaded; attempting to load '{}'", model_name);
        if let Err(e) = engine.load_model(&model_name).await {
            return Err(format!(
                "Cohere model '{}' could not be loaded: {}. Download it from Settings → Transcript.",
                model_name, e
            ));
        }
        info!("✅ Cohere model '{}' loaded", model_name);
    }

    Ok(TranscriptionEngine::CohereOnnx(engine))
}
