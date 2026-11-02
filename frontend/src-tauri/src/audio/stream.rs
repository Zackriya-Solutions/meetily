use std::sync::Arc;
use anyhow::Result;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, Stream, SupportedStreamConfig};
use log::{error, info, warn};
use tokio::sync::mpsc;

use super::devices::{AudioDevice, get_device_and_config};
use super::pipeline::AudioCapture;
use super::recording_state::{RecordingState, DeviceType};
use super::capture::{AudioCaptureBackend, get_current_backend};

#[cfg(target_os = "macos")]
use super::capture::CoreAudioCapture;

#[cfg(target_os = "linux")]
use super::capture::{find_sink_by_name, find_source_by_name, list_pulse_sources, PulseCapture};
#[cfg(target_os = "linux")]
use super::devices::discovery::linux_device_query;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicBool;

/// Stream backend implementation
pub enum StreamBackend {
    /// CPAL-based stream (ScreenCaptureKit or default)
    Cpal(Stream),
    /// Core Audio direct implementation (macOS only)
    #[cfg(target_os = "macos")]
    CoreAudio {
        task: Option<tokio::task::JoinHandle<()>>,
    },
    /// Native PipeWire/PulseAudio implementation (Linux only)
    #[cfg(target_os = "linux")]
    Pulse {
        should_stop: Arc<AtomicBool>,
        should_finish: Arc<AtomicBool>,
        worker_error: Arc<std::sync::Mutex<Option<String>>>,
        task: Option<tokio::task::JoinHandle<()>>,
    },
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LinuxStreamKind {
    Pulse,
    Cpal,
}

// SAFETY: While Stream doesn't implement Send, we ensure it's only accessed
// from the same thread context by using spawn_blocking for operations that cross thread boundaries
unsafe impl Send for StreamBackend {}

/// Simplified audio stream wrapper with multi-backend support
pub struct AudioStream {
    device: Arc<AudioDevice>,
    backend: StreamBackend,
    #[cfg(target_os = "linux")]
    delivery: Option<Arc<AtomicBool>>,
}

// SAFETY: AudioStream contains StreamBackend which we've marked as Send
unsafe impl Send for AudioStream {}

impl AudioStream {
    /// Create a new audio stream for the given device
    pub async fn create(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
    ) -> Result<Self> {
        // Get current backend from global config
        let backend_type = get_current_backend();
        Self::create_with_backend(device, state, device_type, recording_sender, backend_type).await
    }

    /// Create a new audio stream with explicit backend selection
    pub async fn create_with_backend(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
        backend_type: AudioCaptureBackend,
    ) -> Result<Self> {
        info!("🎵 Stream: Creating audio stream for device: {} with backend: {:?}, device_type: {:?}",
              device.name, backend_type, device_type);

        // For system audio devices, use the selected backend.
        // For microphone devices, prefer CPAL, except on Linux where we first
        // try the native PulseAudio/PipeWire source path (see below).
        #[cfg(target_os = "macos")]
        let use_core_audio = device_type == DeviceType::System
            && backend_type == AudioCaptureBackend::CoreAudio;

        #[cfg(not(target_os = "macos"))]
        let use_core_audio = false;

        #[cfg(target_os = "macos")]
        info!("🎵 Stream: use_core_audio = {}, device_type == System: {}, backend == CoreAudio: {}",
              use_core_audio,
              device_type == DeviceType::System,
              backend_type == AudioCaptureBackend::CoreAudio);

        #[cfg(not(target_os = "macos"))]
        info!("🎵 Stream: use_core_audio = {}, device_type == System: {}",
              use_core_audio,
              device_type == DeviceType::System);

        #[cfg(target_os = "macos")]
        if use_core_audio {
            info!("🎵 Stream: Using Core Audio backend (cidre) for system audio");
            return Self::create_core_audio_stream(device, state, device_type, recording_sender).await;
        }

        // Linux system audio always goes through the native PipeWire/PulseAudio
        // path instead of cpal's ALSA host — no backend toggle needed, unlike
        // macOS's ScreenCaptureKit/CoreAudio choice, since there's only one way
        // to capture system audio natively on Linux.
        #[cfg(target_os = "linux")]
        if device_type == DeviceType::System {
            info!("🎵 Stream: Using native PulseAudio/PipeWire backend for system audio");
            return Self::create_pulse_stream(device, state, device_type, recording_sender, true).await;
        }

        // Linux microphones are identified by their exact PulseAudio source
        // name. CPAL is only a degraded mode for an unreachable Pulse server:
        // a reachable server that doesn't know the ID is a hard error, never
        // a reinterpretation of the ID as an ALSA name.
        #[cfg(target_os = "linux")]
        if device_type == DeviceType::Microphone {
            match linux_device_query(|| Ok(list_pulse_sources())).await? {
                Ok(_) => {
                    return Self::create_pulse_mic_stream(
                        device,
                        state,
                        device_type,
                        recording_sender,
                        true,
                    )
                    .await;
                }
                Err(e) => warn!(
                    "🎤 Stream: PulseAudio/PipeWire server unreachable ({}), falling back to CPAL",
                    e
                ),
            }
        }

        // Default path: use CPAL
        #[cfg(target_os = "macos")]
        let backend_name = if backend_type == AudioCaptureBackend::ScreenCaptureKit {
            "ScreenCaptureKit"
        } else {
            "CPAL (default)"
        };

        #[cfg(not(target_os = "macos"))]
        let backend_name = "CPAL";

        info!("🎵 Stream: Using CPAL backend ({}) for device: {}", backend_name, device.name);
        Self::create_cpal_stream(device, state, device_type, recording_sender).await
    }

