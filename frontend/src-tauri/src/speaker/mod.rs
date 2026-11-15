//! Speaker identification (diarization).
//!
//! Live recordings combine two signals:
//! 1. **Source**: the pipeline marks each VAD segment as microphone- or
//!    system-dominant (see `SourceEnergyTracker` in `audio/pipeline.rs`).
//! 2. **Voiceprint**: an optional local CAM++ ONNX model turns each segment
//!    into an embedding that is clustered into `speaker_<n>` ids.
//!
//! Without the model, recordings that capture system audio still get the
//! source-only `me` / `others` split. Import and re-transcription work on
//! the mixed file, so they use voiceprint clustering only.

pub mod clustering;
pub mod commands;
pub mod embedding;
pub mod fbank;
pub mod segmentation;

use anyhow::{anyhow, Context, Result};
use embedding::SpeakerEmbeddingModel;
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_store::StoreExt;

pub use clustering::{OnlineSpeakerClusterer, SpeakerSource};

/// 3D-Speaker CAM++ (Chinese + English, 16 kHz), exported by sherpa-onnx.
pub const MODEL_FILENAME: &str = "3dspeaker_campplus_sv_zh_en_16k_advanced.onnx";
const MODEL_URL: &str = "https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/0743f301363dec56491a490f6d6cbc9d67f9a3bf/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";
const MODEL_SHA256: &str = "aa3cfc16963a10586a9393f5035d6d6b57e98d358b347f80c2a30bf4f00ceba2";
pub const MODEL_SIZE_BYTES: u64 = 28_281_164;

const STORE_FILE: &str = "speaker_preferences.json";
const STORE_ENABLED_KEY: &str = "enabled";

static MODEL_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static ENABLED: AtomicBool = AtomicBool::new(true);
static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static LOADED_MODEL: Lazy<Mutex<Option<SharedModel>>> = Lazy::new(|| Mutex::new(None));

pub type SharedModel = Arc<Mutex<SpeakerEmbeddingModel>>;

/// Called from app setup: resolves the model directory and the saved preference.
pub fn init<R: Runtime>(app: &AppHandle<R>) {
    match app.path().app_data_dir() {
        Ok(dir) => {
            let models_dir = dir.join("models").join("speaker");
            if let Err(e) = std::fs::create_dir_all(&models_dir) {
                log::error!("Failed to create speaker models directory: {}", e);
            }
            *MODEL_DIR.lock().unwrap() = Some(models_dir);
        }
        Err(e) => log::error!("Speaker identification unavailable: no app data dir: {}", e),
    }

    if let Ok(store) = app.store(STORE_FILE) {
        if let Some(enabled) = store.get(STORE_ENABLED_KEY).and_then(|v| v.as_bool()) {
            ENABLED.store(enabled, Ordering::SeqCst);
        }
    }
    log::info!(
        "Speaker identification initialised (enabled={}, model_downloaded={})",
        is_enabled(),
        is_model_downloaded()
    );
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("{e}"))?;
    store.set(STORE_ENABLED_KEY, serde_json::Value::Bool(enabled));
    store.save().map_err(|e| anyhow!("{e}"))?;
    ENABLED.store(enabled, Ordering::SeqCst);
    Ok(())
}

pub fn model_path() -> Option<PathBuf> {
    MODEL_DIR.lock().unwrap().as_ref().map(|d| d.join(MODEL_FILENAME))
}

pub fn is_model_downloaded() -> bool {
    model_path().map_or(false, |p| {
        std::fs::metadata(&p).map_or(false, |m| m.len() == MODEL_SIZE_BYTES)
    })
}

pub fn is_downloading() -> bool {
    DOWNLOADING.load(Ordering::SeqCst)
}

/// Returns the cached voiceprint model, loading it on first use.
/// `None` when the feature is disabled, the model isn't downloaded, or loading failed.
pub async fn get_model() -> Option<SharedModel> {
    if !is_enabled() || !is_model_downloaded() {
        return None;
    }
    if let Some(model) = LOADED_MODEL.lock().unwrap().clone() {
        return Some(model);
    }
    let path = model_path()?;
    match tokio::task::spawn_blocking(move || SpeakerEmbeddingModel::load(&path)).await {
        Ok(Ok(model)) => {
            let shared = Arc::new(Mutex::new(model));
            *LOADED_MODEL.lock().unwrap() = Some(shared.clone());
            Some(shared)
        }
        Ok(Err(e)) => {
            log::warn!("Speaker embedding model failed to load: {:#}", e);
            None
        }
        Err(e) => {
            log::warn!("Speaker embedding model load task failed: {}", e);
            None
        }
    }
}

fn unload_model() {
    *LOADED_MODEL.lock().unwrap() = None;
}

