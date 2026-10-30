use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};

use crate::audio::devices::configuration::{AudioDevice, DeviceType};

/// Configure Linux audio devices using ALSA/PulseAudio
pub fn configure_linux_audio(host: &cpal::Host) -> Result<Vec<AudioDevice>> {
    let mut devices = Vec::new();

    // Add input devices
    for device in host.input_devices()? {
        if let Ok(name) = device.name() {
            devices.push(AudioDevice::new(name, DeviceType::Input));
        }
    }

    // Query the audio server directly, not ALSA's unrelated PCM list.
    match crate::audio::linux_pulse::monitor_sources() {
        Ok(monitors) => devices.extend(monitors.sources.into_iter().map(|source| {
            AudioDevice::new(source.name, DeviceType::Output)
        })),
        Err(error) => log::warn!("System audio sources unavailable: {error:#}"),
    }

    Ok(devices)
}
