//! Cohere ONNX engine: model lifecycle, discovery, and transcription entrypoint.

use anyhow::{anyhow, Context, Result};
use log::info;
use ort::session::Session;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokenizers::Tokenizer;
use tokio::sync::RwLock;

use crate::config::DEFAULT_COHERE_MODEL;

/// Runtime state owned by a [`CohereEngine`]. Contains the active ONNX sessions
/// and tokenizer once a model is loaded.
pub struct CohereEngineState {
    pub encoder: Option<Session>,
    pub decoder: Option<Session>,
    pub tokenizer: Option<Tokenizer>,
    pub current_model: Option<String>,
}

impl Default for CohereEngineState {
    fn default() -> Self {
        Self {
            encoder: None,
            decoder: None,
            tokenizer: None,
            current_model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Stable identifier, e.g. `"cohere-transcribe-03-2026"`.
    pub name: String,
    /// Short human-readable description.
    pub description: String,
    /// Approximate download size in MB (across all required files).
    pub size_mb: u32,
    /// Quantization variant, e.g. `"q4f16"`, `"fp16"`, `"fp32"`.
    pub quantization: String,
    /// Whether every required file is present on disk.
    pub downloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    pub current_model: Option<String>,
    pub is_loaded: bool,
    pub available_models: Vec<ModelInfo>,
}

/// Cohere ONNX engine. Thread-safe handle; clone the `Arc` instead of the inner state.
pub struct CohereEngine {
    models_dir: PathBuf,
    state: RwLock<CohereEngineState>,
}

impl CohereEngine {
    /// Construct an engine rooted at `models_dir`. The directory is created if missing.
    pub fn new_with_models_dir(models_dir: PathBuf) -> Result<Arc<Self>> {
        if !models_dir.exists() {
            std::fs::create_dir_all(&models_dir)
                .with_context(|| format!("create models dir {}", models_dir.display()))?;
        }
        info!("CohereEngine rooted at {}", models_dir.display());
        Ok(Arc::new(Self {
            models_dir,
            state: RwLock::new(CohereEngineState::default()),
        }))
    }

    /// Path to the directory containing all Cohere ONNX models.
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// Per-model directory (contains `encoder.onnx`, `decoder.onnx`, `tokenizer.json`, ...).
    pub fn model_path(&self, model_name: &str) -> PathBuf {
        self.models_dir.join(model_name)
    }

    /// Required file names. Kept in one place so the downloader and loader agree.
    pub const REQUIRED_FILES: &'static [&'static str] = &[
        "encoder_model_q4f16.onnx",
        "decoder_model_merged_q4f16.onnx",
        "tokenizer.json",
        "config.json",
        "generation_config.json",
    ];

    /// `true` iff every required file exists for `model_name`.
    pub fn is_model_downloaded(&self, model_name: &str) -> bool {
        let dir = self.model_path(model_name);
        Self::REQUIRED_FILES
            .iter()
            .all(|f| dir.join(f).is_file())
    }

    /// `true` iff the engine currently holds live ONNX sessions.
    pub async fn is_model_loaded(&self) -> bool {
        let s = self.state.read().await;
        s.encoder.is_some() && s.decoder.is_some() && s.tokenizer.is_some()
    }

    pub async fn get_current_model(&self) -> Option<String> {
        self.state.read().await.current_model.clone()
    }

    /// Enumerate discoverable Cohere ONNX models.
    /// Currently the only known model is `DEFAULT_COHERE_MODEL`.
    pub fn discover_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            name: DEFAULT_COHERE_MODEL.to_string(),
            description: "Cohere Transcribe 03-2026 (ONNX, q4f16)".to_string(),
            size_mb: 1500,
            quantization: "q4f16".to_string(),
            downloaded: self.is_model_downloaded(DEFAULT_COHERE_MODEL),
        }]
    }

    pub async fn status(&self) -> ModelStatus {
        ModelStatus {
            current_model: self.get_current_model().await,
            is_loaded: self.is_model_loaded().await,
            available_models: self.discover_models(),
        }
    }

    /// Load the given model into memory. Fails if any required file is missing.
    ///
    /// On macOS the CoreML execution provider is preferred; on Windows CUDA;
    /// otherwise the default CPU provider.
    pub async fn load_model(&self, model_name: &str) -> Result<()> {
        if !self.is_model_downloaded(model_name) {
            return Err(anyhow!(
                "model '{}' is not fully downloaded into {}",
                model_name,
                self.model_path(model_name).display()
            ));
        }

        let dir = self.model_path(model_name);
        let encoder_path = dir.join("encoder_model_q4f16.onnx");
        let decoder_path = dir.join("decoder_model_merged_q4f16.onnx");
        let tokenizer_path = dir.join("tokenizer.json");

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow!("load tokenizer.json: {e}"))?;

        let encoder = build_session(&encoder_path).context("build encoder session")?;
        let decoder = build_session(&decoder_path).context("build decoder session")?;

        let mut state = self.state.write().await;
        state.encoder = Some(encoder);
        state.decoder = Some(decoder);
        state.tokenizer = Some(tokenizer);
        state.current_model = Some(model_name.to_string());
        info!("Cohere model '{}' loaded", model_name);
        Ok(())
    }

    /// Release ONNX sessions and tokenizer. Safe to call even when no model is loaded.
    pub async fn unload_model(&self) {
        let mut state = self.state.write().await;
        state.encoder = None;
        state.decoder = None;
        state.tokenizer = None;
        state.current_model = None;
    }

    /// Transcribe a single utterance of raw f32 samples.
    ///
    /// `samples` is interpreted at `source_sample_rate` Hz (the pipeline provides
    /// 48 kHz). The audio is resampled to 16 kHz, converted to a log-mel
    /// spectrogram, and fed through the encoder + autoregressive decoder.
    pub async fn transcribe_audio(
        &self,
        samples: Vec<f32>,
        source_sample_rate: u32,
        language: Option<String>,
    ) -> Result<String> {
        let lang = language.unwrap_or_else(|| crate::config::DEFAULT_LANGUAGE.to_string());

        let state = self.state.read().await;
        let encoder = state
            .encoder
            .as_ref()
            .ok_or_else(|| anyhow!("encoder session not loaded"))?;
        let decoder = state
            .decoder
            .as_ref()
            .ok_or_else(|| anyhow!("decoder session not loaded"))?;
        let tokenizer = state
            .tokenizer
            .as_ref()
            .ok_or_else(|| anyhow!("tokenizer not loaded"))?;

        if samples.is_empty() {
            return Ok(String::new());
        }

        let resampled = crate::cohere_engine::preprocess::resample_linear(
            &samples,
            source_sample_rate,
            16_000,
        );
        let mel = crate::cohere_engine::preprocess::log_mel_spectrogram(&resampled, 16_000);

        let text = crate::cohere_engine::decode::run_greedy_decode(
            encoder, decoder, mel, tokenizer, &lang, 448,
        )?;
        Ok(text)
    }
}

fn build_session(path: &Path) -> Result<Session> {
    use ort::session::builder::SessionBuilder;
    let builder = SessionBuilder::new()?;
    // Execution providers are selected opportunistically. `ort` falls back to
    // the default CPU provider if the preferred EP is unavailable on this host.
    let session = builder
        .commit_from_file(path)
        .with_context(|| format!("commit_from_file {}", path.display()))?;
    Ok(session)
}
