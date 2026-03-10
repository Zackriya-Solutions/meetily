use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::AtomicU64;

lazy_static! {
    pub static ref LAST_AUDIO_CAPTURE: AtomicU64 = AtomicU64::new(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioTranscriptionEngine {
    Deepgram,
    WhisperTiny,
    WhisperDistilLargeV3,
    WhisperLargeV3Turbo,
    WhisperLargeV3,
}

impl fmt::Display for AudioTranscriptionEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioTranscriptionEngine::Deepgram => write!(f, "Deepgram"),
            AudioTranscriptionEngine::WhisperTiny => write!(f, "WhisperTiny"),
            AudioTranscriptionEngine::WhisperDistilLargeV3 => write!(f, "WhisperLarge"),
            AudioTranscriptionEngine::WhisperLargeV3Turbo => write!(f, "WhisperLargeV3Turbo"),
            AudioTranscriptionEngine::WhisperLargeV3 => write!(f, "WhisperLargeV3"),
        }
    }
}

impl Default for AudioTranscriptionEngine {
    fn default() -> Self {
        AudioTranscriptionEngine::WhisperLargeV3Turbo
    }
}

#[derive(Clone, Debug)]
pub struct DeviceControl {
    pub is_running: bool,
    pub is_paused: bool,
}

#[derive(Clone, Eq, PartialEq, Hash, Serialize, Debug, Deserialize)]
pub enum DeviceType {
    Input,
    Output,
}

#[derive(Clone, Eq, PartialEq, Hash, Serialize, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub device_type: DeviceType,
}

impl AudioDevice {
    pub fn new(name: String, device_type: DeviceType) -> Self {
        AudioDevice { name, device_type }
    }

    pub fn from_name(name: &str) -> Result<Self> {
        if name.trim().is_empty() {
            return Err(anyhow!("Device name cannot be empty"));
        }

        let (name, device_type) = if name.to_lowercase().ends_with("(input)") {
            (
                name.trim_end_matches("(input)").trim().to_string(),
                DeviceType::Input,
            )
        } else if name.to_lowercase().ends_with("(output)") {
            (
                name.trim_end_matches("(output)").trim().to_string(),
                DeviceType::Output,
            )
        } else {
            return Err(anyhow!(
                "Device type (input/output) not specified in the name"
            ));
        };

        Ok(AudioDevice::new(name, device_type))
    }
}

impl fmt::Display for AudioDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} ({})",
            self.name,
            match self.device_type {
                DeviceType::Input => "input",
                DeviceType::Output => "output",
            }
        )
    }
}

/// Parse audio device from string name
pub fn parse_audio_device(name: &str) -> Result<AudioDevice> {
    AudioDevice::from_name(name)
}

/// Get device and config for audio operations
pub async fn get_device_and_config(
    audio_device: &AudioDevice,
) -> Result<(cpal::Device, cpal::SupportedStreamConfig)> {
    #[cfg(target_os = "windows")]
    {
        return super::platform::get_windows_device(audio_device);
    }

    #[cfg(not(target_os = "windows"))]
    {
        use cpal::traits::{DeviceTrait, HostTrait};

        let host = cpal::default_host();

        match audio_device.device_type {
            DeviceType::Input => {
                for device in host.input_devices()? {
                    if let Ok(name) = device.name() {
                        if name == audio_device.name {
                            let default_config = device
                                .default_input_config()
                                .map_err(|e| anyhow!("Failed to get default input config: {}", e))?;
                            return Ok((device, default_config));
                        }
                    }
                }
            }
            DeviceType::Output => {
                #[cfg(target_os = "macos")]
                {
                    // Use default host for all macOS output devices
                    // Core Audio backend uses direct cidre API for system capture, not cpal
                    for device in host.output_devices()? {
                        if let Ok(name) = device.name() {
                            if name == audio_device.name {
                                let default_config = device
                                    .default_output_config()
                                    .map_err(|e| anyhow!("Failed to get output config: {}", e))?;
                                return Ok((device, default_config));
                            }
                        }
                    }
                }

                #[cfg(target_os = "linux")]
                {
                    use log::{info, warn};

                    // On Linux, system audio devices are PulseAudio/PipeWire monitor sources.
                    // Monitor sources are accessible as INPUT devices via the "pulse" ALSA plugin
                    // when PULSE_SOURCE is set to the monitor name.
                    let is_pa_monitor = audio_device.name.ends_with(".monitor")
                        || audio_device.name.contains("alsa_output");

                    if is_pa_monitor {
                        // Set PULSE_SOURCE so the "pulse" ALSA device reads from this monitor
                        std::env::set_var("PULSE_SOURCE", &audio_device.name);
                        info!("[Linux DeviceConfig] Set PULSE_SOURCE={}", audio_device.name);

                        // Use the "pulse" ALSA device to capture from the monitor
                        for device in host.input_devices()? {
                            if let Ok(name) = device.name() {
                                if name == "pulse" {
                                    let config = device
                                        .default_input_config()
                                        .map_err(|e| anyhow!("Failed to get pulse input config: {}", e))?;
                                    info!("[Linux DeviceConfig] Capturing monitor '{}' via 'pulse' device", audio_device.name);
                                    return Ok((device, config));
                                }
                            }
                        }
                        warn!("[Linux DeviceConfig] 'pulse' ALSA device not found — install pulseaudio-alsa or pipewire-alsa");
                    }

                    // Fallback: try to find the monitor as a direct ALSA input device
                    for device in host.input_devices()? {
                        if let Ok(name) = device.name() {
                            if name == audio_device.name {
                                let config = device
                                    .default_input_config()
                                    .map_err(|e| anyhow!("Failed to get input config: {}", e))?;
                                return Ok((device, config));
                            }
                        }
                    }
                }
            }
        }

        Err(anyhow!("Device not found: {}", audio_device.name))
    }
}