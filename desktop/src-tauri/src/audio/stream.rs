use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, Stream, SupportedStreamConfig};
use log::{error, info, warn};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tokio::sync::mpsc;

use super::capture::{get_current_backend, AudioCaptureBackend};
use super::devices::{get_device_and_config_blocking, AudioDevice};
use super::pipeline::AudioCapture;
use super::recording_state::{DeviceType, RecordingState};

#[cfg(target_os = "macos")]
use super::capture::CoreAudioCapture;

struct CpalStreamHandle {
    stop_tx: Option<std_mpsc::Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

/// Stream backend implementation
enum StreamBackend {
    /// CPAL stream owned by a dedicated worker thread so shared state never contains a non-Send stream.
    Cpal(CpalStreamHandle),
    /// Core Audio direct implementation (macOS only)
    #[cfg(target_os = "macos")]
    CoreAudio {
        task: Option<tokio::task::JoinHandle<()>>,
    },
}

/// Simplified audio stream wrapper with multi-backend support
pub struct AudioStream {
    device: Arc<AudioDevice>,
    backend: StreamBackend,
}

impl AudioStream {
    /// Create a new audio stream for the given device
    pub async fn create(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
    ) -> Result<Self> {
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
        info!(
            "Creating audio stream for device: {} with backend: {:?}, device_type: {:?}",
            device.name, backend_type, device_type
        );

        #[cfg(target_os = "macos")]
        let use_core_audio =
            device_type == DeviceType::System && backend_type == AudioCaptureBackend::CoreAudio;

        #[cfg(not(target_os = "macos"))]
        let _use_core_audio = false;

        #[cfg(target_os = "macos")]
        if use_core_audio {
            info!("Using Core Audio backend for system audio");
            return Self::create_core_audio_stream(device, state, device_type, recording_sender)
                .await;
        }

        info!("Using CPAL backend for device: {}", device.name);
        Self::create_cpal_stream(device, state, device_type, recording_sender).await
    }

