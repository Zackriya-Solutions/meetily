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

/// Pick a preferred `SupportedStreamConfig` for an input device.
///
/// On Linux, strongly prefer 48 kHz. PipeWire's graph runs at 48 kHz by
/// default; opening a client at any other rate (cpal's ALSA default is
/// often 44.1 kHz on pipewire-alsa) forces pipewire-alsa to insert a
/// resampler. The resampled client then runs at a small internal quantum
/// (observed 256 frames) and drags every other audio client — Teams,
/// Zoom, browser WebRTC — into the same small quantum, producing audible
/// fuzz/glitches. See pw-top: `QUANT=256 RATE=44100` on the `alsa_capture.*`
/// node vs. `QUANT=512-1024 RATE=48000` on the rest of the graph.
///
/// On macOS/Windows, fall back to cpal's `default_input_config()`.
#[cfg(not(target_os = "windows"))]
fn preferred_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig> {
    use cpal::traits::DeviceTrait;

    let default_cfg = device
        .default_input_config()
        .map_err(|e| anyhow!("Failed to get default input config: {}", e))?;

    #[cfg(target_os = "linux")]
    {
        use log::info;
        const TARGET_RATE: u32 = 48_000;
        let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());

        if default_cfg.sample_rate().0 == TARGET_RATE {
            return Ok(default_cfg);
        }

        // Look through supported input configs for a range matching the
        // default's channel count / sample format that also covers 48 kHz.
        // Matching channels+format keeps us close to what cpal's default
        // picker chose — we only adjust the sample rate.
        if let Ok(supported) = device.supported_input_configs() {
            for cfg in supported {
                if cfg.channels() == default_cfg.channels()
                    && cfg.sample_format() == default_cfg.sample_format()
                    && cfg.min_sample_rate().0 <= TARGET_RATE
                    && cfg.max_sample_rate().0 >= TARGET_RATE
                {
                    info!(
                        "🎚️ audio: preferring 48 kHz over default {} Hz on '{}' to match PipeWire graph rate",
                        default_cfg.sample_rate().0,
                        device_name,
                    );
                    return Ok(cfg.with_sample_rate(cpal::SampleRate(TARGET_RATE)));
                }
            }
        }

        info!(
            "🎚️ audio: device '{}' doesn't expose a 48 kHz input config matching the default ({} Hz, {} ch, {:?}) — keeping default",
            device_name,
            default_cfg.sample_rate().0,
            default_cfg.channels(),
            default_cfg.sample_format(),
        );
    }

    Ok(default_cfg)
}

#[cfg(not(target_os = "windows"))]
fn preferred_output_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig> {
    use cpal::traits::DeviceTrait;
    device
        .default_output_config()
        .map_err(|e| anyhow!("Failed to get default output config: {}", e))
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
                            let config = preferred_input_config(&device)?;
                            return Ok((device, config));
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
                                let config = preferred_output_config(&device)?;
                                return Ok((device, config));
                            }
                        }
                    }
                }

                #[cfg(target_os = "linux")]
                {
                    // For Linux, we use PulseAudio monitor sources for system audio.
                    // Monitor sources are *input* PCMs in ALSA terms, so the same
                    // 48 kHz preference applies.
                    if let Ok(pulse_host) = cpal::host_from_id(cpal::HostId::Alsa) {
                        for device in pulse_host.input_devices()? {
                            if let Ok(name) = device.name() {
                                if name == audio_device.name {
                                    let config = preferred_input_config(&device)?;
                                    return Ok((device, config));
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(anyhow!("Device not found: {}", audio_device.name))
    }
}