use std::sync::Arc;
use tokio::sync::mpsc;
use anyhow::Result;
use log::{debug, error, info, warn};
#[cfg(target_os = "macos")]
use std::time::Duration;
#[cfg(target_os = "macos")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::devices::AudioDevice;

use super::recording_state::{RecordingState, AudioChunk};
use super::pipeline::AudioPipelineManager;
use super::stream::AudioStreamManager;
use super::recording_saver::RecordingSaver;
use super::device_monitor::{AudioDeviceMonitor, DeviceEvent};

/// Stream manager type enumeration
pub enum StreamManagerType {
    Standard(AudioStreamManager),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RecordingStartError {
    #[error("Failed to initialize speech recognition: {0}")]
    TranscriptionRuntime(#[source] anyhow::Error),
    #[error("Cannot save the recording: {0}")]
    Storage(#[source] anyhow::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

// ============================================================================
// macOS Core Audio "pre-wake" — ported from feat/audio-device-handling
// (commit 8bf6af8b, with refinements from d266feac + 7107adc7 + a10f8e55).
// ============================================================================
//
// When a macOS Mac has a Bluetooth device as the active audio output, the
// built-in audio hardware unit — and, crucially, the BT audio connection
// itself — can sit in a low-power state where registered IO procs do not
// fire even though `AudioDeviceStart` returned `noErr`. The symptom is a
// 60–90 second "silent" period at recording start (or during a disconnect
// fallback) where the mic is technically capturing but no samples reach
// the pipeline, until something nudges Core Audio back to life.
//
// The workaround, empirically validated by Sujith in the audio-device-
// handling branch: briefly play 300 ms of digital silence through the
// output device by name. The name-based enumeration is itself part of the
// fix ("enumeration wake" — it forces the HAL to bring the device out of
// its idle power state). The subsequent `stream.play()` on the output
// activates the hardware unit fully, at which point the built-in mic's
// IO proc starts firing normally. Total cost: ~300 ms added to recording
// start, ~150 ms added to the disconnect-fallback hot-swap path.

#[cfg(target_os = "macos")]
const AUDIO_WAKE_DURATION: Duration = Duration::from_millis(300);

#[cfg(target_os = "macos")]
const AUDIO_WAKE_SWAP_DURATION: Duration = Duration::from_millis(150);

/// Synchronous implementation of the wake. Must be called from a blocking
/// thread (not directly from async code) because it sleeps 300 ms and does
/// sync cpal I/O. The async wrapper below routes to it via spawn_blocking.
#[cfg(target_os = "macos")]
fn wake_audio_connection_sync(speaker_device_name: &str) -> Result<()> {
    info!("[AUDIO_WAKE] Waking audio via speaker: '{}'", speaker_device_name);

    let host = cpal::default_host();

    let output_device = host.output_devices()?
        .find(|d| d.name().ok().as_deref() == Some(speaker_device_name))
        .ok_or_else(|| anyhow::anyhow!("Output device '{}' not found", speaker_device_name))?;

    let config = output_device.default_output_config()?;
    info!("[AUDIO_WAKE] Output config: {} Hz, {} channels",
          config.sample_rate().0, config.channels());

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            output_device.build_output_stream(
                &config.into(),
                |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for sample in data.iter_mut() { *sample = 0.0; }
                },
                |err| error!("[AUDIO_WAKE] Output stream error: {}", err),
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            // BT HFP mode frequently picks I16 for the output config — without
            // this branch the wake would error out exactly when we need it most.
            output_device.build_output_stream(
                &config.into(),
                |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    for sample in data.iter_mut() { *sample = 0; }
                },
                |err| error!("[AUDIO_WAKE] Output stream error: {}", err),
                None,
            )?
        }
        format => {
            return Err(anyhow::anyhow!("Unsupported sample format: {:?}", format));
        }
    };

    stream.play()?;
    info!("[AUDIO_WAKE] Playing silence for {} ms to wake audio connection...", AUDIO_WAKE_DURATION.as_millis());
    std::thread::sleep(AUDIO_WAKE_DURATION);
    drop(stream);
    info!("[AUDIO_WAKE] Audio wake completed");

    Ok(())
}

/// Async wrapper around `wake_audio_connection_sync`. Used at recording start
/// so the 300 ms sleep doesn't block the tokio runtime. Non-fatal — the caller
/// should log the error and proceed if this fails; recording will still work,
/// it just may have the 60-90s silent startup on BT.
#[cfg(target_os = "macos")]
pub(super) async fn wake_audio_connection(speaker_device_name: &str) -> Result<()> {
    let name = speaker_device_name.to_string();
    tokio::task::spawn_blocking(move || {
        wake_audio_connection_sync(&name)
    }).await.map_err(|e| anyhow::anyhow!("Join error: {}", e))?
}

/// Public async wake for use by the disconnect-fallback hot-swap path in
/// `recording_commands.rs::trigger_mic_fallback_to_default`.
///
/// Differences from the start-time wake:
///   - Shorter 150 ms sleep to minimize fallback latency (the window of no
///     audio between primary death and secondary-taking-over)
///   - Falls back to the system default output if the named device isn't
///     found (e.g. the original BT output just disappeared, and we're
///     waking the MacBook speakers instead)
///   - Still handles both F32 and I16 sample formats for robustness
#[cfg(target_os = "macos")]
pub async fn wake_audio_connection_for_swap(speaker_device_name: &str) -> Result<()> {
    let name = speaker_device_name.to_string();
    tokio::task::spawn_blocking(move || {
        info!("[AUDIO_WAKE] Hot-swap wake via speaker: '{}'", name);
        let host = cpal::default_host();
        // Try the specified device first, fall back to default output. During
        // a BT disconnect the original output device name may no longer
        // enumerate, so the fallback is what actually runs in practice.
        let (output_device, config) = host.output_devices()?
            .find(|d| d.name().ok().as_deref() == Some(&*name))
            .and_then(|d| d.default_output_config().ok().map(|c| (d, c)))
            .or_else(|| {
                info!("[AUDIO_WAKE] '{}' not found or config failed — using default output device", name);
                host.default_output_device()
                    .and_then(|d| d.default_output_config().ok().map(|c| (d, c)))
            })
            .ok_or_else(|| anyhow::anyhow!("No output device available for wake"))?;
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                output_device.build_output_stream(
                    &config.into(),
                    |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        for sample in data.iter_mut() { *sample = 0.0; }
                    },
                    |err| error!("[AUDIO_WAKE] Hot-swap wake error: {}", err),
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                output_device.build_output_stream(
                    &config.into(),
                    |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        for sample in data.iter_mut() { *sample = 0; }
                    },
                    |err| error!("[AUDIO_WAKE] Hot-swap wake error: {}", err),
                    None,
                )?
            }
            format => {
                return Err(anyhow::anyhow!("Unsupported sample format for wake: {:?}", format));
            }
        };
        stream.play()?;
        std::thread::sleep(AUDIO_WAKE_SWAP_DURATION);
        drop(stream);
        info!("[AUDIO_WAKE] Hot-swap wake completed");
        Ok(())
    }).await.map_err(|e| anyhow::anyhow!("Join error: {}", e))?
}

