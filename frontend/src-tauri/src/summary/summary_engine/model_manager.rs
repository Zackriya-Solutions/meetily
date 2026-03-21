// Model manager for built-in AI models - handles downloads and lifecycle
// Follows the same pattern as whisper_engine/whisper_engine.rs for consistency

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::RwLock;
use tokio::time::timeout;

use super::models::{get_available_models, get_model_by_name};

// ============================================================================
// Model Status Types
// ============================================================================

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
            ((downloaded as f64 / total as f64) * 100.0) as u8
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

/// Model status in the system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelStatus {
    /// Model is not yet downloaded
    NotDownloaded,

    /// Model is currently being downloaded (progress 0-100)
    Downloading { progress: u8 },

    /// Model is downloaded and ready to use
    Available,

    /// Model file is corrupted and needs redownload
    Corrupted { file_size: u64, expected_min_size: u64 },

    /// Error occurred with the model
    Error(String),
}

/// Model information for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model name (e.g., "gemma3:1b")
    pub name: String,

    /// Display name for UI
    pub display_name: String,

    /// Current status
    pub status: ModelStatus,

    /// File path (if available)
    pub path: PathBuf,

    /// Size in MB
    pub size_mb: u64,

    /// Context window size in tokens
    pub context_size: u32,

    /// Description
    pub description: String,

    /// GGUF filename on disk
    pub gguf_file: String,
}

// ============================================================================
// Model Manager
// ============================================================================

pub struct ModelManager {
    /// Directory where models are stored
    models_dir: PathBuf,

    /// Currently available models with their status
    available_models: Arc<RwLock<HashMap<String, ModelInfo>>>,

    /// Active downloads (model names)
    active_downloads: Arc<RwLock<HashSet<String>>>,

    /// Cancellation flag for current download
    cancel_download_flag: Arc<RwLock<Option<String>>>,
}

impl ModelManager {
    /// Create a new model manager with default models directory
    pub fn new() -> Result<Self> {
        Self::new_with_models_dir(None)
    }

    /// Create a new model manager with custom models directory
    pub fn new_with_models_dir(models_dir: Option<PathBuf>) -> Result<Self> {
        let models_dir = if let Some(dir) = models_dir {
            dir
        } else {
            // Fallback: Use current directory in development
            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;

            if cfg!(debug_assertions) {
                // Development mode
                current_dir.join("models").join("summary")
            } else {
                // Production mode fallback (caller should provide path)
                log::warn!("ModelManager: No models directory provided, using fallback path");
                dirs::data_dir()
                    .or_else(|| dirs::home_dir())
                    .ok_or_else(|| anyhow!("Could not find system data directory"))?
                    .join("Meetily")
                    .join("models")
                    .join("summary")
            }
        };

        log::info!(
            "Built-in AI ModelManager using directory: {}",
            models_dir.display()
        );

        Ok(Self {
            models_dir,
            available_models: Arc::new(RwLock::new(HashMap::new())),
            active_downloads: Arc::new(RwLock::new(HashSet::new())),
            cancel_download_flag: Arc::new(RwLock::new(None)),
        })
    }

    /// Initialize and scan for existing models
    pub async fn init(&self) -> Result<()> {
        // Create models directory if it doesn't exist
        if !self.models_dir.exists() {
            fs::create_dir_all(&self.models_dir).await?;
            log::info!("Created models directory: {}", self.models_dir.display());
        }

        // Scan for existing models
        self.scan_models().await?;

        Ok(())
    }

