use crate::parakeet_engine::model::ParakeetModel;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Quantization type for Parakeet models
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QuantizationType {
    FP32,   // Full precision
    Int8,   // 8-bit integer quantization (faster)
}

impl Default for QuantizationType {
    fn default() -> Self {
        QuantizationType::Int8 // Default to int8 for best performance
    }
}

/// Model status for Parakeet models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelStatus {
    Available,
    Missing,
    Downloading { progress: u8 },
    Error(String),
    Corrupted { file_size: u64, expected_min_size: u64 },
}

/// Detailed download progress info (MB-based with speed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
    /// Total file size in bytes
    pub total_bytes: u64,
    /// Downloaded in MB (for display)
    pub downloaded_mb: f64,
    /// Total size in MB (for display)
    pub total_mb: f64,
    /// Download speed in MB/s
    pub speed_mbps: f64,
    /// Percentage complete (0-100)
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

/// Information about a Parakeet model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_mb: u32,
    pub quantization: QuantizationType,
    pub speed: String,     // Performance description
    pub status: ModelStatus,
    pub description: String,
}

#[derive(Debug)]
pub enum ParakeetEngineError {
    ModelNotLoaded,
    ModelNotFound(String),
    TranscriptionFailed(String),
    DownloadFailed(String),
    IoError(std::io::Error),
    Other(String),
}

impl std::fmt::Display for ParakeetEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParakeetEngineError::ModelNotLoaded => write!(f, "No Parakeet model loaded"),
            ParakeetEngineError::ModelNotFound(name) => write!(f, "Model '{}' not found", name),
            ParakeetEngineError::TranscriptionFailed(err) => write!(f, "Transcription failed: {}", err),
            ParakeetEngineError::DownloadFailed(err) => write!(f, "Download failed: {}", err),
            ParakeetEngineError::IoError(err) => write!(f, "IO error: {}", err),
            ParakeetEngineError::Other(err) => write!(f, "Error: {}", err),
        }
    }
}

impl std::error::Error for ParakeetEngineError {}

impl From<std::io::Error> for ParakeetEngineError {
    fn from(err: std::io::Error) -> Self {
        ParakeetEngineError::IoError(err)
    }
}

pub struct ParakeetEngine {
    models_dir: PathBuf,
    current_model: Arc<RwLock<Option<ParakeetModel>>>,
    current_model_name: Arc<RwLock<Option<String>>>,
    pub(crate) available_models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    cancel_download_flag: Arc<RwLock<Option<String>>>, // Model name being cancelled
    // Active downloads tracking to prevent concurrent downloads
    pub(crate) active_downloads: Arc<RwLock<HashSet<String>>>, // Set of models currently being downloaded
}

