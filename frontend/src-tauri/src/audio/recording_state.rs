use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;
use anyhow::Result;

use super::devices::AudioDevice;

/// Device type for audio chunks
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceType {
    Microphone,
    System,
}

/// Audio chunk with metadata for processing
#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// For VAD transcription chunks: which source dominated the mixed audio
    /// (drives Me/Others speaker labels). None for raw capture, recording,
    /// and flush chunks — and when neither source clearly dominated.
    pub dominant_source: Option<DeviceType>,
    /// Raw capture chunks only: seconds of audio the device did not deliver
    /// between the previous callback and this one (WASAPI loopback sends no
    /// packets while nothing plays). `None` on a capture's first callback,
    /// where there is no previous one, and on every non-capture chunk. The
    /// mixer pads the gap with silence so both streams stay time-aligned.
    pub capture_gap: Option<f64>,
    pub data: Vec<f32>,
    pub sample_rate: u32,
    /// Raw capture chunks: capture time of the chunk's last sample, in
    /// seconds since recording start. Other chunks: their position in the
    /// recording.
    pub timestamp: f64,
    pub chunk_id: u64,
    pub device_type: DeviceType,
}

/// Processed audio chunk (post-VAD) for recording
#[derive(Debug, Clone)]
pub struct ProcessedAudioChunk {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp: f64,
    pub device_type: DeviceType,
}

/// Comprehensive error types for audio system
#[derive(Debug, Clone)]
pub enum AudioError {
    DeviceDisconnected,
    StreamFailed,
    #[cfg(target_os = "linux")]
    LinuxCapture(String),
    ProcessingFailed,
    TranscriptionFailed,
    ChannelClosed,
    InitializationFailed,
    ConfigurationError,
    PermissionDenied,
    BufferOverflow,
    SampleRateUnsupported,
}

impl AudioError {
    /// Check if error is recoverable (can attempt reconnection)
    pub fn is_recoverable(&self) -> bool {
        match self {
            // Device disconnect is now recoverable - we can attempt reconnection
            AudioError::DeviceDisconnected => true,
            AudioError::StreamFailed => true,
            #[cfg(target_os = "linux")]
            AudioError::LinuxCapture(_) => true,
            AudioError::ProcessingFailed => true,
            AudioError::TranscriptionFailed => true,
            AudioError::ChannelClosed => false,
            AudioError::InitializationFailed => false,
            AudioError::ConfigurationError => false,
            AudioError::PermissionDenied => false,
            AudioError::BufferOverflow => true,
            AudioError::SampleRateUnsupported => false,
        }
    }

    /// Get user-friendly error message
    pub fn user_message(&self) -> &str {
        match self {
            AudioError::DeviceDisconnected => "Audio device was disconnected",
            AudioError::StreamFailed => "Audio stream encountered an error",
            #[cfg(target_os = "linux")]
            AudioError::LinuxCapture(message) => message,
            AudioError::ProcessingFailed => "Audio processing failed",
            AudioError::TranscriptionFailed => "Speech transcription failed",
            AudioError::ChannelClosed => "Audio channel was closed unexpectedly",
            AudioError::InitializationFailed => "Failed to initialize audio system",
            AudioError::ConfigurationError => "Audio configuration error",
            AudioError::PermissionDenied => "Microphone permission denied",
            AudioError::BufferOverflow => "Audio buffer overflow",
            AudioError::SampleRateUnsupported => "Audio sample rate not supported",
        }
    }
}

/// Payload of the `recording-error` event. The frontend mirrors this shape in
/// `src/lib/recording-error.ts`; change both together.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RecordingErrorPayload {
    pub message: String,
    pub recoverable: bool,
    /// True on the one event whose error made the backend stop recording.
    /// The frontend runs the normal stop flow when it sees this.
    pub recording_stopped: bool,
}

impl RecordingErrorPayload {
    pub fn new(error: &AudioError, recording_stopped: bool) -> Self {
        Self {
            message: error.user_message().to_string(),
            recoverable: error.is_recoverable(),
            recording_stopped,
        }
    }
}

/// Recoverable errors further apart than this belong to separate episodes:
/// the count restarts instead of accumulating over the whole session.
pub const RECOVERABLE_ERROR_QUIET_PERIOD: std::time::Duration = std::time::Duration::from_secs(60);