    /// Scan models directory and update status
    pub async fn scan_models(&self) -> Result<()> {
        let start = std::time::Instant::now();

        log::info!(
            "Starting model scan in directory: {}",
            self.models_dir.display()
        );

        let model_defs = get_available_models();
        let mut models_map = HashMap::new();

        for model_def in model_defs {
            let model_path = self.models_dir.join(&model_def.gguf_file);
            log::debug!(
                "Checking model '{}' at path: {}",
                model_def.name,
                model_path.display()
            );

            let is_actively_downloading = {
                let active = self.active_downloads.read().await;
                active.contains(&model_def.name)
            };

            // If actively downloading, preserve existing status from memory
            if is_actively_downloading {
                let existing_info = {
                    let models = self.available_models.read().await;
                    models.get(&model_def.name).cloned()
                };

                if let Some(info) = existing_info {
                    // Preserve existing status (should be Downloading)
                    models_map.insert(model_def.name.clone(), info);
                    log::debug!(
                        "Model '{}': Preserving Downloading status during scan",
                        model_def.name
                    );
                    continue;
                }
            }

            let status = if model_path.exists() {
                // Check if file size matches expected size (basic validation)
                match fs::metadata(&model_path).await {
                    Ok(metadata) => {
                        let file_size_mb = metadata.len() / (1024 * 1024);

                        // Allow 10% variance for file size check
                        let expected_min = (model_def.size_mb as f64 * 0.9) as u64;
                        let expected_max = (model_def.size_mb as f64 * 1.1) as u64;

                        log::info!(
                            "Model '{}': found {} MB (expected {}-{} MB)",
                            model_def.name,
                            file_size_mb,
                            expected_min,
                            expected_max
                        );

                        if file_size_mb >= expected_min && file_size_mb <= expected_max {
                            log::info!("Model '{}': AVAILABLE", model_def.name);
                            ModelStatus::Available
                        } else {
                            log::warn!(
                                "Model '{}': CORRUPTED (size mismatch: {} MB, expected {} MB)",
                                model_def.name,
                                file_size_mb,
                                model_def.size_mb
                            );
                            ModelStatus::Corrupted {
                                file_size: file_size_mb,
                                expected_min_size: expected_min,
                            }
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Model '{}': Failed to read metadata: {}",
                            model_def.name,
                            e
                        );
                        ModelStatus::Error(format!("Failed to read metadata: {}", e))
                    }
                }
            } else {
                log::debug!("Model '{}': NOT FOUND", model_def.name);
                ModelStatus::NotDownloaded
            };

            let model_info = ModelInfo {
                name: model_def.name.clone(),
                display_name: model_def.display_name.clone(),
                status,
                path: model_path,
                size_mb: model_def.size_mb,
                context_size: model_def.context_size,
                description: model_def.description.clone(),
                gguf_file: model_def.gguf_file.clone(),
            };

            models_map.insert(model_def.name.clone(), model_info);
        }

        let model_count = models_map.len();

        let mut models = self.available_models.write().await;
        *models = models_map;

        let elapsed = start.elapsed();
        log::info!(
            "Model scan complete: {} models checked in {:?}",
            model_count,
            elapsed
        );
        Ok(())
    }