/// Values the stop command reports after `stop_streams_and_force_flush`,
/// which wipes the live state they come from.
struct StoppedSnapshot {
    total_duration: Option<f64>,
    active_duration: Option<f64>,
    pause_duration: f64,
    had_fatal_error: bool,
    stats: super::recording_state::RecordingStats,
    microphone_name: Option<String>,
    system_name: Option<String>,
}

/// Simplified recording manager that coordinates all audio components
pub struct RecordingManager {
    state: Arc<RecordingState>,
    stream_manager: AudioStreamManager,
    pipeline_manager: AudioPipelineManager,
    recording_saver: RecordingSaver,
    device_monitor: Option<AudioDeviceMonitor>,
    device_event_receiver: Option<mpsc::UnboundedReceiver<DeviceEvent>>,
    stopped: Option<StoppedSnapshot>,
}

// SAFETY: RecordingManager contains types that we've marked as Send
unsafe impl Send for RecordingManager {}

impl RecordingManager {
    /// Create a new recording manager
    pub fn new() -> Self {
        let state = RecordingState::new();
        let stream_manager = AudioStreamManager::new(state.clone());
        let pipeline_manager = AudioPipelineManager::new();
        let (device_monitor, device_event_receiver) = AudioDeviceMonitor::new();

        Self {
            state,
            stream_manager,
            pipeline_manager,
            recording_saver: RecordingSaver::new(),
            device_monitor: Some(device_monitor),
            device_event_receiver: Some(device_event_receiver),
            stopped: None,
        }
    }

