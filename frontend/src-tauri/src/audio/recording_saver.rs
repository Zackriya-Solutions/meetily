use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
use anyhow::Result;
use log::{info, warn, error};
use tauri::{AppHandle, Runtime, Emitter};
use tokio::sync::mpsc;
use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use tokio::task::JoinHandle;
#[cfg(test)]
use std::sync::{atomic::{AtomicBool, Ordering}, Barrier};

use super::recording_state::AudioChunk;
use super::audio_processing::create_meeting_folder;
use super::incremental_saver::IncrementalAudioSaver;

/// Structured transcript segment for JSON export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub text: String,
    pub audio_start_time: f64, // Seconds from recording start
    pub audio_end_time: f64,   // Seconds from recording start
    pub duration: f64,          // Segment duration in seconds
    pub display_time: String,   // Formatted time for display like "[02:15]"
    pub confidence: f32,
    pub sequence_id: u64,
}

/// Meeting metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMetadata {
    pub version: String,
    pub meeting_id: Option<String>,
    pub meeting_name: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<f64>,
    pub devices: DeviceInfo,
    pub audio_file: String,
    pub transcript_file: String,
    pub sample_rate: u32,
    pub status: String,  // "recording", "completed", "error"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub microphone: Option<String>,
    pub system_audio: Option<String>,
}

/// New recording saver using incremental saving strategy
pub struct RecordingSaver {
    incremental_saver: Option<Arc<AsyncMutex<IncrementalAudioSaver>>>,
    meeting_folder: Option<PathBuf>,
    meeting_name: Option<String>,
    metadata: Option<MeetingMetadata>,
    transcript_segments: Arc<Mutex<Vec<TranscriptSegment>>>,
    is_saving: Arc<Mutex<bool>>,
    accumulation_task: Option<JoinHandle<Result<(), String>>>,
    #[cfg(test)]
    test_failure: Option<TestFailure>,
    #[cfg(test)]
    test_chunk_observer: Option<Arc<Mutex<Vec<Vec<f32>>>>>,
    #[cfg(test)]
    test_saver_gate: Option<Arc<Barrier>>,
    #[cfg(test)]
    test_saver_gate_entered: Option<Arc<AtomicBool>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestFailure {
    IncrementalSaverInitialization,
    AudioFinalization,
    TranscriptWrite,
    MetadataCompletion,
}

impl RecordingSaver {
    pub fn new() -> Self {
        Self {
            incremental_saver: None,
            meeting_folder: None,
            meeting_name: None,
            metadata: None,
            transcript_segments: Arc::new(Mutex::new(Vec::new())),
            is_saving: Arc::new(Mutex::new(false)),
            accumulation_task: None,
            #[cfg(test)]
            test_failure: None,
            #[cfg(test)]
            test_chunk_observer: None,
            #[cfg(test)]
            test_saver_gate: None,
            #[cfg(test)]
            test_saver_gate_entered: None,
        }
    }

    /// Set the meeting name for this recording session
    pub fn set_meeting_name(&mut self, name: Option<String>) {
        self.meeting_name = name;
    }

    /// Set device information in metadata
    pub fn set_device_info(&mut self, mic_name: Option<String>, sys_name: Option<String>) {
        if let Some(ref mut metadata) = self.metadata {
            metadata.devices.microphone = mic_name;
            metadata.devices.system_audio = sys_name;

            // Write updated metadata to disk if folder exists
            if let Some(folder) = &self.meeting_folder {
                let metadata_clone = metadata.clone();
                if let Err(e) = self.write_metadata(folder, &metadata_clone) {
                    warn!("Failed to update metadata with device info: {}", e);
                }
            }
        }
    }

    /// Add or update a structured transcript segment (upserts based on sequence_id)
    /// Also saves incrementally to disk
    pub fn add_transcript_segment(&self, segment: TranscriptSegment) {
        if let Ok(mut segments) = self.transcript_segments.lock() {
            // Check if segment with same sequence_id exists (update it)
            if let Some(existing) = segments.iter_mut().find(|s| s.sequence_id == segment.sequence_id) {
                *existing = segment.clone();
                info!("Updated transcript segment {} (seq: {}) - total segments: {}",
                      segment.id, segment.sequence_id, segments.len());
            } else {
                // New segment, add it
                segments.push(segment.clone());
                info!("Added new transcript segment {} (seq: {}) - total segments: {}",
                      segment.id, segment.sequence_id, segments.len());
            }
        } else {
            error!("Failed to lock transcript segments for adding segment {}", segment.id);
        }

        // NEW: Save incrementally to disk
        if let Some(folder) = &self.meeting_folder {
            if let Err(e) = self.write_transcripts_json(folder) {
                warn!("Failed to write incremental transcript update: {}", e);
            }
        }
    }

