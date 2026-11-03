use crate::diarization_engine::model::{DiarizationResult, NemotronDiarizationModel};
use anyhow::{anyhow, Result};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

pub const NEMOTRON_MODEL_NAME: &str = "nemotron-3-diarization-v3";
pub const NEMOTRON_MODEL_FILENAME: &str = "nemotron3_diar_v3.onnx";
pub const NEMOTRON_EXPECTED_BYTES: u64 = 400_506_656; // ~382 MB
pub const NEMOTRON_DOWNLOAD_URL: &str =
    "https://huggingface.co/altunenes/parakeet-rs/resolve/main/nemotron-3-diarization/nemotron3_diar_v3.onnx";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StreamingPreset {
    Offline,
    LowLatency,
    VeryLowLatency,
    UltraLowLatency,
}

impl Default for StreamingPreset {
    fn default() -> Self {
        StreamingPreset::Offline
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationConfig {
    pub enabled: bool,
    pub preset: StreamingPreset,
    pub max_speakers: usize,
    pub threshold: f32,
}

impl Default for DiarizationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            preset: StreamingPreset::Offline,
            max_speakers: 4,
            threshold: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiarizationModelStatus {
    Available,
    Missing,
    Downloading { progress: u8 },
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub downloaded_mb: f64,
    pub total_mb: f64,
    pub speed_mbps: f64,
    pub percent: u8,
}

impl DownloadProgress {
    pub fn new(downloaded: u64, total: u64, speed_mbps: f64) -> Self {
        let percent = if total > 0 {
            ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u8
        } else {
            0
        };
        Self {
            downloaded_bytes: downloaded,
            total_bytes: total,
            downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
            total_mb: total as f64 / (1024.0 * 1024.0),
            speed_mbps,
            percent,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationModelInfo {
    pub name: String,
    pub filename: String,
    pub path: PathBuf,
    pub size_mb: u32,
    pub status: DiarizationModelStatus,
    pub description: String,
    pub architecture: String,
    pub max_speakers_supported: usize,
}

#[derive(thiserror::Error, Debug)]
pub enum DiarizationEngineError {
    #[error("Model not downloaded")]
    ModelNotDownloaded,
    #[error("Model not loaded")]
    ModelNotLoaded,
    #[error("Model is currently downloading")]
    ModelDownloading,
    #[error("Diarization disabled")]
    DiarizationDisabled,
    #[error("Inference failed: {0}")]
    InferenceFailed(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Download error: {0}")]
    Download(String),
}

struct ActiveDownload {
    cancellation: CancellationToken,
    completion: watch::Sender<bool>,
}

pub struct DiarizationEngine {
    models_dir: PathBuf,
    model: Arc<RwLock<Option<NemotronDiarizationModel>>>,
    config: Arc<RwLock<DiarizationConfig>>,
    active_download: Arc<Mutex<Option<ActiveDownload>>>,
    is_loading: Arc<AtomicBool>,
}

impl DiarizationEngine {
    pub fn new<P: AsRef<Path>>(models_dir: P) -> Self {
        let dir = models_dir.as_ref().join("diarization");
        let _ = std::fs::create_dir_all(&dir);

        Self {
            models_dir: dir,
            model: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(DiarizationConfig::default())),
            active_download: Arc::new(Mutex::new(None)),
            is_loading: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    pub fn model_file_path(&self) -> PathBuf {
        self.models_dir.join(NEMOTRON_MODEL_FILENAME)
    }

    pub async fn is_model_available(&self) -> bool {
        let path = self.model_file_path();
        if !path.exists() {
            return false;
        }

        if let Ok(metadata) = fs::metadata(&path).await {
            // Check that file size is reasonable (> 300MB)
            metadata.len() > 300_000_000
        } else {
            false
        }
    }

    pub async fn is_model_loaded(&self) -> bool {
        let guard = self.model.read().await;
        guard.is_some()
    }

    pub async fn get_config(&self) -> DiarizationConfig {
        let guard = self.config.read().await;
        guard.clone()
    }

    pub async fn update_config(&self, new_config: DiarizationConfig) {
        let mut guard = self.config.write().await;
        *guard = new_config;

        // Propagate to loaded model if present
        let mut model_guard = self.model.write().await;
        if let Some(ref mut model) = *model_guard {
            model.set_max_speakers(guard.max_speakers);
            model.set_threshold(guard.threshold);
        }
    }

    pub async fn get_model_info(&self) -> DiarizationModelInfo {
        let path = self.model_file_path();
        let exists = self.is_model_available().await;

        let is_downloading = {
            let guard = self.active_download.lock().await;
            guard.is_some()
        };

        let status = if is_downloading {
            DiarizationModelStatus::Downloading { progress: 0 }
        } else if exists {
            DiarizationModelStatus::Available
        } else {
            DiarizationModelStatus::Missing
        };

        DiarizationModelInfo {
            name: NEMOTRON_MODEL_NAME.to_string(),
            filename: NEMOTRON_MODEL_FILENAME.to_string(),
            path,
            size_mb: (NEMOTRON_EXPECTED_BYTES / (1024 * 1024)) as u32,
            status,
            description: "NVIDIA Nemotron-3 Diarization (Sortformer v3) - Real-time 8-speaker attribution running 100% on-device".to_string(),
            architecture: "31-layer RoPE Transformer (Sortformer v3)".to_string(),
            max_speakers_supported: 8,
        }
    }

    pub async fn load_model(&self) -> Result<()> {
        if self.is_model_loaded().await {
            return Ok(());
        }

        if !self.is_model_available().await {
            return Err(anyhow!(DiarizationEngineError::ModelNotDownloaded));
        }

        if self.is_loading.swap(true, Ordering::SeqCst) {
            info!("Nemotron-3 Diarization model is already loading");
            return Ok(());
        }

        let model_path = self.model_file_path();
        let config = self.get_config().await;

        info!(
            "Loading Nemotron-3 Diarization model from {:?}",
            model_path
        );

        let load_result = tokio::task::spawn_blocking(move || {
            NemotronDiarizationModel::new(&model_path, config.max_speakers, config.threshold)
        })
        .await?;

        self.is_loading.store(false, Ordering::SeqCst);

        match load_result {
            Ok(loaded_model) => {
                let mut guard = self.model.write().await;
                *guard = Some(loaded_model);
                info!("✅ Successfully loaded Nemotron-3 Diarization model into memory");
                Ok(())
            }
            Err(e) => {
                error!("❌ Failed to load Nemotron-3 Diarization model: {}", e);
                Err(anyhow!(e))
            }
        }
    }

    pub async fn unload_model(&self) {
        let mut guard = self.model.write().await;
        if guard.take().is_some() {
            info!("Unloaded Nemotron-3 Diarization model from memory");
        }
    }

    pub async fn delete_model(&self) -> Result<()> {
        self.unload_model().await;
        let path = self.model_file_path();
        if path.exists() {
            fs::remove_file(path).await?;
            info!("Deleted Nemotron-3 Diarization model file");
        }
        Ok(())
    }

    pub async fn cancel_download(&self) -> bool {
        let mut guard = self.active_download.lock().await;
        if let Some(download) = guard.take() {
            download.cancellation.cancel();
            info!("Cancelled Nemotron-3 Diarization model download");
            true
        } else {
            false
        }
    }

    pub async fn download_model<R: Runtime>(&self, app: AppHandle<R>) -> Result<()> {
        if self.is_model_available().await {
            info!("Nemotron-3 Diarization model already downloaded");
            return Ok(());
        }

        let cancellation = CancellationToken::new();
        let (completion_tx, _completion_rx) = watch::channel(false);

        {
            let mut guard = self.active_download.lock().await;
            if guard.is_some() {
                return Err(anyhow!(DiarizationEngineError::ModelDownloading));
            }
            *guard = Some(ActiveDownload {
                cancellation: cancellation.clone(),
                completion: completion_tx,
            });
        }

        let target_path = self.model_file_path();
        let temp_path = target_path.with_extension("download");
        let active_download_clone = self.active_download.clone();

        info!(
            "Starting Nemotron-3 Diarization download from {} to {:?}",
            NEMOTRON_DOWNLOAD_URL, target_path
        );

        let download_future = async move {
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| anyhow!("Failed to build HTTP client: {}", e))?;

            let response = client
                .get(NEMOTRON_DOWNLOAD_URL)
                .send()
                .await
                .map_err(|e| anyhow!("Download request failed: {}", e))?;

            if !response.status().is_success() {
                return Err(anyhow!("Server returned status: {}", response.status()));
            }

            let total_size = response
                .content_length()
                .unwrap_or(NEMOTRON_EXPECTED_BYTES);

            let mut file = fs::File::create(&temp_path).await?;
            let mut downloaded: u64 = 0;
            let start_time = Instant::now();
            let mut last_progress_emit = Instant::now();
            let mut stream = response.bytes_stream();

            use futures_util::StreamExt;
            while let Some(chunk_result) = stream.next().await {
                if cancellation.is_cancelled() {
                    let _ = fs::remove_file(&temp_path).await;
                    return Err(anyhow!("Download cancelled by user"));
                }

                let chunk = chunk_result.map_err(|e| anyhow!("Error streaming bytes: {}", e))?;
                file.write_all(&chunk).await?;
                downloaded += chunk.len() as u64;

                if last_progress_emit.elapsed() >= Duration::from_millis(150) || downloaded == total_size {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let speed_mbps = if elapsed > 0.0 {
                        (downloaded as f64 / (1024.0 * 1024.0)) / elapsed
                    } else {
                        0.0
                    };

                    let progress = DownloadProgress::new(downloaded, total_size, speed_mbps);
                    let _ = app.emit("diarization-download-progress", &progress);
                    last_progress_emit = Instant::now();
                }
            }

            file.flush().await?;
            drop(file);

            // Rename temp to target
            fs::rename(&temp_path, &target_path).await?;

            info!(
                "✅ Nemotron-3 Diarization model download completed: {} MB",
                downloaded / (1024 * 1024)
            );

            let _ = app.emit("diarization-download-complete", serde_json::json!({
                "modelName": NEMOTRON_MODEL_NAME,
                "filePath": target_path.to_string_lossy(),
            }));

            Ok(())
        };

        let result = download_future.await;

        // Clear active download slot
        {
            let mut guard = active_download_clone.lock().await;
            *guard = None;
        }

        result
    }

    pub async fn reset_streaming_state(&self) {
        let mut guard = self.model.write().await;
        if let Some(ref mut model) = *guard {
            model.reset_streaming_state();
        }
    }

    /// Run full diarization on 16kHz audio samples.
    pub async fn diarize_audio(
        &self,
        samples: &[f32],
        sample_rate: u32,
    ) -> Result<DiarizationResult, DiarizationEngineError> {
        let config = self.get_config().await;
        if !config.enabled {
            return Err(DiarizationEngineError::DiarizationDisabled);
        }

        // Auto-load if available and not yet loaded
        if !self.is_model_loaded().await {
            if self.is_model_available().await {
                if let Err(e) = self.load_model().await {
                    return Err(DiarizationEngineError::InferenceFailed(e.to_string()));
                }
            } else {
                return Err(DiarizationEngineError::ModelNotDownloaded);
            }
        }

        let mut guard = self.model.write().await;
        if let Some(ref mut model) = *guard {
            model
                .diarize(samples, sample_rate)
                .map_err(|e| DiarizationEngineError::InferenceFailed(e.to_string()))
        } else {
            Err(DiarizationEngineError::ModelNotLoaded)
        }
    }

    /// Identify the primary active speaker in an audio segment.
    pub async fn identify_speaker(
        &self,
        samples: &[f32],
        sample_rate: u32,
    ) -> Option<String> {
        match self.diarize_audio(samples, sample_rate).await {
            Ok(result) => {
                log::info!(
                    "🎤 Speaker identification result: primary={:?}, total_speakers={}",
                    result.primary_speaker,
                    result.num_speakers
                );
                result.primary_speaker
            }
            Err(e) => {
                log::warn!("⚠️ Speaker identification skipped/failed: {}", e);
                None
            }
        }
    }
}