    // Remove app handle storage for now - will be passed directly when saving

    /// Start recording with specified devices
    ///
    /// On macOS, the command entry points wake audio and verify microphone
    /// access before constructing the manager and calling this method.
    ///
    /// # Arguments
    /// * `microphone_device` - Optional microphone device to use
    /// * `system_device` - Optional system audio device to use
    /// * `auto_save` - Whether to save audio checkpoints (true) or just transcripts/metadata (false)
    /// * `realtime_transcription_enabled` - When false the pipeline skips VAD and
    ///   speaker attribution entirely; mixing and recording are unaffected.
    pub(crate) async fn start_recording(
        &mut self,
        microphone_device: Option<Arc<AudioDevice>>,
        system_device: Option<Arc<AudioDevice>>,
        auto_save: bool,
        realtime_transcription_enabled: bool,
        streaming_live: bool,
    ) -> std::result::Result<mpsc::UnboundedReceiver<AudioChunk>, RecordingStartError> {
        info!(
            "Starting recording manager (auto_save: {}, realtime_transcription: {})",
            auto_save, realtime_transcription_enabled
        );

        // Set up transcription channel
        let (transcription_sender, transcription_receiver) = mpsc::unbounded_channel::<AudioChunk>();
        let (recording_sender, recording_receiver) = mpsc::unbounded_channel::<AudioChunk>();

        // Required persistence comes first: if the folder or audio writer
        // cannot be created, fail before any capture or pipeline exists.
        self.recording_saver
            .prepare_storage(auto_save)
            .map_err(RecordingStartError::Storage)?;

        // Start recording state
        self.state.start_recording()?;

        // Get device information for adaptive mixing
        // The pipeline uses device kind (Bluetooth vs Wired) to apply adaptive buffering:
        // - Bluetooth: Larger buffers (80-200ms) to handle jitter
        // - Wired: Smaller buffers (20-50ms) for low latency
        let (mic_name, mic_kind) = if let Some(ref mic) = microphone_device {
            let device_kind = super::device_detection::InputDeviceKind::detect(&mic.name, 512, 48000);
            (mic.name.clone(), device_kind)
        } else {
            ("No Microphone".to_string(), super::device_detection::InputDeviceKind::Unknown)
        };

        let (sys_name, sys_kind) = if let Some(ref sys) = system_device {
            let device_kind = super::device_detection::InputDeviceKind::detect(&sys.name, 512, 48000);
            (sys.name.clone(), device_kind)
        } else {
            ("No System Audio".to_string(), super::device_detection::InputDeviceKind::Unknown)
        };

        // Start the audio processing pipeline with FFmpeg adaptive mixer
        // Pipeline will: 1) Mix mic+system audio with adaptive buffering, 2) Send mixed to recording_sender,
        // 3) Apply VAD and send speech segments to transcription
        if let Err(error) = self.pipeline_manager.start(
            self.state.clone(),
            transcription_sender,
            0, // Ignored - using dynamic sizing internally
            48000, // 48kHz sample rate
            // Pre-mixed audio destination. Withheld when auto-save is off so the
            // pipeline skips mixing; the saver's drain then ends immediately.
            auto_save.then_some(recording_sender),
            mic_name,
            mic_kind,
            sys_name,
            sys_kind,
            realtime_transcription_enabled,
            streaming_live,
        ) {
            self.state.stop_recording();
            self.recording_saver.discard_after_failed_start().await;
            return Err(RecordingStartError::TranscriptionRuntime(error));
        }

        self.recording_saver.start_accumulation(auto_save, recording_receiver);
        self.recording_saver.set_device_info(
            microphone_device.as_ref().map(|d| d.name.clone()),
            system_device.as_ref().map(|d| d.name.clone())
        );

        // Give the pipeline a moment to fully initialize before starting streams
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Start audio streams - they send RAW unmixed chunks to pipeline for mixing
        // Pipeline handles mixing and distribution to both recording and transcription
        if let Err(error) = self
            .stream_manager
            .start_streams(microphone_device.clone(), system_device.clone(), None)
            .await
        {
            self.abort_failed_start().await;
            return Err(error.into());
        }

        // Start device monitoring to detect disconnects
        if let Some(ref mut monitor) = self.device_monitor {
            #[cfg(target_os = "linux")]
            let monitored = (self.state.get_microphone_device(), self.state.get_system_device());
            #[cfg(not(target_os = "linux"))]
            let monitored = (microphone_device, system_device);
            if let Err(e) = monitor.start_monitoring(monitored.0, monitored.1) {
                warn!("Failed to start device monitoring: {}", e);
                // Non-fatal - continue without monitoring
            } else {
                info!("✅ Device monitoring started");
            }
        }

        info!("Recording manager started successfully with {} active streams",
               self.stream_manager.active_stream_count());

        Ok(transcription_receiver)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn request_capture_finish(&self) { self.stream_manager.request_finish(); }

    #[cfg(target_os = "linux")]
    pub(super) fn finish_role(&self, microphone: bool) -> bool { self.stream_manager.finish_role(microphone) }

    /// Stop recording streams without saving (for use when waiting for transcription)
    pub async fn stop_streams_only(&mut self) -> Result<()> {
        info!("Stopping recording streams only");

        // Stop device monitoring
        if let Some(ref mut monitor) = self.device_monitor {
            monitor.stop_monitoring().await;
        }

        // Stop recording state first
        #[cfg(not(target_os = "linux"))]
        self.state.stop_recording();

        // Stop audio streams
        let stream_result = self.stream_manager.stop_streams().await;
        #[cfg(target_os = "linux")]
        self.state.stop_recording();
        if let Err(ref e) = stream_result {
            error!("Error stopping audio streams: {}", e);
        }

        // Stop audio pipeline
        if let Err(e) = self.pipeline_manager.stop().await {
            error!("Error stopping audio pipeline: {}", e);
        }

        debug!("Recording streams stopped successfully");
        Ok(())
    }

    /// Undo a start that failed after the pipeline and writer were created:
    /// stop capture, close the pipeline (dropping its recording sender) and
    /// await both tasks, so no orphan task or stream outlives the error and a
    /// later start begins from a clean state.
    async fn abort_failed_start(&mut self) {
        warn!("Recording start failed; stopping the partially started pipeline");
        self.state.stop_recording();
        if let Err(e) = self.stream_manager.stop_streams().await {
            warn!("Error stopping streams after failed start: {}", e);
        }
        if let Err(e) = self.pipeline_manager.stop().await {
            warn!("Error stopping pipeline after failed start: {}", e);
        }
        self.recording_saver.discard_after_failed_start().await;
        self.state.cleanup();
    }

    /// Stop streams and force immediate pipeline flush to process all accumulated audio
    pub async fn stop_streams_and_force_flush(&mut self) -> Result<()> {
        info!("🚀 Stopping recording streams with IMMEDIATE pipeline flush");

        // CRITICAL: Stop device monitor FIRST to prevent continuous WASAPI polling on Windows
        // This fixes the slow shutdown issue where device enumeration runs for 90+ seconds
        if let Some(ref mut monitor) = self.device_monitor {
            info!("Stopping device monitor first...");
            monitor.stop_monitoring().await;
        }

        // Taken before stop_recording, which ends any open pause and clears
        // the devices, and before cleanup, which clears everything else.
        self.stopped = Some(StoppedSnapshot {
            total_duration: self.state.get_recording_duration(),
            active_duration: self.state.get_active_recording_duration(),
            pause_duration: self.state.get_total_pause_duration()
                + self.state.get_current_pause_duration().unwrap_or(0.0),
            had_fatal_error: self.state.has_fatal_error(),
            stats: self.state.get_stats(),
            microphone_name: self.state.get_microphone_device().map(|d| d.name.clone()),
            system_name: self.state.get_system_device().map(|d| d.name.clone()),
        });

        // Stop recording state first - this clears device references
        #[cfg(not(target_os = "linux"))]
        self.state.stop_recording();

        // Stop audio streams immediately
        let stream_result = self.stream_manager.stop_streams().await;
        #[cfg(target_os = "linux")]
        self.state.stop_recording();
        if let Err(ref e) = stream_result {
            error!("Error stopping audio streams: {}", e);
        }

        // Dropping the sender lets the pipeline drain, mix the partial window
        // and flush the VAD before it exits.
        if let Err(e) = self.pipeline_manager.stop().await {
            error!("Error stopping pipeline: {}", e);
        }

        // CRITICAL: Full cleanup to release all Arc references and resources
        // This ensures microphone is released even if Drop is delayed
        self.state.cleanup();

        info!("✅ Recording streams stopped with immediate flush completed");
        #[cfg(target_os = "linux")]
        return stream_result;
        #[cfg(not(target_os = "linux"))]
        Ok(())
    }

    /// Save recording after transcription is complete
    pub async fn save_recording_only<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()> {
        debug!("Saving recording with transcript chunks");

        let recording_duration = self.get_active_recording_duration();
        info!("Recording duration: {:?}s", recording_duration);

        // Save the recording with actual duration. Failures are returned so
        // the stop command can tell the user; it still completes shutdown.
        match self.recording_saver.stop_and_save(app, recording_duration).await {
            Ok(Some(file_path)) => {
                info!("Recording saved successfully to: {}", file_path);
            }
            Ok(None) => {
                debug!("Recording not saved (auto-save disabled or no audio data)");
            }
            Err(e) => {
                error!("Failed to save recording: {}", e);
                return Err(anyhow::anyhow!(e));
            }
        }

        debug!("Recording save operation completed");
        Ok(())
    }

    /// Stop recording and save audio (legacy method)
    pub async fn stop_recording<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()> {
        info!("Stopping recording manager");

        // Get recording duration BEFORE stopping (important!)
        let recording_duration = self.state.get_active_recording_duration();
        info!("Recording duration before stop: {:?}s", recording_duration);

        // Stop recording state first
        #[cfg(not(target_os = "linux"))]
        self.state.stop_recording();

        // Stop audio streams
        let stream_result = self.stream_manager.stop_streams().await;
        #[cfg(target_os = "linux")]
        self.state.stop_recording();
        if let Err(ref e) = stream_result {
            error!("Error stopping audio streams: {}", e);
        }

        // Stop audio pipeline
        if let Err(e) = self.pipeline_manager.stop().await {
            error!("Error stopping audio pipeline: {}", e);
        }

        // Save the recording with actual duration
        let saved = match self.recording_saver.stop_and_save(app, recording_duration).await {
            Ok(Some(file_path)) => {
                info!("Recording saved successfully to: {}", file_path);
                Ok(())
            }
            Ok(None) => {
                info!("Recording not saved (auto-save disabled or no audio data)");
                Ok(())
            }
            Err(e) => {
                error!("Failed to save recording: {}", e);
                Err(anyhow::anyhow!(e))
            }
        };

        info!("Recording manager stopped");
        saved
    }

    /// Get recording stats from the saver
    pub fn get_recording_stats(&self) -> (usize, u32) {
        self.recording_saver.get_stats()
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.state.is_recording()
    }

    /// Pause the current recording session
    pub fn pause_recording(&self) -> Result<()> {
        info!("Pausing recording");
        self.state.pause_recording()
    }

    /// Resume the current recording session
    pub fn resume_recording(&self) -> Result<()> {
        info!("Resuming recording");
        self.state.resume_recording()
    }

    /// Check if recording is currently paused
    pub fn is_paused(&self) -> bool {
        self.state.is_paused()
    }

    /// Check if recording is active (recording and not paused)
    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    /// Get recording statistics
    pub fn get_stats(&self) -> super::recording_state::RecordingStats {
        match &self.stopped {
            Some(s) => s.stats.clone(),
            None => self.state.get_stats(),
        }
    }

    /// Get recording duration
    pub fn get_recording_duration(&self) -> Option<f64> {
        match &self.stopped {
            Some(s) => s.total_duration,
            None => self.state.get_recording_duration(),
        }
    }

    /// Get active recording duration (excluding pauses)
    pub fn get_active_recording_duration(&self) -> Option<f64> {
        match &self.stopped {
            Some(s) => s.active_duration,
            None => self.state.get_active_recording_duration(),
        }
    }

    /// Get total pause duration
    pub fn get_total_pause_duration(&self) -> f64 {
        match &self.stopped {
            Some(s) => s.pause_duration,
            None => self.state.get_total_pause_duration(),
        }
    }

    /// Microphone and system device names
    pub fn get_device_names(&self) -> (Option<String>, Option<String>) {
        match &self.stopped {
            Some(s) => (s.microphone_name.clone(), s.system_name.clone()),
            None => (
                self.state.get_microphone_device().map(|d| d.name.clone()),
                self.state.get_system_device().map(|d| d.name.clone()),
            ),
        }
    }

    /// Get current pause duration if paused
    pub fn get_current_pause_duration(&self) -> Option<f64> {
        self.state.get_current_pause_duration()
    }

    /// Get error information
    pub fn get_error_info(&self) -> (u32, Option<super::recording_state::AudioError>) {
        (self.state.get_error_count(), self.state.get_last_error())
    }

    /// Get active stream count
    pub fn active_stream_count(&self) -> usize {
        self.stream_manager.active_stream_count()
    }

    /// Set error callback for handling errors
    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&super::recording_state::AudioError, bool) + Send + Sync + 'static,
    {
        self.state.set_error_callback(callback);
    }