/// Error callback: the error, and whether it stopped the recording.
type ErrorCallback = Box<dyn Fn(&AudioError, bool) + Send + Sync>;

/// Recording statistics
#[derive(Debug, Default)]
pub struct RecordingStats {
    pub chunks_processed: u64,
    pub total_duration: f64,
    pub last_activity: Option<Instant>,
}

/// Unified state management for audio recording
pub struct RecordingState {
    #[cfg(target_os = "linux")]
    pub(crate) capture_cancel: Mutex<Arc<AtomicBool>>,
    // Core recording state
    is_recording: AtomicBool,
    is_paused: AtomicBool,

    // Audio devices
    microphone_device: Mutex<Option<Arc<AudioDevice>>>,
    system_device: Mutex<Option<Arc<AudioDevice>>>,

    // Audio pipeline
    audio_sender: Mutex<Option<mpsc::UnboundedSender<AudioChunk>>>,

    // Error handling
    error_count: AtomicU32,
    recoverable_error_count: AtomicU32,
    last_error: Mutex<Option<AudioError>>,
    error_callback: Mutex<Option<ErrorCallback>>,
    /// When the last recoverable error was reported, as nanoseconds since
    /// `epoch`; `NOT_RECORDING` = none yet. Drives the quiet-period reset.
    last_recoverable_error_nanos: AtomicU64,

    // Statistics
    stats: Mutex<RecordingStats>,
    /// Counted on the realtime audio thread (~100 chunks/s per device), so it
    /// lives outside `stats` — taking that mutex per chunk on a realtime thread
    /// is a priority-inversion risk for a number nothing reads synchronously.
    chunks_processed: AtomicU64,

    // Recording start time for accurate timestamps
    recording_start: Mutex<Option<Instant>>,
    /// Lock-free mirror of `recording_start` for the audio callback, which
    /// timestamps every chunk. Nanoseconds since `epoch`; `u64::MAX` = not
    /// recording. An `Instant` is not atomic, hence the offset-from-epoch form.
    recording_start_nanos: AtomicU64,
    /// Fixed reference point for `recording_start_nanos`.
    epoch: Instant,
    // Pause time tracking
    pause_start: Mutex<Option<Instant>>,
    total_pause_duration: Mutex<std::time::Duration>,
}

/// Sentinel for `recording_start_nanos` meaning "not recording".
const NOT_RECORDING: u64 = u64::MAX;