    /// Legacy method for backward compatibility - converts text to basic segment
    pub fn add_transcript_chunk(&self, text: String) {
        let segment = TranscriptSegment {
            id: format!("seg_{}", chrono::Utc::now().timestamp_millis()),
            text,
            audio_start_time: 0.0,
            audio_end_time: 0.0,
            duration: 0.0,
            display_time: "[00:00]".to_string(),
            confidence: 1.0,
            sequence_id: 0,
        };
        self.add_transcript_segment(segment);
    }

    /// Start accumulation with optional incremental saving
    ///
    /// # Arguments
    /// * `auto_save` - If true, creates checkpoints and enables saving. If false, audio chunks are discarded.
    pub fn start_accumulation(
        &mut self,
        auto_save: bool,
        receiver: mpsc::UnboundedReceiver<AudioChunk>,
    ) -> Result<(), String> {
        self.start_accumulation_with_base(
            auto_save,
            receiver,
            super::recording_preferences::get_default_recordings_folder(),
        )
    }

    #[cfg(test)]
    pub(crate) fn start_accumulation_for_test(
        &mut self,
        auto_save: bool,
        receiver: mpsc::UnboundedReceiver<AudioChunk>,
        base_folder: &std::path::Path,
    ) -> Result<(), String> {
        self.start_accumulation_with_base(auto_save, receiver, base_folder.to_path_buf())
    }

    fn start_accumulation_with_base(
        &mut self,
        auto_save: bool,
        mut receiver: mpsc::UnboundedReceiver<AudioChunk>,
        base_folder: PathBuf,
    ) -> Result<(), String> {
        if auto_save {
            info!("Initializing incremental audio saver for recording (auto-save ENABLED)");
        } else {
            info!("Starting recording without audio saving (auto-save DISABLED - transcripts only)");
        }

        // Initialize meeting folder and incremental saver ONLY if auto_save is enabled
        if auto_save {
            let name = self
                .meeting_name
                .clone()
                .ok_or_else(|| "Cannot enable auto-save without a meeting name".to_string())?;
            self.initialize_meeting_folder_at(&base_folder, &name, true)
                .map_err(|e| format!("Failed to initialize recording storage: {e}"))?;
            info!("Successfully initialized meeting folder with checkpoints");
        } else {
            // When auto_save is false, still create meeting folder for transcripts/metadata
            // but skip .checkpoints directory
            let name = self
                .meeting_name
                .clone()
                .ok_or_else(|| "Cannot initialize transcript storage without a meeting name".to_string())?;
            self.initialize_meeting_folder_at(&base_folder, &name, false)
                .map_err(|e| format!("Failed to initialize transcript storage: {e}"))?;
            info!("Successfully initialized meeting folder (transcripts only)");
        }

        // Mark the receiver as live before spawning. The task drains every
        // accepted chunk until the producer closes; the old flag check could
        // discard the first queued chunk after Stop.
        if let Ok(mut is_saving) = self.is_saving.lock() {
            *is_saving = true;
        }

        let is_saving_clone = self.is_saving.clone();
        let incremental_saver_arc = self.incremental_saver.clone();
        let save_audio = auto_save;
        #[cfg(test)]
        let test_chunk_observer = self.test_chunk_observer.clone();
        #[cfg(test)]
        let test_saver_gate = self.test_saver_gate.clone();
        #[cfg(test)]
        let test_saver_gate_entered = self.test_saver_gate_entered.clone();

        self.accumulation_task = Some(tokio::spawn(async move {
            info!("Recording saver accumulation task started (save_audio: {})", save_audio);
            let mut first_error = None;

            while let Some(chunk) = receiver.recv().await {
                // Only process audio chunks if auto_save is enabled
                if save_audio {
                    // Add chunk to incremental saver
                    if let Some(saver_arc) = &incremental_saver_arc {
                        let mut saver_guard = saver_arc.lock().await;
                        #[cfg(test)]
                        if let Some(gate) = &test_saver_gate {
                            let should_block = test_saver_gate_entered
                                .as_ref()
                                .map(|entered| !entered.swap(true, Ordering::SeqCst))
                                .unwrap_or(true);
                            if should_block {
                                gate.wait();
                            }
                        }
                        let chunk_data = chunk.data.clone();
                        if let Err(e) = saver_guard.add_chunk(chunk) {
                            error!("Failed to add chunk to incremental saver: {}", e);
                            first_error.get_or_insert_with(|| e.to_string());
                        } else {
                            #[cfg(test)]
                            if let Some(observer) = &test_chunk_observer {
                                if let Ok(mut chunks) = observer.lock() {
                                    chunks.push(chunk_data);
                                }
                            }
                        }
                    } else {
                        error!("Incremental saver not available while accumulating");
                        first_error.get_or_insert_with(|| "Incremental saver unavailable".to_string());
                    }
                } else {
                    // auto_save is false: discard audio chunk (no-op)
                    // Transcription already happened in the pipeline before this point
                }
            }

            info!("Recording saver accumulation task ended");
            if let Ok(mut is_saving) = is_saving_clone.lock() {
                *is_saving = false;
            }
            first_error.map_or(Ok(()), Err)
        }));

        Ok(())
    }