    /// Check if there's a fatal error
    pub fn has_fatal_error(&self) -> bool {
        match &self.stopped {
            Some(s) => s.had_fatal_error,
            None => self.state.has_fatal_error(),
        }
    }

    /// Set the meeting name for this recording session
    pub fn set_meeting_name(&mut self, name: Option<String>) {
        self.recording_saver.set_meeting_name(name);
    }

    /// Set the base folder meeting directories are created under (from the
    /// `save_folder` recording preference).
    pub fn set_save_folder(&mut self, folder: Option<std::path::PathBuf>) {
        self.recording_saver.set_save_folder(folder);
    }

    /// Add a structured transcript segment to be saved later
    pub fn transcript_sink(&self) -> super::recording_saver::TranscriptSink {
        self.recording_saver.transcript_sink()
    }

    pub fn add_transcript_segment(&self, segment: super::recording_saver::TranscriptSegment) {
        self.recording_saver.add_transcript_segment(segment);
    }

    /// Add a transcript chunk to be saved later (legacy method)
    pub fn add_transcript_chunk(&self, text: String) {
        self.recording_saver.add_transcript_chunk(text);
    }

    /// Get accumulated transcript segments from current recording session
    /// Used for syncing frontend state after page reload during active recording
    pub fn get_transcript_segments(&self) -> Vec<super::recording_saver::TranscriptSegment> {
        self.recording_saver.get_transcript_segments()
    }