impl RecordingState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            #[cfg(target_os = "linux")]
            capture_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            is_recording: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            microphone_device: Mutex::new(None),
            system_device: Mutex::new(None),
            audio_sender: Mutex::new(None),
            error_count: AtomicU32::new(0),
            recoverable_error_count: AtomicU32::new(0),
            last_error: Mutex::new(None),
            error_callback: Mutex::new(None),
            last_recoverable_error_nanos: AtomicU64::new(NOT_RECORDING),
            stats: Mutex::new(RecordingStats::default()),
            chunks_processed: AtomicU64::new(0),
            recording_start: Mutex::new(None),
            recording_start_nanos: AtomicU64::new(NOT_RECORDING),
            epoch: Instant::now(),
            pause_start: Mutex::new(None),
            total_pause_duration: Mutex::new(std::time::Duration::ZERO),
        })
    }

    // Recording control
    pub fn start_recording(&self) -> Result<()> {
        self.is_recording.store(true, Ordering::SeqCst);
        let now = Instant::now();
        *self.recording_start.lock().unwrap() = Some(now);
        self.recording_start_nanos.store(
            now.duration_since(self.epoch).as_nanos() as u64,
            Ordering::Relaxed,
        );
        self.chunks_processed.store(0, Ordering::Relaxed);
        self.error_count.store(0, Ordering::SeqCst);
        self.recoverable_error_count.store(0, Ordering::SeqCst);
        self.last_recoverable_error_nanos.store(NOT_RECORDING, Ordering::SeqCst);
        *self.last_error.lock().unwrap() = None;
        Ok(())
    }

    pub fn stop_recording(&self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.is_paused.store(false, Ordering::SeqCst);
        // Clear pause tracking when stopping
        *self.pause_start.lock().unwrap() = None;
        // CRITICAL: Clear audio sender to close the pipeline channel
        // This ensures the pipeline loop exits properly after processing all chunks
        *self.audio_sender.lock().unwrap() = None;
        // CRITICAL: Clear device references to release microphone/speaker
        // Without this, Arc<AudioDevice> references persist and keep the mic active
        *self.microphone_device.lock().unwrap() = None;
        *self.system_device.lock().unwrap() = None;
        log::info!("Recording stopped, device references cleared");
    }

    pub fn pause_recording(&self) -> Result<()> {
        if !self.is_recording() {
            return Err(anyhow::anyhow!("Cannot pause when not recording"));
        }
        if self.is_paused() {
            return Err(anyhow::anyhow!("Recording is already paused"));
        }

        self.is_paused.store(true, Ordering::SeqCst);
        *self.pause_start.lock().unwrap() = Some(Instant::now());
        log::info!("Recording paused");
        Ok(())
    }

    pub fn resume_recording(&self) -> Result<()> {
        if !self.is_recording() {
            return Err(anyhow::anyhow!("Cannot resume when not recording"));
        }
        if !self.is_paused() {
            return Err(anyhow::anyhow!("Recording is not paused"));
        }

        // Calculate pause duration and add to total
        if let Some(pause_start) = self.pause_start.lock().unwrap().take() {
            let pause_duration = pause_start.elapsed();
            *self.total_pause_duration.lock().unwrap() += pause_duration;
            log::info!("Recording resumed after pause of {:.2}s", pause_duration.as_secs_f64());
        }

        self.is_paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        self.is_recording() && !self.is_paused()
    }

    // Device management
    pub fn set_microphone_device(&self, device: Arc<AudioDevice>) {
        *self.microphone_device.lock().unwrap() = Some(device);
    }

    pub fn set_system_device(&self, device: Arc<AudioDevice>) {
        *self.system_device.lock().unwrap() = Some(device);
    }

    pub fn get_microphone_device(&self) -> Option<Arc<AudioDevice>> {
        self.microphone_device.lock().unwrap().clone()
    }

    pub fn get_system_device(&self) -> Option<Arc<AudioDevice>> {
        self.system_device.lock().unwrap().clone()
    }

    // Audio pipeline management
    pub fn set_audio_sender(&self, sender: mpsc::UnboundedSender<AudioChunk>) {
        *self.audio_sender.lock().unwrap() = Some(sender);
    }

    pub fn send_audio_chunk(&self, chunk: AudioChunk) -> Result<()> {
        // Don't send audio chunks when paused
        if self.is_paused() {
            return Ok(()); // Silently discard chunks while paused
        }

        if let Some(sender) = self.audio_sender.lock().unwrap().as_ref() {
            sender.send(chunk).map_err(|_| anyhow::anyhow!("Failed to send audio chunk"))?;

            // Counter only: the stats mutex and an Instant::now() per chunk used
            // to run here, on the realtime audio thread, for a `last_activity`
            // field no caller reads. get_stats() folds this back in.
            self.chunks_processed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        } else {
            // Return an error when no sender is available (pipeline not ready)
            Err(anyhow::anyhow!("Audio pipeline not ready - no sender available"))
        }
    }

    // Error handling
    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&AudioError, bool) + Send + Sync + 'static,
    {
        *self.error_callback.lock().unwrap() = Some(Box::new(callback));
    }

    /// End the current error episode: a stream was successfully rebuilt
    /// (mic hot-swap), so earlier recoverable errors no longer count toward
    /// the stop thresholds.
    pub fn note_recovery(&self) {
        let had = self.recoverable_error_count.swap(0, Ordering::SeqCst);
        self.error_count.store(0, Ordering::SeqCst);
        if had > 0 {
            log::info!("Audio recovered; cleared {} recoverable error(s)", had);
        }
    }

    pub fn report_error(&self, error: AudioError) {
        self.report_error_at(error, self.epoch.elapsed().as_nanos() as u64);
    }

    /// `report_error` with an explicit clock (nanoseconds since `epoch`), so
    /// the quiet-period reset is testable.
    fn report_error_at(&self, error: AudioError, now_nanos: u64) {
        let was_recording = self.is_recording();

        #[cfg(target_os = "linux")]
        let supervisor_managed = matches!(&error, AudioError::LinuxCapture(_));
        #[cfg(not(target_os = "linux"))]
        let supervisor_managed = false;

        let count = if supervisor_managed {
            // Native recovery and total-loss shutdown belong to the Linux supervisor.
            log::warn!("Supervisor-managed audio error: {:?}", error);
            0
        } else {
            // Track recoverable vs non-recoverable errors separately
            if error.is_recoverable() {
                // A recoverable error long after the previous one starts a new
                // episode. Both counters restart: the total below is in practice
                // a recoverable count too (non-recoverable errors stop at once),
                // so leaving it to accumulate would reintroduce the session-wide
                // limit through the back door.
                let last = self.last_recoverable_error_nanos.swap(now_nanos, Ordering::SeqCst);
                if last != NOT_RECORDING
                    && now_nanos.saturating_sub(last) > RECOVERABLE_ERROR_QUIET_PERIOD.as_nanos() as u64
                {
                    self.recoverable_error_count.store(0, Ordering::SeqCst);
                    self.error_count.store(0, Ordering::SeqCst);
                }
            }

            let count = self.error_count.fetch_add(1, Ordering::SeqCst) + 1;

            if error.is_recoverable() {
                let recoverable_count = self.recoverable_error_count.fetch_add(1, Ordering::SeqCst) + 1;
                log::warn!("Recoverable audio error ({}): {:?}", recoverable_count, error);

                // Allow more recoverable errors before stopping
                if recoverable_count >= 10 {
                    log::error!("Too many recoverable errors ({}), stopping recording", recoverable_count);
                    self.stop_recording();
                }
            } else {
                log::error!("Non-recoverable audio error: {:?}", error);
                // Stop immediately for non-recoverable errors
                self.stop_recording();
            }
            count
        };

        // Fallback: stop recording after too many total errors
        if count >= 15 {
            log::error!("Too many total audio errors ({}), stopping recording", count);
            self.stop_recording();
        }

        *self.last_error.lock().unwrap() = Some(error.clone());

        // Tell the callback whether *this* error ended the recording, so the
        // frontend runs its stop flow exactly once.
        let stopped = was_recording && !self.is_recording();
        if let Some(callback) = self.error_callback.lock().unwrap().as_ref() {
            callback(&error, stopped);
        }
    }

    pub fn get_error_count(&self) -> u32 {
        self.error_count.load(Ordering::SeqCst)
    }

    pub fn get_recoverable_error_count(&self) -> u32 {
        self.recoverable_error_count.load(Ordering::SeqCst)
    }

    pub fn get_last_error(&self) -> Option<AudioError> {
        self.last_error.lock().unwrap().clone()
    }

    pub fn has_fatal_error(&self) -> bool {
        if let Some(error) = &*self.last_error.lock().unwrap() {
            !error.is_recoverable() && self.error_count.load(Ordering::SeqCst) > 0
        } else {
            false
        }
    }

    // Statistics
    pub fn get_stats(&self) -> RecordingStats {
        let mut stats = self.stats.lock().unwrap().clone();
        stats.chunks_processed = self.chunks_processed.load(Ordering::Relaxed);
        stats
    }

    /// Wall-clock seconds since recording started.
    ///
    /// Called once per audio callback to timestamp chunks (~200/s across both
    /// devices), so it reads an atomic rather than taking the `recording_start`
    /// mutex on a realtime thread.
    pub fn get_recording_duration(&self) -> Option<f64> {
        let start = self.recording_start_nanos.load(Ordering::Relaxed);
        if start == NOT_RECORDING {
            return None;
        }
        let now = self.epoch.elapsed().as_nanos() as u64;
        Some(now.saturating_sub(start) as f64 / 1_000_000_000.0)
    }

    pub fn get_active_recording_duration(&self) -> Option<f64> {
        self.recording_start.lock().unwrap().map(|start| {
            let total_duration = start.elapsed().as_secs_f64();
            let pause_duration = self.get_total_pause_duration();
            let current_pause = if self.is_paused() {
                self.pause_start
                    .lock()
                    .unwrap()
                    .map(|p| p.elapsed().as_secs_f64())
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            total_duration - pause_duration - current_pause
        })
    }

    pub fn get_total_pause_duration(&self) -> f64 {
        self.total_pause_duration.lock().unwrap().as_secs_f64()
    }

    pub fn get_current_pause_duration(&self) -> Option<f64> {
        if self.is_paused() {
            self.pause_start
                .lock()
                .unwrap()
                .map(|start| start.elapsed().as_secs_f64())
        } else {
            None
        }
    }

    // Cleanup
    pub fn cleanup(&self) {
        self.stop_recording();
        *self.microphone_device.lock().unwrap() = None;
        *self.system_device.lock().unwrap() = None;
        *self.audio_sender.lock().unwrap() = None;
        *self.last_error.lock().unwrap() = None;
        *self.error_callback.lock().unwrap() = None;
        *self.stats.lock().unwrap() = RecordingStats::default();
        self.chunks_processed.store(0, Ordering::Relaxed);
        *self.recording_start.lock().unwrap() = None;
        self.recording_start_nanos
            .store(NOT_RECORDING, Ordering::Relaxed);
        *self.pause_start.lock().unwrap() = None;
        *self.total_pause_duration.lock().unwrap() = std::time::Duration::ZERO;
        self.error_count.store(0, Ordering::SeqCst);
        self.recoverable_error_count.store(0, Ordering::SeqCst);
        self.last_recoverable_error_nanos.store(NOT_RECORDING, Ordering::SeqCst);
    }
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            capture_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            is_recording: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            microphone_device: Mutex::new(None),
            system_device: Mutex::new(None),
            audio_sender: Mutex::new(None),
            error_count: AtomicU32::new(0),
            recoverable_error_count: AtomicU32::new(0),
            last_error: Mutex::new(None),
            error_callback: Mutex::new(None),
            last_recoverable_error_nanos: AtomicU64::new(NOT_RECORDING),
            stats: Mutex::new(RecordingStats::default()),
            chunks_processed: AtomicU64::new(0),
            recording_start: Mutex::new(None),
            recording_start_nanos: AtomicU64::new(NOT_RECORDING),
            epoch: Instant::now(),
            pause_start: Mutex::new(None),
            total_pause_duration: Mutex::new(std::time::Duration::ZERO),
        }
    }
}