impl ParakeetEngine {
    /// Create a new Parakeet engine with optional custom models directory
    pub fn new_with_models_dir(models_dir: Option<PathBuf>) -> Result<Self> {
        let models_dir = if let Some(dir) = models_dir {
            dir.join("parakeet") // Parakeet models in subdirectory
        } else {
            // Fallback to default location
            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;

            if cfg!(debug_assertions) {
                // Development mode
                current_dir.join("models").join("parakeet")
            } else {
                // Production mode
                dirs::data_dir()
                    .or_else(|| dirs::home_dir())
                    .ok_or_else(|| anyhow!("Could not find system data directory"))?
                    .join("Meetily")
                    .join("models")
                    .join("parakeet")
            }
        };

        log::info!("ParakeetEngine using models directory: {}", models_dir.display());

        // Create directory if it doesn't exist
        if !models_dir.exists() {
            std::fs::create_dir_all(&models_dir)?;
        }

        Ok(Self {
            models_dir,
            current_model: Arc::new(RwLock::new(None)),
            current_model_name: Arc::new(RwLock::new(None)),
            available_models: Arc::new(RwLock::new(HashMap::new())),
            cancel_download_flag: Arc::new(RwLock::new(None)),
            // Initialize active downloads tracking
            active_downloads: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    /// Discover available Parakeet models
    pub async fn discover_models(&self) -> Result<Vec<ModelInfo>> {
        let models_dir = &self.models_dir;
        let mut models = Vec::new();

        // Parakeet model configurations
        // Model name format: parakeet-tdt-0.6b-v{version}-{quantization}
        // Sizes match actual download sizes (encoder + decoder + preprocessor + vocab)
        let model_configs = [
            ("parakeet-tdt-0.6b-v3-int8", 670, QuantizationType::Int8, "Ultra Fast (v3)", "Real time on M4 Max, latest version with int8 quantization"),
            ("parakeet-tdt-0.6b-v2-int8", 661, QuantizationType::Int8, "Fast (v2)", "Previous version with int8 quantization, good balance of speed and accuracy"),
        ];

        // Get active downloads to override status
        let active_downloads = self.active_downloads.read().await;

        for (name, size_mb, quantization, speed, description) in model_configs {
            let model_path = models_dir.join(name);

            // Check if model is currently downloading
            let status = if active_downloads.contains(name) {
                // If downloading, preserve that status regardless of file system
                // We don't know the exact progress here without more state, but 0 is safe fallback
                // The progress events will update the UI
                ModelStatus::Downloading { progress: 0 }
            } else if model_path.exists() {
                // Check for required ONNX files
                let required_files = match quantization {
                    QuantizationType::Int8 => vec![
                        "encoder-model.int8.onnx",
                        "decoder_joint-model.int8.onnx",
                        "nemo128.onnx",
                        "vocab.txt",
                    ],
                    QuantizationType::FP32 => vec![
                        "encoder-model.onnx",
                        "decoder_joint-model.onnx",
                        "nemo128.onnx",
                        "vocab.txt",
                    ],
                };

                let all_files_exist = required_files.iter().all(|file| {
                    model_path.join(file).exists()
                });

                if all_files_exist {
                    // Validate model by checking file sizes
                    match self.validate_model_directory(&model_path).await {
                        Ok(_) => ModelStatus::Available,
                        Err(_) => {
                            log::warn!("Model directory {} appears corrupted", name);
                            // Calculate total size of existing files
                            let mut total_size = 0u64;
                            for file in required_files {
                                if let Ok(metadata) = std::fs::metadata(model_path.join(file)) {
                                    total_size += metadata.len();
                                }
                            }
                            ModelStatus::Corrupted {
                                file_size: total_size,
                                expected_min_size: (size_mb as u64) * 1024 * 1024,
                            }
                        }
                    }
                } else {
                    ModelStatus::Missing
                }
            } else {
                ModelStatus::Missing
            };

            let model_info = ModelInfo {
                name: name.to_string(),
                path: model_path,
                size_mb: size_mb as u32,
                quantization: quantization.clone(),
                speed: speed.to_string(),
                status,
                description: description.to_string(),
            };

            models.push(model_info);
        }

        // Update internal cache
        let mut available_models = self.available_models.write().await;
        available_models.clear();
        for model in &models {
            available_models.insert(model.name.clone(), model.clone());
        }

        Ok(models)
    }

    /// Validate model directory by checking if all required files exist AND have valid sizes
    async fn validate_model_directory(&self, model_dir: &PathBuf) -> Result<()> {
        // Check if vocab.txt exists and is readable
        let vocab_path = model_dir.join("vocab.txt");
        if !vocab_path.exists() {
            return Err(anyhow!("vocab.txt not found"));
        }

        // Determine which files to check based on what exists
        let is_int8 = model_dir.join("encoder-model.int8.onnx").exists();
        let is_fp32 = model_dir.join("encoder-model.onnx").exists();

        if !is_int8 && !is_fp32 {
            return Err(anyhow!("No ONNX model files found"));
        }

        // Check preprocessor
        if !model_dir.join("nemo128.onnx").exists() {
            return Err(anyhow!("Preprocessor (nemo128.onnx) not found"));
        }

        // Define minimum file sizes (90% of expected to allow some variance)
        // These are critical to catch partial downloads that would crash on load
        let expected_sizes: Vec<(&str, u64)> = if is_int8 {
            vec![
                ("encoder-model.int8.onnx", 580_000_000),    // ~652 MB, min 580 MB (89%)
                ("decoder_joint-model.int8.onnx", 8_000_000), // ~18 MB, min 8 MB
                ("nemo128.onnx", 100_000),                    // ~140 KB, min 100 KB
                ("vocab.txt", 5_000),                         // ~94 KB, min 5 KB
            ]
        } else {
            vec![
                ("encoder-model.onnx", 2_200_000_000),        // ~2.44 GB, min 2.2 GB
                ("decoder_joint-model.onnx", 65_000_000),     // ~72 MB, min 65 MB
                ("nemo128.onnx", 100_000),                    // ~140 KB, min 100 KB
                ("vocab.txt", 5_000),                         // ~94 KB, min 5 KB
            ]
        };

        // Validate each file exists AND has sufficient size
        for (filename, min_size) in expected_sizes {
            let file_path = model_dir.join(filename);
            if !file_path.exists() {
                return Err(anyhow!("{} not found", filename));
            }

            match std::fs::metadata(&file_path) {
                Ok(metadata) => {
                    let actual_size = metadata.len();
                    if actual_size < min_size {
                        return Err(anyhow!(
                            "{} is incomplete: {} bytes (expected at least {} bytes)",
                            filename,
                            actual_size,
                            min_size
                        ));
                    }
                }
                Err(e) => {
                    return Err(anyhow!("Failed to read {} metadata: {}", filename, e));
                }
            }
        }

        Ok(())
    }

    /// Clean incomplete model directory before download
    /// Removes all files if directory exists but model is not Available
    async fn clean_incomplete_model_directory(&self, model_dir: &PathBuf) -> Result<()> {
        if !model_dir.exists() {
            return Ok(()); // Nothing to clean
        }

        // Validate the directory
        match self.validate_model_directory(model_dir).await {
            Ok(_) => {
                log::info!("Model directory is valid, no cleanup needed");
                return Ok(());
            }
            Err(validation_error) => {
                log::warn!(
                    "Model directory exists but is invalid: {}. Cleaning up...",
                    validation_error
                );

                // List and remove all files in the directory
                let mut entries = fs::read_dir(model_dir).await
                    .map_err(|e| anyhow!("Failed to read model directory: {}", e))?;

                let mut removed_count = 0;
                while let Some(entry) = entries.next_entry().await
                    .map_err(|e| anyhow!("Failed to read directory entry: {}", e))?
                {
                    let path = entry.path();
                    if path.is_file() {
                        match fs::remove_file(&path).await {
                            Ok(_) => {
                                log::info!("Removed incomplete file: {:?}", path.file_name());
                                removed_count += 1;
                            }
                            Err(e) => {
                                log::warn!("Failed to remove file {:?}: {}", path, e);
                            }
                        }
                    }
                }

                log::info!("Cleaned {} incomplete files from model directory", removed_count);
                Ok(())
            }
        }
    }

    /// Load a Parakeet model
    pub async fn load_model(&self, model_name: &str) -> Result<()> {
        let models = self.available_models.read().await;
        let model_info = models
            .get(model_name)
            .ok_or_else(|| anyhow!("Model {} not found", model_name))?;

        match model_info.status {
            ModelStatus::Available => {
                // Check if this model is already loaded
                if let Some(current_model) = self.current_model_name.read().await.as_ref() {
                    if current_model == model_name {
                        log::info!("Parakeet model {} is already loaded, skipping reload", model_name);
                        return Ok(());
                    }

                    // Unload current model before loading new one
                    log::info!("Unloading current Parakeet model '{}' before loading '{}'", current_model, model_name);
                    self.unload_model().await;
                }

                log::info!("Loading Parakeet model: {}", model_name);

                // Load model based on quantization type
                let quantized = model_info.quantization == QuantizationType::Int8;
                let model = ParakeetModel::new(&model_info.path, quantized)
                    .map_err(|e| anyhow!("Failed to load Parakeet model {}: {}", model_name, e))?;

                // Update current model and model name
                *self.current_model.write().await = Some(model);
                *self.current_model_name.write().await = Some(model_name.to_string());

                log::info!(
                    "Successfully loaded Parakeet model: {} ({})",
                    model_name,
                    if quantized { "Int8 quantized" } else { "FP32" }
                );
                Ok(())
            }
            ModelStatus::Missing => {
                Err(anyhow!("Parakeet model {} is not downloaded", model_name))
            }
            ModelStatus::Downloading { .. } => {
                Err(anyhow!("Parakeet model {} is currently downloading", model_name))
            }
            ModelStatus::Error(ref err) => {
                Err(anyhow!("Parakeet model {} has error: {}", model_name, err))
            }
            ModelStatus::Corrupted { .. } => {
                Err(anyhow!("Parakeet model {} is corrupted and cannot be loaded", model_name))
            }
        }
    }

    /// Unload the current model
    pub async fn unload_model(&self) -> bool {
        let mut model_guard = self.current_model.write().await;
        let unloaded = model_guard.take().is_some();
        if unloaded {
            log::info!("Parakeet model unloaded");
        }

        let mut model_name_guard = self.current_model_name.write().await;
        model_name_guard.take();

        unloaded
    }

    /// Get the currently loaded model name
    pub async fn get_current_model(&self) -> Option<String> {
        self.current_model_name.read().await.clone()
    }

    /// Check if a model is loaded
    pub async fn is_model_loaded(&self) -> bool {
        self.current_model.read().await.is_some()
    }

    /// Transcribe audio samples using the loaded Parakeet model
    pub async fn transcribe_audio(&self, audio_data: Vec<f32>) -> Result<String> {
        let mut model_guard = self.current_model.write().await;
        let model = model_guard
            .as_mut()
            .ok_or_else(|| anyhow!("No Parakeet model loaded. Please load a model first."))?;

        let duration_seconds = audio_data.len() as f64 / 16000.0; // Assuming 16kHz
        log::debug!(
            "Parakeet transcribing {} samples ({:.1}s duration)",
            audio_data.len(),
            duration_seconds
        );

        // Transcribe using Parakeet model
        let result = model
            .transcribe_samples(audio_data)
            .map_err(|e| anyhow!("Parakeet transcription failed: {}", e))?;

        log::debug!("Parakeet transcription result: '{}'", result.text);

        Ok(result.text)
    }

    /// Get the models directory path
    pub async fn get_models_directory(&self) -> PathBuf {
        self.models_dir.clone()
    }

    /// Delete a corrupted model
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete Parakeet model: {}", model_name);

        // Get model info to find the directory path
        let model_info = {
            let models = self.available_models.read().await;
            models.get(model_name).cloned()
        };

        let model_info = model_info.ok_or_else(|| anyhow!("Parakeet model '{}' not found", model_name))?;

        log::info!("Parakeet model '{}' has status: {:?}", model_name, model_info.status);

        // Allow deletion of corrupted or available models
        match &model_info.status {
            ModelStatus::Corrupted { .. } | ModelStatus::Available => {
                // Delete the entire model directory
                if model_info.path.exists() {
                    fs::remove_dir_all(&model_info.path).await
                        .map_err(|e| anyhow!("Failed to delete directory '{}': {}", model_info.path.display(), e))?;
                    log::info!("Successfully deleted Parakeet model directory: {}", model_info.path.display());
                } else {
                    log::warn!("Directory '{}' does not exist, nothing to delete", model_info.path.display());
                }

                // Update model status to Missing
                {
                    let mut models = self.available_models.write().await;
                    if let Some(model) = models.get_mut(model_name) {
                        model.status = ModelStatus::Missing;
                    }
                }

                Ok(format!("Successfully deleted Parakeet model '{}'", model_name))
            }
            _ => {
                Err(anyhow!(
                    "Can only delete corrupted or available Parakeet models. Model '{}' has status: {:?}",
                    model_name,
                    model_info.status
                ))
            }
        }
    }

    /// Download a Parakeet model from HuggingFace (backward-compatible wrapper)
    pub async fn download_model(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        // Wrap simple callback to use detailed version
        let detailed_callback: Option<Box<dyn Fn(DownloadProgress) + Send>> =
            progress_callback.map(|cb| {
                Box::new(move |p: DownloadProgress| cb(p.percent)) as Box<dyn Fn(DownloadProgress) + Send>
            });
        self.download_model_detailed(model_name, detailed_callback).await
    }

    /// Download a Parakeet model with detailed progress (MB/speed/resume support)
    pub async fn download_model_detailed(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        log::info!("Starting download for Parakeet model: {}", model_name);

        // Check if download is already in progress for this model
        {
            let active = self.active_downloads.read().await;
            if active.contains(model_name) {
                log::warn!("Download already in progress for Parakeet model: {}", model_name);
                return Err(anyhow!("Download already in progress for model: {}", model_name));
            }
        }

        // Add to active downloads
        {
            let mut active = self.active_downloads.write().await;
            active.insert(model_name.to_string());
        }

        // Clear any previous cancellation flag for this model
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            *cancel_flag = None;
        }

        // Get model info
        let model_info = {
            let models = self.available_models.read().await;
            match models.get(model_name).cloned() {
                Some(info) => info,
                None => {
                    // Remove from active downloads on error
                    let mut active = self.active_downloads.write().await;
                    active.remove(model_name);
                    return Err(anyhow!("Model {} not found", model_name));
                }
            }
        };

        // Update model status to downloading
        {
            let mut models = self.available_models.write().await;
            if let Some(model) = models.get_mut(model_name) {
                model.status = ModelStatus::Downloading { progress: 0 };
            }
        }

        // Base URLs for Parakeet models (primary + fallbacks, version-specific)
        let base_urls: Vec<&str> = if model_name.contains("-v2-") {
            vec![
                "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/main",
            ]
        } else {
            // v3 models: try custom server first, fall back to HuggingFace
            vec![
                "https://meetily.towardsgeneralintelligence.com/models/parakeet-tdt-0.6b-v3-onnx",
                "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main",
            ]
        };

        // Determine which files to download based on quantization
        let files_to_download = match model_info.quantization {
            QuantizationType::Int8 => vec![
                "encoder-model.int8.onnx",
                "decoder_joint-model.int8.onnx",
                "nemo128.onnx",
                "vocab.txt",
            ],
            QuantizationType::FP32 => vec![
                "encoder-model.onnx",
                "decoder_joint-model.onnx",
                "nemo128.onnx",
                "vocab.txt",
            ],
        };

        // Create model directory
        let model_dir = &model_info.path;
        if !model_dir.exists() {
            if let Err(e) = fs::create_dir_all(model_dir).await {
                // Remove from active downloads on error
                let mut active = self.active_downloads.write().await;
                active.remove(model_name);
                return Err(anyhow!("Failed to create model directory: {}", e));
            }
        }

        // Clean up incomplete downloads before starting
        log::info!("Checking for incomplete model files to clean up...");
        if let Err(e) = self.clean_incomplete_model_directory(model_dir).await {
            log::warn!("Failed to clean incomplete model directory: {}", e);
            // Continue anyway - we'll handle errors during download
        }

        // Optimized HTTP client for large file downloads
        let client = reqwest::Client::builder()
            .tcp_nodelay(true)              // Disable Nagle's algorithm for better streaming
            .pool_max_idle_per_host(1)      // Keep connection alive
            .timeout(Duration::from_secs(3600))  // 1 hour timeout for large files
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        let total_files = files_to_download.len();

        // Calculate total download size for weighted progress
        // Note: These are approximate sizes based on HuggingFace repo inspection
        let file_sizes: std::collections::HashMap<&str, u64> = match model_info.quantization {
            QuantizationType::Int8 => {
                if model_name.contains("-v2-") {
                    // V2 model sizes
                    [
                        ("encoder-model.int8.onnx", 652_000_000u64),       // 652 MB
                        ("decoder_joint-model.int8.onnx", 9_000_000u64),   // 9 MB
                        ("nemo128.onnx", 140_000u64),                      // 140 KB
                        ("vocab.txt", 9_380u64),                           // 9.38 KB
                    ].iter().cloned().collect()
                } else {
                    // V3 model sizes (default)
                    [
                        ("encoder-model.int8.onnx", 652_000_000u64),       // 652 MB
                        ("decoder_joint-model.int8.onnx", 18_200_000u64),  // 18.2 MB
                        ("nemo128.onnx", 140_000u64),                      // 140 KB
                        ("vocab.txt", 93_900u64),                          // 93.9 KB
                    ].iter().cloned().collect()
                }
            }
            QuantizationType::FP32 => {
                // FP32 model sizes (encoder has .onnx + .onnx.data)
                [
                    ("encoder-model.onnx", 41_800_000u64 + 2_440_000_000u64), // 41.8 MB + 2.44 GB
                    ("decoder_joint-model.onnx", 72_500_000u64),               // 72.5 MB
                    ("nemo128.onnx", 140_000u64),                              // 140 KB
                    ("vocab.txt", 93_900u64),                                  // 93.9 KB
                ].iter().cloned().collect()
            }
        };

        // Calculate total expected download size
        let total_size_bytes: u64 = files_to_download.iter()
            .filter_map(|f| file_sizes.get(*f))
            .copied()
            .sum();

        // Check for existing downloads (complete or partial) to calculate resume offset
        let mut already_downloaded: u64 = 0;
        for filename in &files_to_download {
            let file_path = model_dir.join(filename);
            if file_path.exists() {
                if let Ok(metadata) = fs::metadata(&file_path).await {
                    let file_size = metadata.len();
                    let expected_size = file_sizes.get(*filename).copied().unwrap_or(0);
                    // Count all existing bytes (complete files capped at expected size, partial as-is)
                    // This ensures progress starts from where we left off
                    already_downloaded += file_size.min(expected_size);
                }
            }
        }

        let mut total_downloaded: u64 = already_downloaded;

        // Timing for speed calculation
        let download_start_time = Instant::now();
        let mut last_report_time = Instant::now();
        let mut bytes_since_last_report: u64 = 0;
        let mut last_reported_progress: u8 = 0;

        log::info!(
            "Starting weighted download for {} files, total size: {:.2} MB (already downloaded: {:.2} MB)",
            total_files,
            total_size_bytes as f64 / 1_048_576.0,
            already_downloaded as f64 / 1_048_576.0
        );

        for (index, filename) in files_to_download.iter().enumerate() {
            let file_path = model_dir.join(filename);

            // Check for existing partial file to resume
            let existing_size: u64 = if file_path.exists() {
                fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };

            let expected_size = file_sizes.get(*filename).copied().unwrap_or(0);

            // Skip if file is already complete (with 1% tolerance for size variations)
            let size_tolerance = (expected_size as f64 * 0.99) as u64;
            if existing_size >= size_tolerance && expected_size > 0 {
                log::info!(
                    "Skipping complete file: {} ({:.2} MB, expected: {:.2} MB)",
                    filename,
                    existing_size as f64 / 1_048_576.0,
                    expected_size as f64 / 1_048_576.0
                );
                continue;
            }

            log::info!("Downloading file {}/{}: {} (resuming from {} bytes)", index + 1, total_files, filename, existing_size);

            // Try each URL (primary + fallbacks), including mid-stream failures
            let mut last_error = String::new();
            let mut file_downloaded_success = false;

            'url_loop: for (url_index, url_base) in base_urls.iter().enumerate() {
                let file_url = format!("{}/{}", url_base, filename);
                let is_fallback = url_index > 0;
                if is_fallback {
                    log::warn!(
                        "URL failed for {} ({}), trying fallback {}/{}: {}",
                        filename, last_error, url_index, base_urls.len() - 1, file_url
                    );
                    // Emit fallback event so UI can inform the user
                    if let Some(ref callback) = progress_callback {
                        callback(DownloadProgress::new(total_downloaded, total_size_bytes, 0.0));
                    }
                }

                // Re-check existing file size (may have partial data from a previous URL attempt)
                let current_existing_size: u64 = if file_path.exists() {
                    fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                };

                // Build request with optional Range header for resume
                let mut request = client.get(&file_url);
                if current_existing_size > 0 {
                    request = request.header("Range", format!("bytes={}-", current_existing_size));
                    log::info!("Resuming download of {} from byte {}", filename, current_existing_size);
                }

                // Connect
                let connect_result = request.send().await;
                let resp = match connect_result {
                    Ok(resp) => resp,
                    Err(e) => {
                        last_error = format!("Connection failed: {}", e);
                        log::warn!("URL {} connection failed for {}: {}", file_url, filename, e);
                        continue 'url_loop;
                    }
                };

                // Handle response status, potentially replacing resp on 416 retry
                let mut resp = resp;
                let (file_total_size, resuming) = if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                    let remaining = resp.content_length().unwrap_or(0);
                    log::info!("Server supports resume, remaining: {} bytes", remaining);
                    (current_existing_size + remaining, true)
                } else if resp.status().is_success() {
                    if current_existing_size > 0 {
                        log::warn!("Server doesn't support resume for {}, starting fresh download", filename);
                    }
                    (resp.content_length().unwrap_or(0), false)
                } else if resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                    let size_tolerance = (expected_size as f64 * 0.99) as u64;
                    if current_existing_size >= size_tolerance && expected_size > 0 {
                        log::info!("File {} already complete ({} bytes). Skipping.", filename, current_existing_size);
                        file_downloaded_success = true;
                        break 'url_loop;
                    } else {
                        // Delete incomplete file and retry fresh on this URL
                        log::warn!("File {} incomplete ({}/{} bytes), deleting for fresh download", filename, current_existing_size, expected_size);
                        let _ = fs::remove_file(&file_path).await;
                        match client.get(&file_url).send().await {
                            Ok(retry_resp) if retry_resp.status().is_success() => {
                                let size = retry_resp.content_length().unwrap_or(0);
                                resp = retry_resp;
                                (size, false)
                            }
                            Ok(retry_resp) => {
                                last_error = format!("HTTP {} on fresh retry", retry_resp.status());
                                continue 'url_loop;
                            }
                            Err(e) => {
                                last_error = format!("Fresh retry failed: {}", e);
                                continue 'url_loop;
                            }
                        }
                    }
                } else {
                    last_error = format!("HTTP {}", resp.status());
                    log::warn!("URL {} returned {} for {}", file_url, resp.status(), filename);
                    continue 'url_loop;
                };

                if is_fallback {
                    log::info!("Fallback URL connected for {}: {}", filename, file_url);
                }

                // Open file for writing (append if resuming, create new if not)
                let file = if resuming {
                    match fs::OpenOptions::new().append(true).open(&file_path).await {
                        Ok(f) => f,
                        Err(e) => {
                            last_error = format!("Failed to open file for resume: {}", e);
                            continue 'url_loop;
                        }
                    }
                } else {
                    match fs::File::create(&file_path).await {
                        Ok(f) => f,
                        Err(e) => {
                            // File creation failure is not URL-specific, don't retry
                            let mut active = self.active_downloads.write().await;
                            active.remove(model_name);
                            return Err(anyhow!("Failed to create file {}: {}", filename, e));
                        }
                    }
                };

                let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file);
                let mut file_downloaded = if resuming { current_existing_size } else { 0u64 };