    /// Get meeting name from current recording session
    /// Used for syncing frontend state after page reload during active recording
    pub fn get_meeting_name(&self) -> Option<String> {
        self.recording_saver.get_meeting_name()
    }

    /// Cleanup all resources without saving
    pub async fn cleanup_without_save(&mut self) {
        if self.is_recording() {
            debug!("Stopping recording without saving during cleanup");

            // Stop recording state first
            self.state.stop_recording();

            // Stop audio streams
            if let Err(e) = self.stream_manager.stop_streams().await {
                error!("Error stopping audio streams during cleanup: {}", e);
            }

            // Stop audio pipeline
            if let Err(e) = self.pipeline_manager.stop().await {
                error!("Error stopping audio pipeline during cleanup: {}", e);
            }
        }
        self.state.cleanup();
    }

    /// Get the meeting folder path (if available)
    /// Returns None if no meeting name was set or folder structure not initialized
    pub fn get_meeting_folder(&self) -> Option<std::path::PathBuf> {
        self.recording_saver.get_meeting_folder().map(|p| p.clone())
    }

    pub fn audio_saving_enabled(&self) -> bool {
        self.recording_saver.audio_saving_enabled()
    }

    /// Take ownership of the device event receiver for use by a background processor.
    pub fn take_device_event_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<DeviceEvent>> {
        self.device_event_receiver.take()
    }