    /// Initialize meeting folder structure and metadata
    ///
    /// # Arguments
    /// * `meeting_name` - Name of the meeting
    /// * `create_checkpoints` - Whether to create .checkpoints/ directory and IncrementalAudioSaver
    fn initialize_meeting_folder_at(
        &mut self,
        base_folder: &PathBuf,
        meeting_name: &str,
        create_checkpoints: bool,
    ) -> Result<()> {
        // Create meeting folder structure (with or without .checkpoints/ subdirectory)
        let meeting_folder = create_meeting_folder(&base_folder, meeting_name, create_checkpoints)?;

        let initialized = (|| -> Result<(
            Option<Arc<AsyncMutex<IncrementalAudioSaver>>>,
            MeetingMetadata,
        )> {
            // Only initialize incremental saver if checkpoints are needed (auto_save is true)
            let incremental_saver = if create_checkpoints {
                #[cfg(test)]
                if self.test_failure == Some(TestFailure::IncrementalSaverInitialization) {
                    return Err(anyhow::anyhow!("injected incremental saver initialization failure"));
                }
                let saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000)?;
                info!("✅ Incremental audio saver initialized for meeting: {}", meeting_name);
                Some(Arc::new(AsyncMutex::new(saver)))
            } else {
                info!("⚠️  Skipped incremental audio saver (auto-save disabled)");
                None
            };

            let metadata = MeetingMetadata {
                version: "1.0".to_string(),
                meeting_id: None,  // Will be set by backend
                meeting_name: Some(meeting_name.to_string()),
                created_at: chrono::Utc::now().to_rfc3339(),
                completed_at: None,
                duration_seconds: None,
                devices: DeviceInfo {
                    microphone: None,  // Could be enhanced to store actual device names
                    system_audio: None,
                },
                audio_file: if create_checkpoints { "audio.mp4".to_string() } else { "".to_string() },
                transcript_file: "transcripts.json".to_string(),
                sample_rate: 48000,
                status: "recording".to_string(),
            };

            // Write initial metadata.json
            self.write_metadata(&meeting_folder, &metadata)?;
            Ok((incremental_saver, metadata))
        })();

        match initialized {
            Ok((incremental_saver, metadata)) => {
                self.incremental_saver = incremental_saver;
                self.meeting_folder = Some(meeting_folder);
                self.metadata = Some(metadata);
                Ok(())
            }
            Err(error) => {
                // Do not leave a half-created meeting directory when startup
                // fails before capture is published as live.
                let _ = std::fs::remove_dir_all(&meeting_folder);
                Err(error)
            }
        }
    }

    /// Write metadata.json to disk (atomic write with temp file)
    fn write_metadata(&self, folder: &PathBuf, metadata: &MeetingMetadata) -> Result<()> {
        let metadata_path = folder.join("metadata.json");
        let temp_path = folder.join(".metadata.json.tmp");

        let json_string = serde_json::to_string_pretty(metadata)?;
        std::fs::write(&temp_path, json_string)?;
        std::fs::rename(&temp_path, &metadata_path)?;  // Atomic

        Ok(())
    }