    /// Get list of all models with their status
    pub async fn list_models(&self) -> Vec<ModelInfo> {
        self.available_models
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// Get info for a specific model
    pub async fn get_model_info(&self, model_name: &str) -> Option<ModelInfo> {
        self.available_models
            .read()
            .await
            .get(model_name)
            .cloned()
    }

    /// Check if a model is ready to use
    /// If refresh=true, scans filesystem before checking (slower but accurate)
    pub async fn is_model_ready(&self, model_name: &str, refresh: bool) -> bool {
        if refresh {
            if let Err(e) = self.scan_models().await {
                log::error!("Failed to scan models: {}", e);
                return false;
            }
        }

        if let Some(info) = self.get_model_info(model_name).await {
            info.status == ModelStatus::Available
        } else {
            false
        }
    }

    /// Download a model with simple percentage callback (backward compatible)
    pub async fn download_model(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        // Wrap the simple callback to use detailed progress internally
        let detailed_callback: Option<Box<dyn Fn(DownloadProgress) + Send>> =
            progress_callback.map(|cb| {
                Box::new(move |p: DownloadProgress| cb(p.percent)) as Box<dyn Fn(DownloadProgress) + Send>
            });
        self.download_model_detailed(model_name, detailed_callback).await
    }

    /// Download a model with detailed progress (MB, speed, etc.)
    pub async fn download_model_detailed(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        log::info!("Starting download for model: {}", model_name);

        // Check if already downloading
        {
            let active = self.active_downloads.read().await;
            if active.contains(model_name) {
                log::warn!("Download already in progress for model: {}", model_name);
                return Err(anyhow!("Download already in progress"));
            }
        }

        // Get model definition
        let model_def = get_model_by_name(model_name)
            .ok_or_else(|| anyhow!("Unknown model: {}", model_name))?;

        // Add to active downloads
        {
            let mut active = self.active_downloads.write().await;
            active.insert(model_name.to_string());
        }

        // Clear cancellation flag
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            *cancel_flag = None;
        }

        // Update status to downloading
        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::Downloading { progress: 0 };
            }
        }

        let file_path = self.models_dir.join(&model_def.gguf_file);

        // Check if model already exists and is valid (skip re-download)
        if file_path.exists() {
            if let Ok(metadata) = fs::metadata(&file_path).await {
                let file_size_mb = metadata.len() / (1024 * 1024);
                let expected_min = (model_def.size_mb as f64 * 0.9) as u64;
                let expected_max = (model_def.size_mb as f64 * 1.1) as u64;

                if file_size_mb >= expected_min && file_size_mb <= expected_max {
                    log::info!(
                        "Model '{}' already exists and is valid ({} MB), skipping download",
                        model_name,
                        file_size_mb
                    );

                    // Update status to available
                    {
                        let mut models = self.available_models.write().await;
                        if let Some(model_info) = models.get_mut(model_name) {
                            model_info.status = ModelStatus::Available;
                        }
                    }

                    // Remove from active downloads
                    {
                        let mut active = self.active_downloads.write().await;
                        active.remove(model_name);
                    }

                    // Report 100% progress
                    if let Some(ref callback) = progress_callback {
                        let total = metadata.len();
                        callback(DownloadProgress::new(total, total, 0.0));
                    }

                    return Ok(());
                } else if file_size_mb > expected_max {
                    // File is LARGER than expected - possibly corrupted or wrong file
                    // Delete and re-download in this case
                    log::warn!(
                        "Model '{}' exists but is too large ({} MB, expected max {} MB), deleting and re-downloading",
                        model_name,
                        file_size_mb,
                        expected_max
                    );
                    if let Err(e) = fs::remove_file(&file_path).await {
                        log::warn!("Failed to delete oversized model file: {}", e);
                    }
                } else {
                    // File is SMALLER than expected - likely partial download
                    // DON'T DELETE - let resume logic handle it
                    log::info!(
                        "Model '{}' exists but is incomplete ({} MB, expected min {} MB), will resume download",
                        model_name,
                        file_size_mb,
                        expected_min
                    );
                    // Continue to download/resume logic below
                }
            }
        }

        log::info!("Downloading model '{}' to: {}", model_name, file_path.display());

        // Create models directory if needed
        if !self.models_dir.exists() {
            fs::create_dir_all(&self.models_dir).await?;
        }

        // Download the file with optimized client settings
        let client = Client::builder()
            .tcp_nodelay(true)
            .pool_max_idle_per_host(1)
            .timeout(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        // Build list of URLs to try (primary + fallbacks)
        let urls_to_try: Vec<String> = std::iter::once(model_def.download_url.clone())
            .chain(model_def.fallback_urls.iter().cloned())
            .collect();

        let download_start_time = std::time::Instant::now();
        let mut last_error = String::new();
        let mut download_succeeded = false;

        // Try each URL for the entire download (connection + streaming).
        // Mid-stream failures (timeout, dropped connection) fall through to the next URL
        // with resume support, so partial progress is not lost.
        'url_loop: for (url_index, url) in urls_to_try.iter().enumerate() {
            let is_fallback = url_index > 0;
            if is_fallback {
                log::warn!(
                    "URL failed ({}), trying fallback {}/{}: {}",
                    last_error, url_index, urls_to_try.len() - 1, url
                );
            } else {
                log::info!("Downloading from: {}", url);
            }

            // Re-check existing file size (may have partial data from previous URL attempt)
            let existing_size: u64 = if file_path.exists() {
                fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };

            // Connect with optional Range header for resume
            let mut request = client.get(url.as_str());
            if existing_size > 0 {
                log::info!("Resuming from byte {} ({:.1} MB)", existing_size, existing_size as f64 / (1024.0 * 1024.0));
                request = request.header("Range", format!("bytes={}-", existing_size));
            }

            let resp = match request.send().await {
                Ok(r) => r,
                Err(e) => {
                    last_error = format!("Connection failed: {}", e);
                    log::warn!("Connection to {} failed: {}", url, e);
                    continue 'url_loop;
                }
            };

            // Handle response status
            let (total_size, resuming) = if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                let remaining = resp.content_length().unwrap_or(0);
                log::info!("Server supports resume, {} MB remaining", remaining / (1024 * 1024));
                (existing_size + remaining, true)
            } else if resp.status().is_success() {
                if existing_size > 0 {
                    log::warn!("Server doesn't support resume, starting fresh download");
                }
                (resp.content_length().unwrap_or(0), false)
            } else {
                last_error = format!("HTTP {}", resp.status());
                log::warn!("URL {} returned status: {}", url, resp.status());
                continue 'url_loop;
            };

            if is_fallback {
                log::info!("Fallback URL connected: {}", url);
            }
            log::info!("Total size: {} MB", total_size / (1024 * 1024));

            // Open file for append if resuming, or create new
            let file = if resuming {
                match OpenOptions::new().write(true).append(true).open(&file_path).await {
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
                        // File creation is not URL-specific, don't retry
                        let mut active = self.active_downloads.write().await;
                        active.remove(model_name);
                        return Err(anyhow!("Failed to create file: {}", e));
                    }
                }
            };

            let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file);
            let mut downloaded: u64 = if resuming { existing_size } else { 0 };
            let start_downloaded = downloaded;

            if let Some(ref callback) = progress_callback {
                callback(DownloadProgress::new(downloaded, total_size, 0.0));
            }

            let mut last_progress_percent: u8 = if total_size > 0 {
                ((downloaded as f64 / total_size as f64) * 100.0) as u8
            } else {
                0
            };
            let mut last_report_time = std::time::Instant::now();
            let mut bytes_since_last_report: u64 = 0;

            use futures_util::StreamExt;
            let mut stream = resp.bytes_stream();

            let stream_ok = loop {
                // Check for cancellation
                {
                    let cancel_flag = self.cancel_download_flag.read().await;
                    if cancel_flag.as_ref() == Some(&model_name.to_string()) {
                        log::info!("Download cancelled for model: {}", model_name);
                        let _ = writer.flush().await;
                        drop(writer);
                        let mut active = self.active_downloads.write().await;
                        active.remove(model_name);
                        {
                            let mut models = self.available_models.write().await;
                            if let Some(model_info) = models.get_mut(model_name) {
                                model_info.status = ModelStatus::NotDownloaded;
                            }
                        }
                        return Err(anyhow!("CANCELLED: Download cancelled by user"));
                    }
                }

                let next_result = timeout(Duration::from_secs(30), stream.next()).await;

                match next_result {
                    Err(_) => {
                        // Timeout - flush partial data and try next URL
                        log::warn!("Stream timeout on {}, will try next URL", url);
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
                        writer.write_all(&chunk).await
                            .map_err(|e| anyhow!("Error writing to file: {}", e))?;

                        downloaded += chunk_len;
                        bytes_since_last_report += chunk_len;

                        // Progress reporting
                        let progress_percent = if total_size > 0 {
                            ((downloaded as f64 / total_size as f64) * 100.0).min(100.0) as u8
                        } else {
                            0
                        };

                        let elapsed_since_report = last_report_time.elapsed();
                        let is_download_complete = downloaded >= total_size;
                        let should_report = progress_percent > last_progress_percent
                            || is_download_complete
                            || elapsed_since_report.as_millis() >= 500;

                        if should_report {
                            let speed_mbps = if elapsed_since_report.as_secs_f64() > 0.0 {
                                (bytes_since_last_report as f64 / (1024.0 * 1024.0)) / elapsed_since_report.as_secs_f64()
                            } else {
                                let total_elapsed = download_start_time.elapsed().as_secs_f64();
                                if total_elapsed > 0.0 {
                                    ((downloaded - start_downloaded) as f64 / (1024.0 * 1024.0)) / total_elapsed
                                } else {
                                    0.0
                                }
                            };

                            {
                                let mut models = self.available_models.write().await;
                                if let Some(model_info) = models.get_mut(model_name) {
                                    model_info.status = ModelStatus::Downloading {
                                        progress: if is_download_complete { 100 } else { progress_percent }
                                    };
                                }
                            }

                            if let Some(ref callback) = progress_callback {
                                callback(DownloadProgress::new(downloaded, total_size, speed_mbps));
                            }

                            last_progress_percent = progress_percent;
                            last_report_time = std::time::Instant::now();
                            bytes_since_last_report = 0;
                        }
                    }
                    Ok(Some(Err(e))) => {
                        // Stream error - flush partial data and try next URL
                        log::warn!("Stream error on {}: {}", url, e);
                        let _ = writer.flush().await;
                        drop(writer);
                        last_error = format!("Stream error: {}", e);
                        break false;
                    }
                }
            };

            if stream_ok {
                writer.flush().await?;
                drop(writer);
                download_succeeded = true;
                break 'url_loop;
            }
            // If !stream_ok, continue to next URL with resume
        }

        if !download_succeeded {
            let mut active = self.active_downloads.write().await;
            active.remove(model_name);

            {
                let mut models = self.available_models.write().await;
                if let Some(model_info) = models.get_mut(model_name) {
                    model_info.status = ModelStatus::Error(last_error.clone());
                }
            }

            return Err(anyhow!(
                "Download failed - all download servers failed (tried {} servers). \
                Last error: {}. This may be a server issue; please try again later.",
                urls_to_try.len(), last_error
            ));
        }

        log::info!("Download completed for model: {}", model_name);

        // Get final file size for progress reporting
        let final_size = fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(0);

        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::Downloading { progress: 100 };
            }
        }

        if let Some(ref callback) = progress_callback {
            callback(DownloadProgress::new(final_size, final_size, 0.0));
        }

        // Small delay to ensure UI receives 100% event
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        if let Err(e) = self.validate_gguf_file(&file_path).await {
            log::error!("Downloaded file failed validation: {}", e);

            // Clean up invalid file
            let _ = fs::remove_file(&file_path).await;

            // Update status
            {
                let mut models = self.available_models.write().await;
                if let Some(model_info) = models.get_mut(model_name) {
                    model_info.status = ModelStatus::Error(format!("Validation failed: {}", e));
                }
            }

            // Remove from active downloads
            let mut active = self.active_downloads.write().await;
            active.remove(model_name);

            return Err(anyhow!("File validation failed: {}", e));
        }

        // Update status to available
        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::Available;
                model_info.path = file_path.clone();
            }
        }

        // Remove from active downloads
        {
            let mut active = self.active_downloads.write().await;
            active.remove(model_name);
        }

        Ok(())
    }

    /// Validate that a file is a valid GGUF model
    async fn validate_gguf_file(&self, path: &PathBuf) -> Result<()> {
        let mut file = fs::File::open(path).await?;

        // Read first 4 bytes to check for GGUF magic number
        use tokio::io::AsyncReadExt;
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic).await?;

        // GGUF magic number is "GGUF" (0x47475546)
        if &magic == b"GGUF" {
            Ok(())
        } else if &magic == b"ggjt" || &magic == b"ggla" || &magic == b"ggml" {
            // Older formats (GGML, GGJT)
            Ok(())
        } else {
            Err(anyhow!(
                "Invalid model file: magic number {:?} doesn't match GGUF/GGML",
                magic
            ))
        }
    }

    /// Cancel an ongoing download
    pub async fn cancel_download(&self, model_name: &str) -> Result<()> {
        log::info!("Cancelling download for model: {}", model_name);

        // Set cancellation flag - download loop will detect this and handle cleanup
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            *cancel_flag = Some(model_name.to_string());
        }

        // Note: active_downloads cleanup is handled by the download loop when it detects
        // the cancellation flag. This avoids double-removal race condition.

        // Update status immediately for UI responsiveness
        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::NotDownloaded;
            }
        }

        // Brief delay to let download loop detect cancellation
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(())
    }

    /// Delete a corrupted or available model file
    pub async fn delete_model(&self, model_name: &str) -> Result<()> {
        log::info!("Deleting model: {}", model_name);

        let model_def = get_model_by_name(model_name)
            .ok_or_else(|| anyhow!("Unknown model: {}", model_name))?;

        let file_path = self.models_dir.join(&model_def.gguf_file);

        if file_path.exists() {
            fs::remove_file(&file_path).await?;
            log::info!("Deleted model file: {}", file_path.display());
        }

        // Update status
        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::NotDownloaded;
            }
        }

        Ok(())
    }

    /// Get models directory path
    pub fn get_models_directory(&self) -> PathBuf {
        self.models_dir.clone()
    }
}
