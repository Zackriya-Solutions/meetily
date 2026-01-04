// Auto-recording module for always-on meeting capture with silence-based segmentation
use anyhow::Result;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::{Mutex, RwLock};

use super::recording_preferences::{
    ensure_transcript_export_folder, load_recording_preferences, RecordingPreferences,
};
use super::recording_saver::TranscriptSegment;

/// Auto-recording state that can be shared across threads
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoRecordingState {
    Stopped,
    Starting,
    Recording,
    Segmenting,  // Creating a new segment due to silence
    Stopping,
}

/// Represents a completed meeting segment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSegment {
    pub segment_id: String,
    pub meeting_name: String,
    pub folder_path: PathBuf,
    pub start_time: String,
    pub end_time: String,
    pub duration_seconds: f64,
    pub transcript_count: usize,
    pub exported: bool,
}

/// Manages continuous background recording with automatic segmentation
pub struct AutoRecordingManager {
    // State
    state: Arc<RwLock<AutoRecordingState>>,
    is_running: Arc<AtomicBool>,

    // Silence detection
    last_speech_timestamp: Arc<AtomicU64>,  // Timestamp of last detected speech (ms)
    current_segment_start: Arc<RwLock<Option<Instant>>>,
    silence_threshold_seconds: u32,
    minimum_segment_minutes: u32,

    // Segment tracking
    segments: Arc<Mutex<Vec<MeetingSegment>>>,
    current_segment_id: Arc<RwLock<Option<String>>>,

    // Settings
    preferences: Arc<RwLock<RecordingPreferences>>,
}

impl AutoRecordingManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(AutoRecordingState::Stopped)),
            is_running: Arc::new(AtomicBool::new(false)),
            last_speech_timestamp: Arc::new(AtomicU64::new(0)),
            current_segment_start: Arc::new(RwLock::new(None)),
            silence_threshold_seconds: 30,
            minimum_segment_minutes: 30,
            segments: Arc::new(Mutex::new(Vec::new())),
            current_segment_id: Arc::new(RwLock::new(None)),
            preferences: Arc::new(RwLock::new(RecordingPreferences::default())),
        }
    }

    /// Update settings from preferences
    pub async fn update_settings(&self, prefs: &RecordingPreferences) {
        let mut preferences = self.preferences.write().await;
        *preferences = prefs.clone();
    }

    /// Get current auto-recording state
    pub async fn get_state(&self) -> AutoRecordingState {
        *self.state.read().await
    }

    /// Check if auto-recording is currently running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Record speech activity (updates last speech timestamp)
    pub fn record_speech_activity(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.last_speech_timestamp.store(now, Ordering::SeqCst);
    }

    /// Get seconds since last speech activity
    pub fn seconds_since_last_speech(&self) -> u64 {
        let last = self.last_speech_timestamp.load(Ordering::SeqCst);
        if last == 0 {
            return 0;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        (now.saturating_sub(last)) / 1000
    }

    /// Check if we should segment based on silence duration and minimum segment time
    pub async fn should_segment(&self) -> bool {
        let prefs = self.preferences.read().await;
        let silence_seconds = self.seconds_since_last_speech();

        // Check if silence threshold exceeded
        if silence_seconds < prefs.silence_threshold_seconds as u64 {
            return false;
        }

        // Check minimum segment duration
        let segment_start = self.current_segment_start.read().await;
        if let Some(start) = *segment_start {
            let segment_duration = start.elapsed();
            let min_duration = Duration::from_secs(prefs.minimum_segment_minutes as u64 * 60);
            return segment_duration >= min_duration;
        }

        false
    }

    /// Start a new segment (called when silence threshold is met)
    pub async fn start_new_segment(&self) -> String {
        let segment_id = format!(
            "segment_{}",
            chrono::Local::now().format("%Y%m%d_%H%M%S")
        );

        let mut current_id = self.current_segment_id.write().await;
        *current_id = Some(segment_id.clone());

        let mut segment_start = self.current_segment_start.write().await;
        *segment_start = Some(Instant::now());

        // Reset speech timestamp
        self.record_speech_activity();

        info!("Started new auto-recording segment: {}", segment_id);
        segment_id
    }

    /// Complete current segment and add to list
    pub async fn complete_segment(
        &self,
        meeting_name: String,
        folder_path: PathBuf,
        transcripts: Vec<TranscriptSegment>,
    ) -> Option<MeetingSegment> {
        let segment_id = {
            let id = self.current_segment_id.read().await;
            id.clone()
        };

        let segment_id = segment_id?;

        let (start_time, duration) = {
            let start = self.current_segment_start.read().await;
            if let Some(start_instant) = *start {
                let duration = start_instant.elapsed().as_secs_f64();
                (
                    chrono::Local::now()
                        .checked_sub_signed(chrono::Duration::seconds(duration as i64))
                        .unwrap_or_else(chrono::Local::now)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                    duration,
                )
            } else {
                (chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(), 0.0)
            }
        };

        let segment = MeetingSegment {
            segment_id: segment_id.clone(),
            meeting_name,
            folder_path,
            start_time,
            end_time: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            duration_seconds: duration,
            transcript_count: transcripts.len(),
            exported: false,
        };

        // Add to segments list
        {
            let mut segments = self.segments.lock().await;
            segments.push(segment.clone());
        }

        // Clear current segment
        {
            let mut current_id = self.current_segment_id.write().await;
            *current_id = None;
        }

        info!(
            "Completed auto-recording segment: {} ({:.1}s, {} transcripts)",
            segment_id, duration, segment.transcript_count
        );

        Some(segment)
    }

    /// Get all completed segments
    pub async fn get_segments(&self) -> Vec<MeetingSegment> {
        let segments = self.segments.lock().await;
        segments.clone()
    }

    /// Mark a segment as exported
    pub async fn mark_segment_exported(&self, segment_id: &str) {
        let mut segments = self.segments.lock().await;
        if let Some(segment) = segments.iter_mut().find(|s| s.segment_id == segment_id) {
            segment.exported = true;
            info!("Marked segment {} as exported", segment_id);
        }
    }
}