    /// Write transcripts.json to disk (atomic write with temp file and validation)
    fn write_transcripts_json(&self, folder: &PathBuf) -> Result<()> {
        // Clone segments to avoid holding lock during I/O
        let segments_clone = if let Ok(segments) = self.transcript_segments.lock() {
            segments.clone()
        } else {
            error!("Failed to lock transcript segments for writing");
            return Err(anyhow::anyhow!("Failed to lock transcript segments"));
        };

        info!("Writing {} transcript segments to JSON", segments_clone.len());

        let transcript_path = folder.join("transcripts.json");
        let temp_path = folder.join(".transcripts.json.tmp");

        // Create JSON structure
        let json = serde_json::json!({
            "version": "1.0",
            "segments": segments_clone,
            "last_updated": chrono::Utc::now().to_rfc3339(),
            "total_segments": segments_clone.len()
        });

        // Serialize to pretty JSON string
        let json_string = serde_json::to_string_pretty(&json)
            .map_err(|e| {
                error!("Failed to serialize transcripts to JSON: {}", e);
                anyhow::anyhow!("JSON serialization failed: {}", e)
            })?;

        // Write to temp file with error handling
        std::fs::write(&temp_path, &json_string)
            .map_err(|e| {
                error!("Failed to write transcript temp file to {}: {}", temp_path.display(), e);
                anyhow::anyhow!("Failed to write temp file: {}", e)
            })?;

        // Verify temp file was written correctly
        if !temp_path.exists() {
            error!("Temp transcript file does not exist after write: {}", temp_path.display());
            return Err(anyhow::anyhow!("Temp file verification failed"));
        }

        // Atomic rename
        std::fs::rename(&temp_path, &transcript_path)
            .map_err(|e| {
                error!("Failed to rename transcript file from {} to {}: {}",
                       temp_path.display(), transcript_path.display(), e);
                anyhow::anyhow!("Failed to rename transcript file: {}", e)
            })?;

        info!("✅ Successfully wrote transcripts.json with {} segments", segments_clone.len());
        Ok(())
    }

    // in frontend/src-tauri/src/audio/recording_saver.rs
    pub fn get_stats(&self) -> (usize, u32) {
        if let Some(ref saver) = self.incremental_saver {
            if let Ok(guard) = saver.try_lock() {
                (guard.get_checkpoint_count() as usize, 48000)
            } else {
                (0, 48000)
            }
        } else {
            (0, 48000)
        }
    }