                // Stream download
                use futures_util::StreamExt;
                let mut stream = resp.bytes_stream();

                let stream_ok = loop {
                    // Check for cancellation
                    {
                        let cancel_flag = self.cancel_download_flag.read().await;
                        if cancel_flag.as_ref() == Some(&model_name.to_string()) {
                            log::info!("Download cancelled for {}", model_name);
                            let _ = writer.flush().await;
                            drop(writer);
                            let mut active = self.active_downloads.write().await;
                            active.remove(model_name);
                            return Err(anyhow!("Download cancelled by user"));
                        }
                    }

                    let next_result = timeout(Duration::from_secs(30), stream.next()).await;

                    match next_result {
                        Err(_) => {
                            // Timeout - flush partial data and try next URL
                            log::warn!("Stream timeout for {} on {}, will try next URL", filename, file_url);
                            let _ = writer.flush().await;
                            drop(writer);
                            last_error = "Stream stalled - no data for 30 seconds".to_string();
                            break false;
                        }
                        Ok(None) => {
                            // Stream complete
                            break true;
                        }
                        Ok(Some(Ok(chunk))) => {
                            let chunk_len = chunk.len() as u64;
                            if let Err(e) = writer.write_all(&chunk).await {
                                let mut active = self.active_downloads.write().await;
                                active.remove(model_name);
                                return Err(anyhow!("Failed to write to file {}: {}", filename, e));
                            }
                            file_downloaded += chunk_len;
                            total_downloaded += chunk_len;
                            bytes_since_last_report += chunk_len;

                            // Progress reporting
                            let overall_progress = if total_size_bytes > 0 {
                                ((total_downloaded as f64 / total_size_bytes as f64) * 100.0).min(99.0) as u8
                            } else {
                                ((index as f64 + (file_downloaded as f64 / file_total_size.max(1) as f64)) / total_files as f64 * 100.0) as u8
                            };

                            let elapsed_since_report = last_report_time.elapsed();
                            let progress_changed = overall_progress > last_reported_progress;
                            let time_threshold = elapsed_since_report >= Duration::from_millis(500);
                            let is_complete = file_downloaded >= file_total_size;

                            if progress_changed || time_threshold || is_complete {
                                let speed_mbps = if elapsed_since_report.as_secs_f64() >= 0.1 {
                                    (bytes_since_last_report as f64 / (1024.0 * 1024.0)) / elapsed_since_report.as_secs_f64()
                                } else {
                                    let total_elapsed = download_start_time.elapsed().as_secs_f64();
                                    if total_elapsed > 0.0 {
                                        ((total_downloaded - already_downloaded) as f64 / (1024.0 * 1024.0)) / total_elapsed
                                    } else {
                                        0.0
                                    }
                                };

                                last_reported_progress = overall_progress;
                                last_report_time = Instant::now();
                                bytes_since_last_report = 0;

                                let progress = DownloadProgress::new(total_downloaded, total_size_bytes, speed_mbps);
                                if let Some(ref callback) = progress_callback {
                                    callback(progress);
                                }

                                {
                                    let mut models = self.available_models.write().await;
                                    if let Some(model) = models.get_mut(model_name) {
                                        model.status = ModelStatus::Downloading { progress: overall_progress };
                                    }
                                }
                            }
                        }
                        Ok(Some(Err(e))) => {
                            // Stream error - flush partial data and try next URL
                            log::warn!("Stream error for {} on {}: {}", filename, file_url, e);
                            let _ = writer.flush().await;
                            drop(writer);
                            last_error = format!("Stream error: {}", e);
                            break false;
                        }
                    }
                };