    /// Create a CPAL-based stream (ScreenCaptureKit on macOS)
    async fn create_cpal_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
    ) -> Result<Self> {
        info!("Creating CPAL stream for device: {}", device.name);

        // Get the underlying cpal device and config
        let (cpal_device, config) = get_device_and_config(&device).await?;

        info!("Audio config - Sample rate: {}, Channels: {}, Format: {:?}",
              config.sample_rate().0, config.channels(), config.sample_format());

        // Create audio capture processor
        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            config.sample_rate().0,
            config.channels(),
            device_type,
            recording_sender,
        );

        // Build the appropriate stream based on sample format
        let stream = Self::build_stream(&cpal_device, &config, capture.clone())?;

        // Start the stream
        stream.play()?;
        info!("CPAL stream started for device: {}", device.name);

        Ok(Self {
            device,
            backend: StreamBackend::Cpal(stream),
            #[cfg(target_os = "linux")]
            delivery: None,
        })
    }

    /// Create a Core Audio stream (macOS only)
    #[cfg(target_os = "macos")]
    async fn create_core_audio_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
    ) -> Result<Self> {
        info!("🔊 Stream: Creating Core Audio stream for device: {}", device.name);

        // Create Core Audio capture
        info!("🔊 Stream: Calling CoreAudioCapture::new()...");
        let capture_impl = CoreAudioCapture::new()
            .map_err(|e| {
                error!("❌ Stream: CoreAudioCapture::new() failed: {}", e);
                anyhow::anyhow!("Failed to create Core Audio capture: {}", e)
            })?;

        info!("✅ Stream: CoreAudioCapture created, calling stream()...");
        let core_stream = capture_impl.stream()
            .map_err(|e| {
                error!("❌ Stream: capture_impl.stream() failed: {}", e);
                anyhow::anyhow!("Failed to create Core Audio stream: {}", e)
            })?;

        let sample_rate = core_stream.sample_rate();
        info!("✅ Stream: Core Audio stream created with sample rate: {} Hz", sample_rate);

        // Create audio capture processor for pipeline integration
        // CRITICAL: Core Audio tap is MONO (with_mono_global_tap_excluding_processes)
        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            sample_rate,
            1, // Core Audio tap is MONO (not stereo!)
            device_type,
            recording_sender,
        );

        // Spawn task to process Core Audio stream samples
        // The stream needs to be polled continuously to produce samples
        let device_name = device.name.clone();
        info!("🔊 Stream: Spawning tokio task to poll Core Audio stream...");
        let task = tokio::spawn({
            let capture = capture.clone();
            let mut stream = core_stream;

            async move {
                use futures_util::StreamExt;

                let mut buffer = Vec::new();
                let mut frame_count = 0;
                let frames_per_chunk = 1024; // Process in chunks of 1024 samples

                info!("✅ Stream: Core Audio processing task started for {}", device_name);

                let mut _sample_count = 0u64;
                while let Some(sample) = stream.next().await {
                    _sample_count += 1;
                    // if _sample_count % 48000 == 0 {
                    //     info!("📊 Stream: Received {} samples from Core Audio stream", _sample_count);
                    // }

                    buffer.push(sample);
                    frame_count += 1;

                    // Process when we have enough samples
                    if frame_count >= frames_per_chunk {
                        capture.process_audio_data(&buffer);
                        buffer.clear();
                        frame_count = 0;
                    }
                }

                // Process any remaining samples
                if !buffer.is_empty() {
                    capture.process_audio_data(&buffer);
                }

                info!("⚠️ Stream: Core Audio processing task ended for {}", device_name);
            }
        });

        info!("✅ Stream: Core Audio stream fully initialized for device: {}", device.name);

        Ok(Self {
            device: device.clone(),
            backend: StreamBackend::CoreAudio {
                task: Some(task),
            },
        })
    }

    /// Create a native PulseAudio/PipeWire stream (Linux only)
    #[cfg(target_os = "linux")]
    async fn create_pulse_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
        delivering: bool,
    ) -> Result<Self> {
        info!("🔊 Stream: Creating PulseAudio stream for device: {}", device.name);

        let name = device.name.clone();
        let sink = linux_device_query(move || find_sink_by_name(&name)).await?;

        let capture_impl = PulseCapture::new_system(&sink.monitor_source_name)
            .map_err(|e| anyhow::anyhow!("Failed to open PulseAudio record stream: {}", e))?;

        let sample_rate = capture_impl.sample_rate();
        let channels = capture_impl.channels();
        let should_stop = capture_impl.stop_handle();
        let should_finish = capture_impl.finish_handle();

        // Create audio capture processor for pipeline integration
        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            sample_rate,
            channels,
            device_type,
            recording_sender,
        );

        let delivery = Arc::new(AtomicBool::new(delivering));
        let worker_delivery = delivery.clone();
        let session_cancel = state.capture_cancel.lock().unwrap().clone();
        let worker_stop = should_stop.clone();
        let worker_error = Arc::new(std::sync::Mutex::new(None));
        let worker_error_out = worker_error.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let device_name = device.name.clone();
        let task = tokio::task::spawn_blocking(move || {
            info!("✅ Stream: PulseAudio capture thread started for {}", device_name);
            let epoch = std::cell::Cell::new(0.0);
            if let Err(e) = capture_impl.run_timed_ready(|samples, first_frame| {
                if worker_delivery.load(std::sync::atomic::Ordering::Acquire) && !state.is_paused() {
                    let timestamp = linux_sample_timestamp(epoch.get(), first_frame, state.get_total_pause_duration());
                    capture.process_audio_data_at(samples, timestamp);
                }
            }, || {
                epoch.set(state.get_recording_duration().unwrap_or(0.0));
                let _ = ready_tx.send(());
            }) {
                warn!("Linux capture failed: {}", e);
                *worker_error_out.lock().unwrap() = Some(e.to_string());
                if !state.capture_cancel.lock().unwrap().load(std::sync::atomic::Ordering::Acquire) {
                    state.report_error(super::recording_state::AudioError::LinuxCapture(e.to_string()));
                }
            }
            info!("⚠️ Stream: PulseAudio capture thread ended for {}", device_name);
        });

        let mut ready_rx = ready_rx;
        loop {
            tokio::select! {
                ready = &mut ready_rx => {
                    if ready.is_err() {
                        let _ = task.await;
                        return Err(anyhow::anyhow!(worker_error.lock().unwrap().clone().unwrap_or_else(|| "Audio startup cancelled".into())));
                    }
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                    if session_cancel.load(std::sync::atomic::Ordering::Acquire) {
                        worker_stop.store(true, std::sync::atomic::Ordering::Release);
                        let _ = task.await;
                        return Err(anyhow::anyhow!("Audio startup cancelled"));
                    }
                }
            }
        }
        info!("✅ Stream: PulseAudio stream fully initialized for device: {}", device.name);

        Ok(Self {
            device: device.clone(),
            delivery: Some(delivery),
            backend: StreamBackend::Pulse {
                should_finish,
                worker_error,
                should_stop,
                task: Some(task),
            },
        })
    }

    /// Create a native PulseAudio/PipeWire microphone stream (Linux only)
    #[cfg(target_os = "linux")]
    async fn create_pulse_mic_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
        delivering: bool,
    ) -> Result<Self> {
        info!(
            "🔊 Stream: Creating PulseAudio microphone stream for device: {}",
            device.name
        );

        let name = device.name.clone();
        let source = linux_device_query(move || find_source_by_name(&name)).await?;

        let capture_impl = PulseCapture::new_microphone(&source.source_name)
            .map_err(|e| anyhow::anyhow!("Failed to open PulseAudio record stream: {}", e))?;

        let sample_rate = capture_impl.sample_rate();
        let channels = capture_impl.channels();
        let should_stop = capture_impl.stop_handle();
        let should_finish = capture_impl.finish_handle();

        // Create audio capture processor for pipeline integration
        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            sample_rate,
            channels,
            device_type,
            recording_sender,
        );

        let delivery = Arc::new(AtomicBool::new(delivering));
        let worker_delivery = delivery.clone();
        let session_cancel = state.capture_cancel.lock().unwrap().clone();
        let worker_stop = should_stop.clone();
        let worker_error = Arc::new(std::sync::Mutex::new(None));
        let worker_error_out = worker_error.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let device_name = device.name.clone();
        let task = tokio::task::spawn_blocking(move || {
            info!(
                "✅ Stream: PulseAudio microphone capture thread started for {}",
                device_name
            );
            let epoch = std::cell::Cell::new(0.0);
            if let Err(e) = capture_impl.run_timed_ready(|samples, first_frame| {
                if worker_delivery.load(std::sync::atomic::Ordering::Acquire) && !state.is_paused() {
                    let timestamp = linux_sample_timestamp(epoch.get(), first_frame, state.get_total_pause_duration());
                    capture.process_audio_data_at(samples, timestamp);
                }
            }, || {
                epoch.set(state.get_recording_duration().unwrap_or(0.0));
                let _ = ready_tx.send(());
            }) {
                warn!("Linux capture failed: {}", e);
                *worker_error_out.lock().unwrap() = Some(e.to_string());
                if !state.capture_cancel.lock().unwrap().load(std::sync::atomic::Ordering::Acquire) {
                    state.report_error(super::recording_state::AudioError::LinuxCapture(e.to_string()));
                }
            }
            info!(
                "⚠️ Stream: PulseAudio microphone capture thread ended for {}",
                device_name
            );
        });

        info!(
            "✅ Stream: PulseAudio microphone stream fully initialized for device: {}",
            device.name
        );

        let mut ready_rx = ready_rx;
        loop {
            tokio::select! {
                ready = &mut ready_rx => {
                    if ready.is_err() {
                        let _ = task.await;
                        return Err(anyhow::anyhow!(worker_error.lock().unwrap().clone().unwrap_or_else(|| "Audio startup cancelled".into())));
                    }
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                    if session_cancel.load(std::sync::atomic::Ordering::Acquire) {
                        worker_stop.store(true, std::sync::atomic::Ordering::Release);
                        let _ = task.await;
                        return Err(anyhow::anyhow!("Audio startup cancelled"));
                    }
                }
            }
        }
        Ok(Self {
            device: device.clone(),
            delivery: Some(delivery),
            backend: StreamBackend::Pulse {
                should_finish,
                worker_error,
                should_stop,
                task: Some(task),
            },
        })
    }

    /// Build stream based on sample format
    fn build_stream(
        device: &Device,
        config: &SupportedStreamConfig,
        capture: AudioCapture,
    ) -> Result<Stream> {
        let config_copy = config.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        capture.process_audio_data(data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let f32_data: Vec<f32> = data.iter()
                            .map(|&sample| sample as f32 / i16::MAX as f32)
                            .collect();
                        capture.process_audio_data(&f32_data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I32 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i32], _: &cpal::InputCallbackInfo| {
                        let f32_data: Vec<f32> = data.iter()
                            .map(|&sample| sample as f32 / i32::MAX as f32)
                            .collect();
                        capture.process_audio_data(&f32_data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I8 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i8], _: &cpal::InputCallbackInfo| {
                        let f32_data: Vec<f32> = data.iter()
                            .map(|&sample| sample as f32 / i8::MAX as f32)
                            .collect();
                        capture.process_audio_data(&f32_data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported sample format: {:?}", config.sample_format()));
            }
        };

        Ok(stream)
    }

    #[cfg(target_os = "linux")]
    pub fn has_ended(&self) -> bool {
        match &self.backend {
            StreamBackend::Pulse { task, .. } => task.as_ref().map_or(true, |task| task.is_finished()),
            _ => false,
        }
    }

    #[cfg(target_os = "linux")]
    pub async fn prepare_linux(device: Arc<AudioDevice>, state: Arc<RecordingState>, role: DeviceType) -> Result<Self> {
        if role == DeviceType::System { Self::create_pulse_stream(device, state, role, None, false).await }
        else { Self::create_pulse_mic_stream(device, state, role, None, false).await }
    }
    #[cfg(target_os = "linux")]
    pub fn set_delivery(&self, enabled: bool) {
        if let Some(delivery) = &self.delivery { delivery.store(enabled, std::sync::atomic::Ordering::Release); }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn request_finish(&self) {
        if let StreamBackend::Pulse { should_finish, should_stop, .. } = &self.backend {
            if self.delivery.as_ref().is_some_and(|enabled| enabled.load(std::sync::atomic::Ordering::Acquire)) {
                should_finish.store(true, std::sync::atomic::Ordering::Release);
            } else {
                should_stop.store(true, std::sync::atomic::Ordering::Release);
            }
        }
    }

    /// Get device info
    pub fn device(&self) -> &AudioDevice {
        &self.device
    }

    /// Signal the capture thread to stop, without waiting for it to exit.
    ///
    /// Used by Drop and aborted startup, which must not preserve queued capture. Every
    /// normal shutdown path must use `stop().await` instead.
    pub fn signal_stop_only(&self) {
        match &self.backend {
            #[cfg(target_os = "linux")]
            StreamBackend::Pulse { should_stop, .. } => {
                should_stop.store(true, std::sync::atomic::Ordering::Release);
            }
            _ => {
                // CPAL/CoreAudio backends clean themselves up through their own
                // Drop implementations; no explicit signal is needed here.
            }
        }
    }

    /// Stop the stream and wait for its capture thread/task to finish.
    pub async fn stop(self) -> Result<()> {
        #[cfg(target_os = "linux")]
        self.request_finish();
        info!("Stopping audio stream for device: {}", self.device.name);

        match self.backend {
            StreamBackend::Cpal(stream) => {
                // CRITICAL: Pause the stream first to stop callbacks immediately
                // This ensures closures stop executing before we drop the stream,
                // allowing Arc references captured in callbacks to be released
                if let Err(e) = stream.pause() {
                    warn!("Failed to pause stream before drop: {}", e);
                }
                info!("Stream paused, now dropping to release callbacks");
                drop(stream);
            }
            #[cfg(target_os = "macos")]
            StreamBackend::CoreAudio { task } => {
                // Abort the processing task and wait briefly for cleanup
                if let Some(task_handle) = task {
                    info!("Aborting Core Audio task...");
                    task_handle.abort();
                    // Give the runtime a moment to clean up the aborted task
                    // This helps ensure Arc references in the closure are dropped
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    info!("Core Audio task aborted");
                }
            }
            #[cfg(target_os = "linux")]
            StreamBackend::Pulse { task, worker_error, .. } => {
                // The nonblocking worker checks this flag between mainloop
                // iterations. Join it before releasing pipeline resources.
                info!("Signalling PulseAudio capture thread to stop...");
                if let Some(task_handle) = task {
                    task_handle.await.map_err(|e| anyhow::anyhow!("Audio worker failed: {}", e))?;
                }
                if let Some(error) = worker_error.lock().unwrap().as_ref() {
                    return Err(anyhow::anyhow!(error.clone()));
                }
            }
        }

        // Explicitly drop self.device Arc reference
        drop(self.device);
        info!("Audio stream stopped and device reference dropped");
        Ok(())
    }
}

/// Audio stream manager for handling multiple streams
pub struct AudioStreamManager {
    microphone_stream: Option<AudioStream>,
    system_stream: Option<AudioStream>,
    state: Arc<RecordingState>,
}

// SAFETY: AudioStreamManager contains AudioStream which we've marked as Send
unsafe impl Send for AudioStreamManager {}

impl AudioStreamManager {
    pub fn new(state: Arc<RecordingState>) -> Self {
        Self {
            microphone_stream: None,
            system_stream: None,
            state,
        }
    }

    /// Start audio streams for the given devices
    pub async fn start_streams(
        &mut self,
        microphone_device: Option<Arc<AudioDevice>>,
        system_device: Option<Arc<AudioDevice>>,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
    ) -> Result<()> {
        self.start_streams_with(microphone_device, system_device, |device, state, role| {
            AudioStream::create(device, state, role, recording_sender.clone())
        }).await
    }

    async fn start_streams_with<F, Fut>(
        &mut self,
        microphone_device: Option<Arc<AudioDevice>>,
        system_device: Option<Arc<AudioDevice>>,
        mut create: F,
    ) -> Result<()>
    where
        F: FnMut(Arc<AudioDevice>, Arc<RecordingState>, DeviceType) -> Fut,
        Fut: std::future::Future<Output = Result<AudioStream>>,
    {
        use super::capture::get_current_backend;
        let backend = get_current_backend();
        info!("🎙️ Starting audio streams with backend: {:?}", backend);

        // Start microphone stream
        if let Some(mic_device) = microphone_device {
            info!("🎤 Creating microphone stream: {} (always uses CPAL)", mic_device.name);
            match create(mic_device.clone(), self.state.clone(), DeviceType::Microphone).await {
                Ok(stream) => {
                    self.state.set_microphone_device(mic_device);
                    self.microphone_stream = Some(stream);
                    info!("✅ Microphone stream created successfully");
                }
                Err(e) => {
                    error!("❌ Failed to create microphone stream: {}", e);
                    return Err(e);
                }
            }
        } else {
            info!("ℹ️ No microphone device specified, skipping microphone stream");
        }

        // Start system audio stream
        if let Some(sys_device) = system_device {
            info!("🔊 Creating system audio stream: {} (backend: {:?})", sys_device.name, backend);
            match create(sys_device.clone(), self.state.clone(), DeviceType::System).await {
                Ok(stream) => {
                    self.state.set_system_device(sys_device);
                    self.system_stream = Some(stream);
                    info!("✅ System audio stream created with {:?} backend", backend);
                }
                Err(e) => {
                    warn!("⚠️ Failed to create system audio stream: {}", e);
                    // Don't fail if only system audio fails
                }
            }
        } else {
            info!("ℹ️ No system device specified, skipping system audio stream");
        }

        // Ensure at least one stream was created
        if self.microphone_stream.is_none() && self.system_stream.is_none() {
            return Err(anyhow::anyhow!("No audio streams could be created"));
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(super) fn request_finish(&self) {
        for stream in [&self.microphone_stream, &self.system_stream].into_iter().flatten() {
            stream.request_finish();
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn finish_role(&self, microphone: bool) -> bool {
        let stream = if microphone { &self.microphone_stream } else { &self.system_stream };
        stream.as_ref().map_or(true, |stream| {
            stream.request_finish();
            !matches!(&stream.backend, StreamBackend::Pulse { .. }) || stream.has_ended()
        })
    }

    /// Stop all audio streams and wait for each capture thread to finish.
    pub async fn stop_streams(&mut self) -> Result<()> {
        info!("Stopping all audio streams");

        #[cfg(target_os = "linux")]
        if self.state.is_recording() {
            self.request_finish();
        } else {
            for stream in [&self.microphone_stream, &self.system_stream].into_iter().flatten() {
                stream.signal_stop_only();
            }
        }
        let mut errors = Vec::new();

        // Stop microphone stream
        if let Some(mic_stream) = self.microphone_stream.take() {
            if let Err(e) = mic_stream.stop().await {
                error!("Failed to stop microphone stream: {}", e);
                errors.push(e);
            }
        }

        // Stop system stream
        if let Some(sys_stream) = self.system_stream.take() {
            if let Err(e) = sys_stream.stop().await {
                error!("Failed to stop system stream: {}", e);
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            Err(anyhow::anyhow!("Failed to stop some streams: {:?}", errors))
        } else {
            info!("All audio streams stopped successfully");
            Ok(())
        }
    }

    /// Take the microphone stream OUT of the manager, keeping system audio
    /// running. The caller stops/drops it OUTSIDE any lock — a cpal teardown
    /// of a dead BT device can stall and must not block a held mutex.
    pub fn take_mic_stream(&mut self) -> Option<AudioStream> {
        self.microphone_stream.take()
    }

    /// Set a new microphone stream (used after hot-swap creation).
    pub fn set_mic_stream(&mut self, stream: AudioStream) {
        self.microphone_stream = Some(stream);
    }

    #[cfg(target_os = "linux")]
    pub fn take_system_stream(&mut self) -> Option<AudioStream> { self.system_stream.take() }
    #[cfg(target_os = "linux")]
    pub fn set_system_stream(&mut self, stream: AudioStream) { self.system_stream = Some(stream); }

    #[cfg(target_os = "linux")]
    pub(super) fn linux_stream_kind(&self, microphone: bool) -> Option<LinuxStreamKind> {
        let stream = if microphone { &self.microphone_stream } else { &self.system_stream };
        stream.as_ref().map(|stream| match &stream.backend {
            StreamBackend::Pulse { .. } => LinuxStreamKind::Pulse,
            StreamBackend::Cpal(_) => LinuxStreamKind::Cpal,
        })
    }

    #[cfg(target_os = "linux")]
    pub fn has_working_stream(&self, microphone: bool) -> bool {
        let stream = if microphone { &self.microphone_stream } else { &self.system_stream };
        stream.as_ref().is_some_and(|stream| !stream.has_ended())
    }

    #[cfg(target_os = "linux")]
    pub fn has_working_streams(&self) -> bool {
        self.has_working_stream(true) || self.has_working_stream(false)
    }

    /// Get stream count
    pub fn active_stream_count(&self) -> usize {
        let mut count = 0;
        if self.microphone_stream.is_some() {
            count += 1;
        }
        if self.system_stream.is_some() {
            count += 1;
        }
        count
    }

    /// Check if any streams are active
    pub fn has_active_streams(&self) -> bool {
        self.microphone_stream.is_some() || self.system_stream.is_some()
    }
}

impl Drop for AudioStreamManager {
    fn drop(&mut self) {
        // Drop can't be async, so this is a best-effort signal-only shutdown —
        // it does not wait for capture threads to actually exit. The real,
        // awaited shutdown always happens via an explicit stop_streams().await
        // earlier in every control-flow path (recording_manager.rs); this is
        // only a safety net for paths that drop the manager without calling it.
        if let Some(mic_stream) = self.microphone_stream.take() {
            mic_stream.signal_stop_only();
        }
        if let Some(sys_stream) = self.system_stream.take() {
            sys_stream.signal_stop_only();
        }
    }
}
// Callers check Pause before reading its completed total so Resume cannot admit a future timestamp.
#[cfg(target_os = "linux")]
fn linux_sample_timestamp(wall_origin: f64, first_frame: u64, paused: f64) -> f64 {
    wall_origin + first_frame as f64 / 48000.0 - paused
}

#[cfg(all(test, target_os = "linux"))]
mod linux_startup_tests {
    use super::*;

    fn device(name: &str, role: super::super::devices::DeviceType) -> Arc<AudioDevice> {
        Arc::new(AudioDevice::new(name.into(), role))
    }

    fn stream(device: Arc<AudioDevice>) -> AudioStream {
        let mut stream = super::linux_clock_tests::test_stream(
            &device.name, Arc::new(AtomicBool::new(false)), tokio::spawn(async {}),
        );
        stream.device = device;
        stream
    }

    #[tokio::test]
    async fn microphone_start_failure_returns_before_opening_system_audio() {
        let state = RecordingState::new();
        let mut manager = AudioStreamManager::new(state.clone());
        let mut calls = Vec::new();
        let result = manager.start_streams_with(
            Some(device("mic", super::super::devices::DeviceType::Input)),
            Some(device("system", super::super::devices::DeviceType::Output)),
            |device, _, role| {
                calls.push(role.clone());
                std::future::ready(if role == DeviceType::Microphone {
                    Err(anyhow::anyhow!("microphone readiness failed"))
                } else {
                    Ok(stream(device))
                })
            },
        ).await;
        assert_eq!(result.unwrap_err().to_string(), "microphone readiness failed");
        assert_eq!(calls, vec![DeviceType::Microphone]);
        assert_eq!(manager.active_stream_count(), 0);
        assert!(state.get_microphone_device().is_none());
        assert!(state.get_system_device().is_none());

        manager.start_streams_with(
            Some(device("mic", super::super::devices::DeviceType::Input)),
            Some(device("system", super::super::devices::DeviceType::Output)),
            |device, _, _| std::future::ready(Ok(stream(device))),
        ).await.unwrap();
        assert_eq!(manager.active_stream_count(), 2);
        assert_eq!(state.get_microphone_device().unwrap().name, "mic");
        manager.stop_streams().await.unwrap();
    }

    #[tokio::test]
    async fn no_microphone_still_allows_system_only_startup() {
        let state = RecordingState::new();
        let mut manager = AudioStreamManager::new(state.clone());
        let system = device("system", super::super::devices::DeviceType::Output);
        manager.start_streams_with(None, Some(system.clone()), |device, _, role| {
            assert_eq!(role, DeviceType::System);
            std::future::ready(Ok(stream(device)))
        }).await.unwrap();
        assert_eq!(manager.active_stream_count(), 1);
        assert!(state.get_microphone_device().is_none());
        assert_eq!(state.get_system_device().unwrap().name, system.name);
        manager.stop_streams().await.unwrap();
    }

    #[tokio::test]
    async fn system_failure_still_allows_microphone_only_startup() {
        let state = RecordingState::new();
        let mut manager = AudioStreamManager::new(state.clone());
        manager.start_streams_with(
            Some(device("mic", super::super::devices::DeviceType::Input)),
            Some(device("system", super::super::devices::DeviceType::Output)),
            |device, _, role| std::future::ready(if role == DeviceType::System {
                Err(anyhow::anyhow!("system unavailable"))
            } else {
                Ok(stream(device))
            }),
        ).await.unwrap();
        assert_eq!(manager.active_stream_count(), 1);
        assert_eq!(state.get_microphone_device().unwrap().name, "mic");
        assert!(state.get_system_device().is_none());
        manager.stop_streams().await.unwrap();
    }

    #[tokio::test]
    async fn both_successful_roles_are_installed() {
        let state = RecordingState::new();
        let mut manager = AudioStreamManager::new(state.clone());
        manager.start_streams_with(
            Some(device("mic", super::super::devices::DeviceType::Input)),
            Some(device("system", super::super::devices::DeviceType::Output)),
            |device, _, _| std::future::ready(Ok(stream(device))),
        ).await.unwrap();
        assert_eq!(manager.active_stream_count(), 2);
        assert_eq!(state.get_microphone_device().unwrap().name, "mic");
        assert_eq!(state.get_system_device().unwrap().name, "system");
        manager.stop_streams().await.unwrap();
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(super) mod linux_clock_tests {
    use super::*;

    pub(crate) fn test_stream(name: &str, finish: Arc<AtomicBool>, task: tokio::task::JoinHandle<()>) -> AudioStream {
        AudioStream {
            device: Arc::new(AudioDevice::new(name.into(), super::super::devices::DeviceType::Input)),
            delivery: Some(Arc::new(AtomicBool::new(true))),
            backend: StreamBackend::Pulse {
                should_stop: Arc::new(AtomicBool::new(false)), should_finish: finish,
                worker_error: Arc::new(std::sync::Mutex::new(None)), task: Some(task),
            },
        }
    }

    #[tokio::test]
    async fn native_backend_identity_survives_worker_exit() {
        let mut manager = AudioStreamManager::new(RecordingState::new());
        assert_eq!(manager.linux_stream_kind(true), None);
        let task = tokio::spawn(async {});
        while !task.is_finished() { tokio::task::yield_now().await; }
        manager.set_mic_stream(test_stream("mic", Arc::new(AtomicBool::new(false)), task));
        assert_eq!(manager.linux_stream_kind(true), Some(LinuxStreamKind::Pulse));
        assert!(!manager.has_working_stream(true));
        let stream = manager.take_mic_stream().unwrap();
        assert_eq!(manager.linux_stream_kind(true), None);
        stream.stop().await.unwrap();
    }

    #[test]
    fn alsa_callback_jitter_and_pause_do_not_change_sample_spacing() {
        let state = RecordingState::new();
        state.start_recording().unwrap();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        state.set_audio_sender(sender);
        let capture = AudioCapture::new(Arc::new(AudioDevice::new("test".into(), super::super::devices::DeviceType::Input)),
            state.clone(), 48000, 1, DeviceType::System, None);
        capture.process_audio_data(&[1.0; 10]);
        state.pause_recording().unwrap();
        capture.process_audio_data(&[3.0; 17]);
        std::thread::sleep(std::time::Duration::from_millis(10));
        state.resume_recording().unwrap();
        capture.process_audio_data(&[2.0; 11]);
        let first = receiver.try_recv().unwrap();
        let second = receiver.try_recv().unwrap();
        assert_eq!(first.data.len(), 10);
        assert_eq!(second.data.len(), 11);
        assert!((second.timestamp - first.timestamp - 10.0 / 48000.0).abs() < 1e-9);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn prepared_and_paused_frames_keep_their_session_position() {
        assert_eq!(linux_sample_timestamp(7.0, 192000, 8.0), 3.0);
        // A candidate prepared earlier retains its current position when activated.
        assert_eq!(linux_sample_timestamp(7.0, 48000, 5.0), 3.0);
        // A replacement created after an earlier pause subtracts that pause once.
        assert_eq!(linux_sample_timestamp(12.0, 48000, 8.0), 5.0);
    }

    #[test]
    fn ready_during_pause_preserves_resumed_sample_position() {
        let resumed = linux_sample_timestamp(15.0, 240000, 10.0);
        assert_eq!(resumed, 10.0);
        assert!((linux_sample_timestamp(15.0, 240480, 10.0) - resumed - 0.01).abs() < 1e-9);
    }
}