    /// Stop and save using incremental saving approach
    ///
    /// # Arguments
    /// * `app` - Tauri app handle for emitting events
    /// * `recording_duration` - Actual recording duration in seconds (from RecordingState)
    pub async fn stop_and_save<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        recording_duration: Option<f64>
    ) -> Result<Option<String>, String> {
        self.stop_and_save_inner(recording_duration, |save_event| {
            app.emit("recording-saved", save_event)
                .map_err(|error| error.to_string())
        })
        .await
    }

    async fn stop_and_save_inner<F>(
        &mut self,
        recording_duration: Option<f64>,
        emit_saved: F,
    ) -> Result<Option<String>, String>
    where
        F: FnOnce(&serde_json::Value) -> Result<(), String>,
    {
        info!("Stopping recording saver");

        // Stop accepting new work, then await the receiver task. The producer
        // side is closed by the pipeline before this method is called, so all
        // chunks already accepted by the saver channel are drained in order.
        if let Ok(mut is_saving) = self.is_saving.lock() {
            *is_saving = false;
        }

        if let Some(task) = self.accumulation_task.take() {
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    return Err(format!(
                        "Partial save: recording saver could not persist every accepted chunk: {error}; folder={}",
                        self.meeting_folder
                            .as_ref()
                            .map(|folder| folder.display().to_string())
                            .unwrap_or_else(|| "unavailable".to_string())
                    ));
                }
                Err(error) => {
                    return Err(format!(
                        "Partial save: recording saver task failed: {error}; folder={}",
                        self.meeting_folder
                            .as_ref()
                            .map(|folder| folder.display().to_string())
                            .unwrap_or_else(|| "unavailable".to_string())
                    ));
                }
            }
        }

        // Check if incremental saver exists (indicates auto_save was enabled)
        let should_save_audio = self.incremental_saver.is_some();

        if !should_save_audio {
            info!("⚠️  No audio saver initialized (auto-save was disabled) - skipping audio finalization");
            if let Some(folder) = &self.meeting_folder {
                self.write_transcripts_json(folder)
                    .map_err(|error| format!("Failed to save transcripts: {error}"))?;
                if !folder.join("transcripts.json").exists() {
                    return Err("Transcript file verification failed".to_string());
                }

                if let Some(mut metadata) = self.metadata.clone() {
                    metadata.status = "completed".to_string();
                    metadata.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    metadata.duration_seconds = recording_duration;
                    self.write_metadata(folder, &metadata)
                        .map_err(|error| format!("Failed to update metadata: {error}"))?;
                    self.metadata = Some(metadata);
                }
            }
            info!("✅ Transcripts and metadata finalized (auto-save disabled)");
            return Ok(None);
        }

        // Finalize incremental saver (merge checkpoints into final audio.mp4)
        let final_audio_path = if let Some(saver_arc) = &self.incremental_saver {
            let mut saver = saver_arc.lock().await;
            #[cfg(test)]
            if self.test_failure == Some(TestFailure::AudioFinalization) {
                return Err(self.partial_save_error_message(
                    "injected audio finalization failure",
                ));
            }
            match saver.finalize().await {
                Ok(path) => {
                    info!("✅ Successfully finalized audio: {}", path.display());
                    path
                }
                Err(e) => {
                    error!("❌ Failed to finalize incremental saver: {}", e);
                    return Err(format!("Failed to finalize audio: {}", e));
                }
            }
        } else {
            error!("No incremental saver initialized - cannot save recording");
            return Err("No incremental saver initialized".to_string());
        };

        // Save final transcripts.json with validation
        if let Some(folder) = &self.meeting_folder {
            #[cfg(test)]
            if self.test_failure == Some(TestFailure::TranscriptWrite) {
                return Err(self.partial_save_error_message(
                    "injected final transcript write failure",
                ));
            }
            if let Err(e) = self.write_transcripts_json(folder) {
                error!("❌ Failed to write final transcripts: {}", e);
                return Err(format!("Failed to save transcripts: {}", e));
            }

            // Verify transcripts were written correctly
            let transcript_path = folder.join("transcripts.json");
            if !transcript_path.exists() {
                error!("❌ Transcript file was not created at: {}", transcript_path.display());
                return Err("Transcript file verification failed".to_string());
            }
            info!("✅ Transcripts saved and verified at: {}", transcript_path.display());
        }

        // Update metadata to completed status with actual recording duration
        if let (Some(folder), Some(mut metadata)) = (&self.meeting_folder, self.metadata.clone()) {
            metadata.status = "completed".to_string();
            metadata.completed_at = Some(chrono::Utc::now().to_rfc3339());

            // Use actual recording duration from RecordingState (more accurate than transcript segments)
            // Falls back to last transcript segment if duration not provided
            metadata.duration_seconds = recording_duration.or_else(|| {
                if let Ok(segments) = self.transcript_segments.lock() {
                    segments.last().map(|seg| seg.audio_end_time)
                } else {
                    None
                }
            });

            #[cfg(test)]
            if self.test_failure == Some(TestFailure::MetadataCompletion) {
                return Err(self.partial_save_error_message(
                    "injected metadata completion failure",
                ));
            }
            if let Err(e) = self.write_metadata(folder, &metadata) {
                error!("❌ Failed to update metadata to completed: {}", e);
                return Err(format!("Failed to update metadata: {}", e));
            }

            info!("✅ Metadata updated with duration: {:?}s", metadata.duration_seconds);
        }

        // Emit save event with audio and transcript paths
        let save_event = serde_json::json!({
            "audio_file": final_audio_path.to_string_lossy(),
            "transcript_file": self.meeting_folder.as_ref()
                .map(|f| f.join("transcripts.json").to_string_lossy().to_string()),
            "meeting_name": self.meeting_name,
            "meeting_folder": self.meeting_folder.as_ref()
                .map(|f| f.to_string_lossy().to_string())
        });

        if let Err(e) = emit_saved(&save_event) {
            warn!("Failed to emit recording-saved event: {}", e);
        }

        // Clean up transcript segments
        if let Ok(mut segments) = self.transcript_segments.lock() {
            segments.clear();
        }

        Ok(Some(final_audio_path.to_string_lossy().to_string()))
    }

    #[cfg(test)]
    fn set_test_failure(&mut self, failure: TestFailure) {
        self.test_failure = Some(failure);
    }

    #[cfg(test)]
    fn set_test_saver_gate(
        &mut self,
        observer: Arc<Mutex<Vec<Vec<f32>>>>,
        gate: Arc<Barrier>,
        entered: Arc<AtomicBool>,
    ) {
        self.test_chunk_observer = Some(observer);
        self.test_saver_gate = Some(gate);
        self.test_saver_gate_entered = Some(entered);
    }

    #[cfg(test)]
    fn partial_save_error_message(&self, detail: &str) -> String {
        format!(
            "Partial save: {detail}; folder={}",
            self.meeting_folder
                .as_ref()
                .map(|folder| folder.display().to_string())
                .unwrap_or_else(|| "unavailable".to_string())
        )
    }

    /// Get the meeting folder path (for passing to backend)
    pub fn get_meeting_folder(&self) -> Option<&PathBuf> {
        self.meeting_folder.as_ref()
    }

    /// Get accumulated transcript segments (for reload sync)
    pub fn get_transcript_segments(&self) -> Vec<TranscriptSegment> {
        if let Ok(segments) = self.transcript_segments.lock() {
            segments.clone()
        } else {
            Vec::new()
        }
    }

    /// Get meeting name (for reload sync)
    pub fn get_meeting_name(&self) -> Option<String> {
        self.meeting_name.clone()
    }
}