    /// Take the mic stream OUT for hot-swap (Phase 1) so the caller can tear it
    /// down without holding RECORDING_MANAGER. System audio continues uninterrupted.
    pub fn take_mic_stream_for_swap(&mut self) -> Option<super::stream::AudioStream> {
        self.stream_manager.take_mic_stream()
    }

    /// Set new mic stream and update device state after hot-swap (Phase 3).
    pub fn set_mic_stream_after_swap(
        &mut self,
        stream: super::stream::AudioStream,
        device: Arc<AudioDevice>,
        system_name: Option<String>,
    ) {
        self.stream_manager.set_mic_stream(stream);
        // Notify the device monitor so it tracks the new mic (and updated
        // system output) instead of the dead ones (M8 fix). BT devices like
        // AirPods are often both mic AND speaker — when they disconnect, both
        // monitor entries go stale. `system_name` is the *current* default
        // output, resolved lock-free in Phase 2 by the caller (do_mic_swap)
        // so a CoreAudio stall can't happen under the RECORDING_MANAGER lock;
        // `state.get_system_device()` still holds the pre-swap
        // reference (e.g. AirPods) because we don't mutate system_device
        // during a mic-only hot-swap, which would leave the monitor tracking
        // the dead name and spamming "missing for N checks".
        if let Some(ref monitor) = self.device_monitor {
            #[cfg(not(target_os = "linux"))]
            monitor.notify_mic_swapped(device.name.clone(), system_name);
            #[cfg(target_os = "linux")]
            { let _ = system_name; monitor.notify_mic_swapped(device.name.clone(), None); }
        }
        self.state.set_microphone_device(device);
        // The disconnect that triggered this swap is resolved; its errors must
        // not count toward stopping a later, unrelated episode.
        self.state.note_recovery();
    }