impl Default for AutoRecordingManager {
    fn default() -> Self {
        Self::new()
    }
}

// Global auto-recording manager instance
lazy_static::lazy_static! {
    pub static ref AUTO_RECORDING_MANAGER: Arc<AutoRecordingManager> = Arc::new(AutoRecordingManager::new());
}

/// Export transcript to a text file
pub fn export_transcript_to_file(
    transcripts: &[TranscriptSegment],
    export_folder: &PathBuf,
    meeting_name: &str,
    start_time: &str,
    duration_seconds: f64,
) -> Result<PathBuf> {
    // Ensure export folder exists
    ensure_transcript_export_folder(export_folder)?;

    // Generate filename: YYYY-MM-DD_HH-MM_{first-10-words-of-transcript}.txt
    let first_10_words = get_first_n_words_from_transcripts(transcripts, 10);
    let sanitized_words = sanitize_filename(&first_10_words);
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M");
    let filename = format!("{}_{}.txt", timestamp, sanitized_words);
    let export_path = export_folder.join(&filename);

    // Build transcript content
    let mut content = String::new();

    // Header with metadata
    content.push_str(&format!("Meeting Transcript\n"));
    content.push_str(&format!("==================\n\n"));
    content.push_str(&format!("Meeting: {}\n", meeting_name));
    content.push_str(&format!("Date/Time: {}\n", start_time));
    content.push_str(&format!("Duration: {}\n", format_duration(duration_seconds)));
    content.push_str(&format!("Exported: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    content.push_str(&format!("\n---\n\n"));

    // Transcript content
    for segment in transcripts {
        content.push_str(&format!("{} {}\n\n", segment.display_time, segment.text));
    }

    // Write to file
    std::fs::write(&export_path, content)?;
    info!("Exported transcript to: {:?}", export_path);

    Ok(export_path)
}

/// Get first N words from transcript segments
fn get_first_n_words_from_transcripts(transcripts: &[TranscriptSegment], n: usize) -> String {
    let mut words = Vec::new();
    for segment in transcripts {
        for word in segment.text.split_whitespace() {
            words.push(word.to_string());
            if words.len() >= n {
                break;
            }
        }
        if words.len() >= n {
            break;
        }
    }
    words.join(" ")
}

/// Sanitize a string for use in a filename
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(50) // Limit length
        .collect()
}

/// Format duration as human-readable string
fn format_duration(seconds: f64) -> String {
    let hours = (seconds / 3600.0).floor() as u32;
    let minutes = ((seconds % 3600.0) / 60.0).floor() as u32;
    let secs = (seconds % 60.0).floor() as u32;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

/// Clean up old audio files based on retention period
pub async fn cleanup_old_audio_files<R: Runtime>(app: &AppHandle<R>) -> Result<usize> {
    let prefs = load_recording_preferences(app).await?;
    let retention_days = prefs.audio_retention_days;
    let recordings_folder = prefs.save_folder;

    if retention_days == 0 {
        info!("Audio retention is disabled (0 days), skipping cleanup");
        return Ok(0);
    }

    let cutoff_time = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
    let mut deleted_count = 0;

    info!(
        "Cleaning up audio files older than {} days (cutoff: {})",
        retention_days,
        cutoff_time.format("%Y-%m-%d %H:%M:%S")
    );

    // Iterate through meeting folders
    if let Ok(entries) = std::fs::read_dir(&recordings_folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Check metadata.json for creation time
            let metadata_path = path.join("metadata.json");
            if metadata_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&metadata_path) {
                    if let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(created_at) = metadata.get("created_at").and_then(|v| v.as_str())
                        {
                            if let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) {
                                if created < cutoff_time {
                                    // Delete audio files but keep transcripts
                                    let audio_extensions = ["wav", "mp4", "mp3", "m4a", "ogg", "flac"];

                                    for ext in audio_extensions {
                                        let audio_file = path.join(format!("audio.{}", ext));
                                        if audio_file.exists() {
                                            if let Err(e) = std::fs::remove_file(&audio_file) {
                                                warn!("Failed to delete audio file {:?}: {}", audio_file, e);
                                            } else {
                                                info!("Deleted old audio file: {:?}", audio_file);
                                                deleted_count += 1;
                                            }
                                        }
                                    }

                                    // Also clean up .checkpoints directory
                                    let checkpoints_dir = path.join(".checkpoints");
                                    if checkpoints_dir.exists() {
                                        if let Err(e) = std::fs::remove_dir_all(&checkpoints_dir) {
                                            warn!("Failed to delete checkpoints dir {:?}: {}", checkpoints_dir, e);
                                        } else {
                                            info!("Deleted checkpoints directory: {:?}", checkpoints_dir);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    info!("Audio cleanup complete: deleted {} files", deleted_count);
    Ok(deleted_count)
}

/// Start the background cleanup task
pub fn start_cleanup_task<R: Runtime + 'static>(app: AppHandle<R>) {
    tokio::spawn(async move {
        // Initial delay to let the app fully start
        tokio::time::sleep(Duration::from_secs(60)).await;

        loop {
            // Run cleanup
            match cleanup_old_audio_files(&app).await {
                Ok(count) => {
                    if count > 0 {
                        info!("Background cleanup deleted {} old audio files", count);
                    }
                }
                Err(e) => {
                    error!("Background cleanup failed: {}", e);
                }
            }

            // Run cleanup once per hour
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

/// Start auto-recording if enabled and not already recording
pub async fn start_auto_recording<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let prefs = load_recording_preferences(app).await?;

    if !prefs.auto_recording_enabled {
        info!("Auto-recording is disabled, skipping start");
        return Ok(());
    }

    // Check if already recording
    if super::recording_commands::is_recording().await {
        info!("Already recording, skipping auto-recording start");
        return Ok(());
    }

    info!("🎙️ Starting auto-recording...");

    // Update manager settings
    AUTO_RECORDING_MANAGER.update_settings(&prefs).await;

    // Start a new segment
    let segment_id = AUTO_RECORDING_MANAGER.start_new_segment().await;
    info!("Started auto-recording segment: {}", segment_id);

    // Generate meeting name based on timestamp
    let meeting_name = format!(
        "Auto-Recording {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    );

    // Start recording with default devices
    match super::recording_commands::start_recording_with_meeting_name(
        app.clone(),
        Some(meeting_name),
    )
    .await
    {
        Ok(_) => {
            info!("✅ Auto-recording started successfully");

            // Start the silence monitoring task
            let app_clone = app.clone();
            tokio::spawn(async move {
                monitor_silence_for_segmentation(app_clone).await;
            });

            Ok(())
        }
        Err(e) => {
            error!("❌ Failed to start auto-recording: {}", e);
            Err(anyhow::anyhow!("Failed to start auto-recording: {}", e))
        }
    }
}

/// Monitor for silence and trigger segmentation when threshold is met
async fn monitor_silence_for_segmentation<R: Runtime>(app: AppHandle<R>) {
    info!("Starting silence monitor for auto-recording segmentation");

    loop {
        // Wait before checking again
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Check if still recording
        if !super::recording_commands::is_recording().await {
            info!("Recording stopped, ending silence monitor");
            break;
        }

        // Check if we should segment
        if AUTO_RECORDING_MANAGER.should_segment().await {
            info!("🔕 Silence threshold met, segmenting recording...");

            // Stop current recording and start new segment
            if let Err(e) = segment_and_restart(&app).await {
                error!("Failed to segment recording: {}", e);
            }
        }
    }
}

/// Stop current recording, export transcript, and start a new segment
async fn segment_and_restart<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let prefs = load_recording_preferences(app).await?;

    // Get current meeting info before stopping
    let meeting_folder = super::recording_commands::get_meeting_folder_path(app.clone()).await;
    let meeting_name = super::recording_commands::get_recording_meeting_name()
        .await
        .unwrap_or_else(|_| None)
        .unwrap_or_else(|| "Auto-Recording".to_string());

    // Get transcripts before stopping (returns Vec<TranscriptSegment> already)
    let transcripts = super::recording_commands::get_transcript_history()
        .await
        .unwrap_or_default();

    // Stop current recording
    let data_dir = app.path().app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?;
    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let save_path = data_dir.join(format!("recording-{}.wav", timestamp));

    super::recording_commands::stop_recording(
        app.clone(),
        super::recording_commands::RecordingArgs {
            save_path: save_path.to_string_lossy().to_string(),
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to stop recording: {}", e))?;

    // Complete the segment
    if let Some(folder_path) = meeting_folder {
        let segment = AUTO_RECORDING_MANAGER
            .complete_segment(
                meeting_name.clone(),
                PathBuf::from(&folder_path),
                transcripts.clone(),
            )
            .await;

        // Export transcript
        if let Some(seg) = segment {
            match export_transcript_to_file(
                &transcripts,
                &prefs.transcript_export_folder,
                &seg.meeting_name,
                &seg.start_time,
                seg.duration_seconds,
            ) {
                Ok(export_path) => {
                    info!("📝 Exported transcript to: {:?}", export_path);
                    AUTO_RECORDING_MANAGER.mark_segment_exported(&seg.segment_id).await;
                }
                Err(e) => {
                    error!("Failed to export transcript: {}", e);
                }
            }
        }
    }

    // Wait a moment before starting new recording
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check if auto-recording is still enabled
    let prefs = load_recording_preferences(app).await?;
    if prefs.auto_recording_enabled {
        // Start a new segment
        start_auto_recording(app).await?;
    }

    Ok(())
}

// Tauri commands for auto-recording

/// Get current auto-recording state
#[tauri::command]
pub async fn get_auto_recording_state() -> Result<String, String> {
    let state = AUTO_RECORDING_MANAGER.get_state().await;
    Ok(format!("{:?}", state))
}

/// Check if auto-recording is running
#[tauri::command]
pub async fn is_auto_recording_running() -> bool {
    AUTO_RECORDING_MANAGER.is_running()
}

/// Get completed segments
#[tauri::command]
pub async fn get_auto_recording_segments() -> Result<Vec<MeetingSegment>, String> {
    Ok(AUTO_RECORDING_MANAGER.get_segments().await)
}

/// Manually trigger audio cleanup
#[tauri::command]
pub async fn trigger_audio_cleanup<R: Runtime>(app: AppHandle<R>) -> Result<usize, String> {
    cleanup_old_audio_files(&app)
        .await
        .map_err(|e| format!("Cleanup failed: {}", e))
}