                if stream_ok {
                    // Flush and confirm success
                    if let Err(e) = writer.flush().await {
                        let mut active = self.active_downloads.write().await;
                        active.remove(model_name);
                        return Err(anyhow!("Failed to flush file {}: {}", filename, e));
                    }
                    file_downloaded_success = true;
                    break 'url_loop;
                }
                // If !stream_ok, the loop continues to the next URL
            }

            if !file_downloaded_success {
                // All URLs exhausted for this file
                let mut active = self.active_downloads.write().await;
                active.remove(model_name);

                {
                    let mut models = self.available_models.write().await;
                    if let Some(model) = models.get_mut(model_name) {
                        model.status = ModelStatus::Missing;
                    }
                }

                return Err(anyhow!(
                    "Download failed for {} - all download servers failed (tried {} servers). \
                    Last error: {}. This may be a server issue; please try again later.",
                    filename, base_urls.len(), last_error
                ));
            }

            log::info!(
                "Completed download: {} (overall progress: {:.1}%)",
                filename,
                (total_downloaded as f64 / total_size_bytes as f64) * 100.0
            );
        }

        // Report 100% progress with final speed
        let total_elapsed = download_start_time.elapsed().as_secs_f64();
        let final_speed = if total_elapsed > 0.0 {
            ((total_downloaded - already_downloaded) as f64 / (1024.0 * 1024.0)) / total_elapsed
        } else {
            0.0
        };
        let final_progress = DownloadProgress::new(total_size_bytes, total_size_bytes, final_speed);
        if let Some(ref callback) = progress_callback {
            callback(final_progress);
        }

        // Update model status to available
        {
            let mut models = self.available_models.write().await;
            if let Some(model) = models.get_mut(model_name) {
                model.status = ModelStatus::Available;
                model.path = model_dir.clone();
            }
        }

        // Remove from active downloads on completion
        {
            let mut active = self.active_downloads.write().await;
            active.remove(model_name);
        }

        // Clear cancellation flag on successful completion
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            if cancel_flag.as_ref() == Some(&model_name.to_string()) {
                *cancel_flag = None;
            }
        }

        log::info!("Download completed for Parakeet model: {}", model_name);
        Ok(())
    }

    /// Cancel an ongoing model download
    pub async fn cancel_download(&self, model_name: &str) -> Result<()> {
        log::info!("Cancelling download for Parakeet model: {}", model_name);

        // Set cancellation flag to interrupt the download loop
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            *cancel_flag = Some(model_name.to_string());
        }

        // Remove from active downloads
        {
            let mut active = self.active_downloads.write().await;
            active.remove(model_name);
        }

        // Update model status to Missing (so it can be retried)
        {
            let mut models = self.available_models.write().await;
            if let Some(model) = models.get_mut(model_name) {
                model.status = ModelStatus::Missing;
            }
        }

        // Clean up partially downloaded files
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await; // Brief delay to let download loop exit

        let model_path = self.models_dir.join(model_name);
        if model_path.exists() {
            if let Err(e) = fs::remove_dir_all(&model_path).await {
                log::warn!("Failed to clean up cancelled download directory: {}", e);
            } else {
                log::info!("Cleaned up cancelled download directory: {}", model_path.display());
            }
        }

        Ok(())
    }
}