// Thread-safe cloning for RecordingStats
impl Clone for RecordingStats {
    fn clone(&self) -> Self {
        Self {
            chunks_processed: self.chunks_processed,
            total_duration: self.total_duration,
            last_activity: self.last_activity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    const SEC: u64 = 1_000_000_000;

    /// Records every `stopped` flag the error callback is handed.
    fn capture_callback(state: &RecordingState) -> Arc<StdMutex<Vec<bool>>> {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let sink = seen.clone();
        state.set_error_callback(move |_error, stopped| sink.lock().unwrap().push(stopped));
        seen
    }

    /// The threshold is intentional: ten recoverable errors with no recovery
    /// and no quiet period between them still stop the session, and the
    /// callback is told so on exactly the error that stopped it.
    #[test]
    fn burst_of_recoverable_errors_stops_and_reports_the_stop() {
        let state = RecordingState::new();
        let seen = capture_callback(&state);
        state.start_recording().unwrap();
        for i in 0..10 {
            state.report_error_at(AudioError::DeviceDisconnected, i * SEC);
        }
        assert!(!state.is_recording());
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 10);
        assert_eq!(seen.iter().filter(|s| **s).count(), 1);
        assert!(seen[9]);
    }

    /// H1-F3: a successful hot-swap ends the error episode, so recoverable
    /// errors from separate disconnects do not add up across the session.
    #[test]
    fn successful_recovery_resets_recoverable_count() {
        let state = RecordingState::new();
        state.start_recording().unwrap();
        let mut t = 0;
        for _ in 0..3 {
            for _ in 0..9 {
                state.report_error_at(AudioError::DeviceDisconnected, t);
                t += SEC;
            }
            state.note_recovery();
        }
        // 27 errors in total: neither the recoverable (10) nor the total (15)
        // fallback may fire.
        assert!(state.is_recording());
    }

    /// H1-F3: a quiet period since the previous recoverable error also starts
    /// a fresh count.
    #[test]
    fn quiet_period_resets_recoverable_count() {
        let state = RecordingState::new();
        state.start_recording().unwrap();
        for i in 0..9 {
            state.report_error_at(AudioError::StreamFailed, i * SEC);
        }
        let later = 8 * SEC + RECOVERABLE_ERROR_QUIET_PERIOD.as_nanos() as u64 + SEC;
        for i in 0..9 {
            state.report_error_at(AudioError::StreamFailed, later + i * SEC);
        }
        assert!(state.is_recording());
        assert_eq!(state.get_recoverable_error_count(), 9);
    }

    #[test]
    fn non_recoverable_error_stops_immediately_and_reports_the_stop() {
        let state = RecordingState::new();
        let seen = capture_callback(&state);
        state.start_recording().unwrap();
        state.report_error_at(AudioError::PermissionDenied, 0);
        assert!(!state.is_recording());
        assert_eq!(*seen.lock().unwrap(), vec![true]);
    }

    /// The `recording-error` payload shape is a contract with the frontend
    /// (frontend/src/lib/recording-error.ts and its test use this literal).
    #[test]
    fn recording_error_payload_json_shape() {
        let payload = RecordingErrorPayload::new(&AudioError::DeviceDisconnected, true);
        assert_eq!(
            serde_json::to_string(&payload).unwrap(),
            r#"{"message":"Audio device was disconnected","recoverable":true,"recording_stopped":true}"#
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_capture_error_tests {
    use super::*;

    fn audio_chunk() -> AudioChunk {
        AudioChunk {
            data: vec![0.25, -0.25],
            sample_rate: 48_000,
            timestamp: 1.0,
            chunk_id: 1,
            device_type: DeviceType::Microphone,
        }
    }

    #[test]
    fn native_errors_preserve_recording_audio_and_diagnostics() {
        let state = RecordingState::new();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        state.set_audio_sender(sender);
        state.start_recording().unwrap();
        let callbacks = Arc::new(AtomicU32::new(0));
        let callback_count = callbacks.clone();
        state.set_error_callback(move |error, _stopped| {
            assert!(matches!(error, AudioError::LinuxCapture(_)));
            callback_count.fetch_add(1, Ordering::SeqCst);
        });

        for index in 0..20 {
            let message = format!("Native capture failed {index}");
            state.report_error(AudioError::LinuxCapture(message.clone()));

            assert!(state.is_active());
            assert_eq!(state.get_error_count(), 0);
            assert_eq!(state.get_recoverable_error_count(), 0);
            assert!(!state.has_fatal_error());
            assert!(matches!(
                state.get_last_error(),
                Some(AudioError::LinuxCapture(actual)) if actual == message
            ));
            assert_eq!(callbacks.load(Ordering::SeqCst), index + 1);
            state.send_audio_chunk(audio_chunk()).unwrap();
            assert_eq!(receiver.try_recv().unwrap().data, vec![0.25, -0.25]);
        }
    }

    #[test]
    fn native_errors_do_not_consume_or_reset_ordinary_error_budget() {
        let state = RecordingState::new();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        state.set_audio_sender(sender);
        state.start_recording().unwrap();
        let callbacks = Arc::new(AtomicU32::new(0));
        let callback_count = callbacks.clone();
        state.set_error_callback(move |_, _| {
            callback_count.fetch_add(1, Ordering::SeqCst);
        });

        for count in 1..=9 {
            state.report_error(AudioError::ProcessingFailed);
            state.report_error(AudioError::LinuxCapture("Native capture failed".to_owned()));

            assert!(state.is_active());
            assert_eq!(state.get_error_count(), count);
            assert_eq!(state.get_recoverable_error_count(), count);
            assert_eq!(callbacks.load(Ordering::SeqCst), count * 2);
            assert!(matches!(
                state.get_last_error(),
                Some(AudioError::LinuxCapture(_))
            ));
            state.send_audio_chunk(audio_chunk()).unwrap();
            assert_eq!(receiver.try_recv().unwrap().data, vec![0.25, -0.25]);
        }

        state.report_error(AudioError::ProcessingFailed);

        assert!(!state.is_recording());
        assert_eq!(state.get_error_count(), 10);
        assert_eq!(state.get_recoverable_error_count(), 10);
        assert_eq!(callbacks.load(Ordering::SeqCst), 19);
        assert!(matches!(
            state.get_last_error(),
            Some(AudioError::ProcessingFailed)
        ));
        assert!(state.send_audio_chunk(audio_chunk()).is_err());
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }
}