/// Computes a voiceprint off the async runtime. Errors are logged and yield `None`.
pub async fn embed(model: &SharedModel, samples_16k: Vec<f32>) -> Option<Vec<f32>> {
    let model = model.clone();
    match tokio::task::spawn_blocking(move || model.lock().ok()?.embed(&samples_16k).ok().flatten()).await {
        Ok(embedding) => embedding,
        Err(e) => {
            log::debug!("Speaker embedding task failed: {}", e);
            None
        }
    }
}

/// Batch diarization for import / re-transcription: splits VAD segments at
/// speaker changes and labels each piece. Without the model (or when disabled)
/// the segments come back unchanged and unlabelled.
pub async fn split_segments_by_speaker(
    segments: Vec<crate::audio::vad::SpeechSegment>,
) -> Vec<(crate::audio::vad::SpeechSegment, Option<String>)> {
    let Some(model) = get_model().await else {
        return segments.into_iter().map(|s| (s, None)).collect();
    };
    let result = tokio::task::spawn_blocking(move || {
        // Never lose the segments: on a poisoned lock or a panic inside the model,
        // hand them back unlabelled.
        let spans = {
            let mut model = match model.lock() {
                Ok(m) => m,
                Err(_) => return Err(segments),
            };
            let slices: Vec<&[f32]> = segments.iter().map(|s| s.samples.as_slice()).collect();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                segmentation::segment_by_speaker(&slices, |w| model.embed(w).ok().flatten())
            }))
        };
        let Ok(spans) = spans else {
            return Err(segments);
        };
        let mut out = Vec::new();
        for (segment, spans) in segments.into_iter().zip(spans) {
            for span in spans {
                let start_ms = segment.start_timestamp_ms + span.start as f64 / 16.0;
                let end_ms = segment.start_timestamp_ms + span.end as f64 / 16.0;
                out.push((
                    crate::audio::vad::SpeechSegment {
                        samples: segment.samples[span.start..span.end].to_vec(),
                        start_timestamp_ms: start_ms,
                        end_timestamp_ms: end_ms,
                        confidence: segment.confidence,
                    },
                    span.speaker,
                ));
            }
        }
        Ok(out)
    })
    .await;
    match result {
        Ok(Ok(labelled)) => {
            let speakers: std::collections::HashSet<_> = labelled.iter().filter_map(|(_, s)| s.as_ref()).collect();
            log::info!(
                "Speaker segmentation produced {} spans, {} speakers",
                labelled.len(),
                speakers.len()
            );
            labelled
        }
        Ok(Err(segments)) => segments.into_iter().map(|s| (s, None)).collect(),
        Err(e) => {
            // Unreachable in practice (the closure catches panics); kept explicit.
            log::error!("Speaker segmentation task failed: {}", e);
            Vec::new()
        }
    }
}

#[derive(Clone, serde::Serialize)]
struct DownloadProgress {
    downloaded: u64,
    total: u64,
    percent: u32,
}

/// Downloads and verifies the voiceprint model, emitting `speaker-model-download-progress`.
pub async fn download_model<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    if DOWNLOADING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(anyhow!("Speaker model download already in progress"));
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            DOWNLOADING.store(false, Ordering::SeqCst);
        }
    }
    let _reset = Reset;

    let final_path = model_path().ok_or_else(|| anyhow!("Speaker model directory not initialised"))?;
    let part_path = final_path.with_extension("onnx.part");

    let response = reqwest::Client::new()
        .get(MODEL_URL)
        .send()
        .await
        .context("Could not reach the model server")?
        .error_for_status()
        .context("Model server returned an error")?;
    let total = response.content_length().unwrap_or(MODEL_SIZE_BYTES);

    let mut file = tokio::fs::File::create(&part_path).await?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut last_percent = u32::MAX;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Download interrupted")?;
        hasher.update(&chunk);
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded += chunk.len() as u64;
        let percent = ((downloaded as f64 / total.max(1) as f64) * 100.0).min(100.0) as u32;
        if percent != last_percent {
            last_percent = percent;
            let _ = app.emit(
                "speaker-model-download-progress",
                DownloadProgress { downloaded, total, percent },
            );
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    drop(file);

    let digest = hasher.finalize();
    let digest_hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if digest_hex != MODEL_SHA256 || downloaded != MODEL_SIZE_BYTES {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(anyhow!("Downloaded speaker model failed verification"));
    }
    tokio::fs::rename(&part_path, &final_path).await?;
    unload_model();
    log::info!("Speaker model downloaded and verified");
    Ok(())
}

pub async fn delete_model() -> Result<()> {
    unload_model();
    if let Some(path) = model_path() {
        if path.exists() {
            tokio::fs::remove_file(path).await?;
        }
    }
    Ok(())
}