impl Default for RecordingSaver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tempfile::tempdir;

    fn test_chunk(value: f32, chunk_id: u64) -> AudioChunk {
        AudioChunk {
            data: vec![value; 48_000],
            sample_rate: 48_000,
            timestamp: chunk_id as f64,
            chunk_id,
            device_type: super::super::recording_state::DeviceType::Microphone,
        }
    }

    fn test_segment(sequence_id: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: format!("test-segment-{sequence_id}"),
            text: format!("identified segment {sequence_id}"),
            audio_start_time: sequence_id as f64,
            audio_end_time: sequence_id as f64 + 1.0,
            duration: 1.0,
            display_time: format!("[00:0{sequence_id}]"),
            confidence: 0.99,
            sequence_id,
        }
    }

    #[test]
    fn auto_save_start_failure_rolls_back_before_capture() {
        let base = tempdir().expect("temporary recordings directory should be created");
        let mut saver = RecordingSaver::new();
        saver.set_meeting_name(Some("injected-start-failure".to_string()));
        saver.set_test_failure(TestFailure::IncrementalSaverInitialization);
        let (sender, receiver) = mpsc::unbounded_channel();

        let error = saver
            .start_accumulation_for_test(true, receiver, base.path())
            .expect_err("injected storage initialization must fail");

        assert!(error.contains("recording storage"));
        assert!(error.contains("injected incremental saver initialization"));
        assert!(saver.meeting_folder.is_none());
        assert!(saver.incremental_saver.is_none());
        assert!(saver.accumulation_task.is_none());
        assert!(!*saver.is_saving.lock().unwrap());
        assert!(sender.send(test_chunk(0.1, 0)).is_err(), "failed startup must not consume audio");
        assert!(base.path().read_dir().unwrap().next().is_none(), "startup rollback must remove the meeting folder");
    }

    #[tokio::test]
    async fn auto_save_disabled_completes_transcript_metadata_only() {
        let base = tempdir().expect("temporary recordings directory should be created");
        let mut saver = RecordingSaver::new();
        saver.set_meeting_name(Some("transcript-only-test".to_string()));
        let (sender, receiver) = mpsc::unbounded_channel();
        saver
            .start_accumulation_for_test(false, receiver, base.path())
            .expect("transcript-only saver should initialize");
        saver.add_transcript_segment(test_segment(0));
        drop(sender);

        let result = saver
            .stop_and_save_inner(Some(1.0), |_| Ok(()))
            .await
            .expect("auto-save disabled stop should succeed");
        assert!(result.is_none(), "transcript-only recording must not produce audio");
        let folder = saver.meeting_folder.as_ref().expect("meeting folder should remain available");
        let transcripts: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(folder.join("transcripts.json"))
                .expect("transcript sidecar should exist"),
        )
        .expect("transcript sidecar should be valid JSON");
        assert_eq!(transcripts["total_segments"], serde_json::json!(1));
        let metadata: MeetingMetadata = serde_json::from_str(
            &std::fs::read_to_string(folder.join("metadata.json"))
                .expect("metadata sidecar should exist"),
        )
        .expect("metadata sidecar should be valid JSON");
        assert_eq!(metadata.status, "completed", "transcript-only metadata should finalize successfully");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stop_drains_blocked_identifiable_chunks_once_in_order_before_finalization() {
        let base = tempdir().expect("temporary recordings directory should be created");
        let mut saver = RecordingSaver::new();
        saver.set_meeting_name(Some("blocked-drain-test".to_string()));
        let (sender, receiver) = mpsc::unbounded_channel();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let gate = Arc::new(Barrier::new(2));
        let entered = Arc::new(AtomicBool::new(false));
        saver.set_test_saver_gate(observed.clone(), gate.clone(), entered.clone());
        saver
            .start_accumulation_for_test(true, receiver, base.path())
            .expect("auto-save should initialize");
        saver.add_transcript_segment(test_segment(0));

        let chunks = vec![test_chunk(0.11, 1), test_chunk(0.22, 2), test_chunk(0.33, 3)];
        for chunk in &chunks {
            sender.send(chunk.clone()).expect("chunk should be accepted");
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while !entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("saver must reach the deliberate blocked point");

        drop(sender); // Stop closes the producer side while the saver is blocked.
        let stop_counter = Arc::new(AtomicUsize::new(0));
        let stop_counter_for_task = stop_counter.clone();
        let stop_task = tokio::spawn(async move {
            saver
                .stop_and_save_inner(Some(3.0), move |_| {
                    stop_counter_for_task.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!stop_task.is_finished(), "Stop must await the blocked saver instead of timing out or dropping its tail");

        tokio::task::spawn_blocking(move || gate.wait())
            .await
            .expect("saver release task panicked"); // Release the real saver; it must then drain the remaining accepted chunks.
        let final_audio = tokio::time::timeout(Duration::from_secs(10), stop_task)
            .await
            .expect("stop must finish after releasing the saver")
            .expect("stop task panicked")
            .expect("stop should finalize all accepted chunks")
            .expect("auto-save should return the final audio path");
        assert_eq!(stop_counter.load(Ordering::SeqCst), 1, "successful stop emits one save event");
        assert!(PathBuf::from(&final_audio).exists(), "finalized audio should exist");

        let observed = observed.lock().unwrap();
        assert_eq!(observed.len(), chunks.len(), "every accepted chunk must be persisted exactly once");
        for (actual, expected) in observed.iter().zip(chunks.iter()) {
            assert_eq!(actual, &expected.data, "persisted chunks must retain acceptance order and samples");
        }
    }

    #[tokio::test]
    async fn stop_propagates_each_finalization_failure_without_success_event() {
        for failure in [
            TestFailure::AudioFinalization,
            TestFailure::TranscriptWrite,
            TestFailure::MetadataCompletion,
        ] {
            let base = tempdir().expect("temporary recordings directory should be created");
            let mut saver = RecordingSaver::new();
            saver.set_meeting_name(Some(format!("injected-stop-failure-{failure:?}")));
            saver.set_test_failure(failure);
            let (sender, receiver) = mpsc::unbounded_channel();
            saver
                .start_accumulation_for_test(true, receiver, base.path())
                .expect("auto-save should initialize");
            saver.add_transcript_segment(test_segment(0));
            sender.send(test_chunk(0.44, 9)).expect("chunk should be accepted");
            drop(sender);

            let save_events = Arc::new(AtomicUsize::new(0));
            let save_events_for_test = save_events.clone();
            let error = saver
                .stop_and_save_inner(Some(1.0), move |_| {
                    save_events_for_test.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .await
                .expect_err("injected persistence failure must reach the caller");
            assert!(error.starts_with("Partial save:"), "failure should be classified as partial save: {error}");
            assert!(error.contains("folder="), "partial save should carry recovery folder: {error}");
            assert_eq!(save_events.load(Ordering::SeqCst), 0, "persistence failure must not emit success");

            let folder = saver.meeting_folder.as_ref().expect("partial save retains meeting folder");
            if failure == TestFailure::AudioFinalization {
                assert!(!folder.join("audio.mp4").exists(), "audio failure occurs before final output");
            }
            if failure == TestFailure::TranscriptWrite {
                assert!(folder.join("audio.mp4").exists(), "transcript failure follows audio finalization");
            }
        }
    }
}