    #[cfg(target_os = "linux")]
    pub fn take_system_stream_for_swap(&mut self) -> Option<super::stream::AudioStream> {
        self.stream_manager.take_system_stream()
    }
    #[cfg(target_os = "linux")]
    pub fn set_system_stream_after_swap(&mut self, stream: super::stream::AudioStream, device: Arc<AudioDevice>) {
        self.stream_manager.set_system_stream(stream);
        self.state.set_system_device(device.clone());
        if let Some(monitor) = &self.device_monitor {
            monitor.notify_mic_swapped(String::new(), Some(device.name.clone()));
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn linux_stream_kind(&self, microphone: bool) -> Option<super::stream::LinuxStreamKind> {
        self.stream_manager.linux_stream_kind(microphone)
    }

    #[cfg(target_os = "linux")]
    pub fn has_working_stream(&self, microphone: bool) -> bool {
        self.stream_manager.has_working_stream(microphone)
    }

    #[cfg(target_os = "linux")]
    pub fn has_working_streams(&self) -> bool { self.stream_manager.has_working_streams() }

    /// Get reference to recording state for external access
    pub fn get_state(&self) -> &Arc<RecordingState> {
        &self.state
    }
}

impl Default for RecordingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RecordingManager {
    fn drop(&mut self) {
        // Note: Can't call async cleanup in Drop, but streams have their own Drop implementations
        self.state.cleanup();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod wake_tests {
    use super::*;

    #[test]
    fn wake_fails_gracefully_for_unknown_device() {
        // Nonexistent device name fails at the `find` step — no audio hardware needed.
        // Asserts the wake is non-fatal (returns Err, does not panic).
        assert!(wake_audio_connection_sync("__no_such_device__").is_err());
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;

    /// The stop command reads duration (for metadata.json and analytics)
    /// only after `stop_streams_and_force_flush` has returned, so the values
    /// must survive that call.
    #[tokio::test]
    async fn duration_survives_stop_streams_and_force_flush() {
        let mut manager = RecordingManager::new();
        manager.state.start_recording().unwrap();
        manager.state.set_microphone_device(Arc::new(AudioDevice::new(
            "Test Mic".to_string(),
            super::super::devices::DeviceType::Input,
        )));
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(manager.get_active_recording_duration().is_some());

        manager.stop_streams_and_force_flush().await.unwrap();

        let active = manager.get_active_recording_duration();
        assert!(matches!(active, Some(d) if d >= 0.03), "active duration after stop: {:?}", active);
        let total = manager.get_recording_duration();
        assert!(matches!(total, Some(d) if d >= 0.03), "total duration after stop: {:?}", total);
        assert_eq!(manager.get_device_names(), (Some("Test Mic".to_string()), None));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_shutdown_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use super::super::{stream::linux_clock_tests::test_stream, recording_state::DeviceType};

    #[tokio::test]
    async fn stop_keeps_acceptance_open_until_both_workers_finish() {
        let mut manager = RecordingManager::new();
        manager.device_monitor = None;
        manager.state.start_recording().unwrap();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        manager.state.set_audio_sender(sender);
        let mic_finish = Arc::new(AtomicBool::new(false));
        let sys_finish = Arc::new(AtomicBool::new(false));
        for microphone in [true, false] {
            let finish = if microphone { mic_finish.clone() } else { sys_finish.clone() };
            let both = (mic_finish.clone(), sys_finish.clone());
            let state = manager.state.clone();
            let worker_finish = finish.clone();
            let task = tokio::spawn(async move {
                while !worker_finish.load(Ordering::Acquire) { tokio::task::yield_now().await; }
                assert!(both.0.load(Ordering::Acquire) && both.1.load(Ordering::Acquire));
                assert!(state.is_recording());
                state.send_audio_chunk(AudioChunk { data: vec![1.0], sample_rate: 48000,
                    timestamp: 0.0, chunk_id: 0, device_type: DeviceType::Microphone }).unwrap();
            });
            let stream = test_stream(if microphone { "mic" } else { "system" }, finish, task);
            if microphone { manager.stream_manager.set_mic_stream(stream); }
            else { manager.stream_manager.set_system_stream(stream); }
        }
        manager.stop_streams_and_force_flush().await.unwrap();
        assert!(receiver.recv().await.is_some());
        assert!(receiver.recv().await.is_some());
        assert!(receiver.recv().await.is_none());
        assert!(!manager.state.is_recording());
    }
}