    /// Create a CPAL-based stream and keep it on a dedicated worker thread.
    async fn create_cpal_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
    ) -> Result<Self> {
        info!("Creating CPAL stream for device: {}", device.name);

        let thread_device = device.clone();
        let thread_state = state.clone();
        let thread_device_name = device.name.clone();
        let (ready_tx, ready_rx) = std_mpsc::sync_channel::<Result<()>>(1);
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();

        let thread_name = format!(
            "audio-stream-{}",
            thread_device_name
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                .collect::<String>()
        );

        let join_handle = thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let startup_result = (|| -> Result<Stream> {
                    let (cpal_device, config) = get_device_and_config_blocking(&thread_device)?;

                    info!(
                        "Audio config for {} - sample rate: {}, channels: {}, format: {:?}",
                        thread_device.name,
                        config.sample_rate().0,
                        config.channels(),
                        config.sample_format()
                    );

                    let capture = AudioCapture::new(
                        thread_device.clone(),
                        thread_state,
                        config.sample_rate().0,
                        config.channels(),
                        device_type.clone(),
                        recording_sender,
                    );

                    let stream = Self::build_stream(&cpal_device, &config, capture)?;
                    stream.play()?;
                    Ok(stream)
                })();

                match startup_result {
                    Ok(stream) => {
                        let _ = ready_tx.send(Ok(()));
                        let _ = stop_rx.recv();

                        if let Err(err) = stream.pause() {
                            warn!("Failed to pause stream before drop: {}", err);
                        }
                    }
                    Err(err) => {
                        error!(
                            "Failed to create CPAL stream for {}: {}",
                            thread_device_name, err
                        );
                        let _ = ready_tx.send(Err(err));
                    }
                }
            })
            .map_err(|err| {
                anyhow!(
                    "Failed to spawn audio stream worker for {}: {}",
                    device.name,
                    err
                )
            })?;

        match ready_rx.recv() {
            Ok(Ok(())) => {
                info!("CPAL stream started for device: {}", device.name);
                Ok(Self {
                    device,
                    backend: StreamBackend::Cpal(CpalStreamHandle {
                        stop_tx: Some(stop_tx),
                        join_handle: Some(join_handle),
                    }),
                })
            }
            Ok(Err(err)) => {
                let _ = join_handle.join();
                Err(err)
            }
            Err(_) => {
                let join_result = join_handle.join();
                if join_result.is_err() {
                    return Err(anyhow!(
                        "Audio stream worker panicked before startup completed for {}",
                        device.name
                    ));
                }
                Err(anyhow!(
                    "Audio stream worker exited before startup completed for {}",
                    device.name
                ))
            }
        }
    }

    /// Create a Core Audio stream (macOS only)
    #[cfg(target_os = "macos")]
    async fn create_core_audio_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<super::recording_state::AudioChunk>>,
    ) -> Result<Self> {
        info!("Creating Core Audio stream for device: {}", device.name);

        let capture_impl = CoreAudioCapture::new().map_err(|e| {
            error!("CoreAudioCapture::new() failed: {}", e);
            anyhow!("Failed to create Core Audio capture: {}", e)
        })?;

        let core_stream = capture_impl.stream().map_err(|e| {
            error!("capture_impl.stream() failed: {}", e);
            anyhow!("Failed to create Core Audio stream: {}", e)
        })?;

        let sample_rate = core_stream.sample_rate();
        info!(
            "Core Audio stream created with sample rate: {} Hz",
            sample_rate
        );

        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            sample_rate,
            1,
            device_type,
            recording_sender,
        );

        let device_name = device.name.clone();
        let task = tokio::spawn({
            let capture = capture.clone();
            let mut stream = core_stream;

            async move {
                use futures_util::StreamExt;

                let mut buffer = Vec::new();
                let mut frame_count = 0;
                let frames_per_chunk = 1024;

                info!("Core Audio processing task started for {}", device_name);

                while let Some(sample) = stream.next().await {
                    buffer.push(sample);
                    frame_count += 1;

                    if frame_count >= frames_per_chunk {
                        capture.process_audio_data(&buffer);
                        buffer.clear();
                        frame_count = 0;
                    }
                }

                if !buffer.is_empty() {
                    capture.process_audio_data(&buffer);
                }

                info!("Core Audio processing task ended for {}", device_name);
            }
        });

        Ok(Self {
            device: device.clone(),
            backend: StreamBackend::CoreAudio { task: Some(task) },
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
                        let f32_data: Vec<f32> = data
                            .iter()
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
                        let f32_data: Vec<f32> = data
                            .iter()
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
                        let f32_data: Vec<f32> = data
                            .iter()
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
                return Err(anyhow!(
                    "Unsupported sample format: {:?}",
                    config.sample_format()
                ));
            }
        };

        Ok(stream)
    }

    /// Get device info
    pub fn device(&self) -> &AudioDevice {
        &self.device
    }

    /// Stop the stream
    pub fn stop(self) -> Result<()> {
        info!("Stopping audio stream for device: {}", self.device.name);

        match self.backend {
            StreamBackend::Cpal(mut handle) => {
                if let Some(stop_tx) = handle.stop_tx.take() {
                    let _ = stop_tx.send(());
                }

                if let Some(join_handle) = handle.join_handle.take() {
                    join_handle.join().map_err(|_| {
                        anyhow!(
                            "Audio stream worker panicked while stopping device {}",
                            self.device.name
                        )
                    })?;
                }
            }
            #[cfg(target_os = "macos")]
            StreamBackend::CoreAudio { task } => {
                if let Some(task_handle) = task {
                    task_handle.abort();
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }

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
        let backend = get_current_backend();
        info!("Starting audio streams with backend: {:?}", backend);

        if let Some(mic_device) = microphone_device {
            info!("Creating microphone stream: {}", mic_device.name);
            match AudioStream::create(
                mic_device.clone(),
                self.state.clone(),
                DeviceType::Microphone,
                recording_sender.clone(),
            )
            .await
            {
                Ok(stream) => {
                    self.state.set_microphone_device(mic_device);
                    self.microphone_stream = Some(stream);
                    info!("Microphone stream created successfully");
                }
                Err(e) => {
                    error!("Failed to create microphone stream: {}", e);
                    return Err(e);
                }
            }
        } else {
            info!("No microphone device specified, skipping microphone stream");
        }

        if let Some(sys_device) = system_device {
            info!(
                "Creating system audio stream: {} (backend: {:?})",
                sys_device.name, backend
            );
            match AudioStream::create(
                sys_device.clone(),
                self.state.clone(),
                DeviceType::System,
                recording_sender.clone(),
            )
            .await
            {
                Ok(stream) => {
                    self.state.set_system_device(sys_device);
                    self.system_stream = Some(stream);
                    info!("System audio stream created with {:?} backend", backend);
                }
                Err(e) => {
                    warn!("Failed to create system audio stream: {}", e);
                }
            }
        } else {
            info!("No system device specified, skipping system audio stream");
        }

        if self.microphone_stream.is_none() && self.system_stream.is_none() {
            return Err(anyhow!("No audio streams could be created"));
        }

        Ok(())
    }

    /// Stop all audio streams
    pub fn stop_streams(&mut self) -> Result<()> {
        info!("Stopping all audio streams");

        let mut errors = Vec::new();

        if let Some(mic_stream) = self.microphone_stream.take() {
            if let Err(e) = mic_stream.stop() {
                error!("Failed to stop microphone stream: {}", e);
                errors.push(e);
            }
        }

        if let Some(sys_stream) = self.system_stream.take() {
            if let Err(e) = sys_stream.stop() {
                error!("Failed to stop system stream: {}", e);
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            Err(anyhow!("Failed to stop some streams: {:?}", errors))
        } else {
            info!("All audio streams stopped successfully");
            Ok(())
        }
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
        if let Err(e) = self.stop_streams() {
            error!("Error stopping streams during drop: {}", e);
        }
    }
}
