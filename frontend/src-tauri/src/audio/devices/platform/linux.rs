use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
use log::warn;

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

    // Add PulseAudio/PipeWire monitor sources for system audio.
    //
    // cpal has no PulseAudio host on Linux (only ALSA), and plain ALSA
    // enumeration never surfaces a sink's virtual ".monitor" source — that
    // name only exists in PulseAudio/PipeWire's own namespace. Query pactl
    // directly instead; the matching capture path (audio/capture/pulse_monitor.rs)
    // opens these via `parec`, not cpal.
    match list_pulse_monitor_sources() {
        Ok(monitors) => {
            for name in monitors {
                devices.push(AudioDevice::new(name, DeviceType::Output));
            }
        }
        Err(e) => {
            warn!("Failed to list PulseAudio/PipeWire monitor sources via pactl: {}", e);
        }
    }

    Ok(devices)
}

/// Enumerate PulseAudio/PipeWire monitor sources by shelling out to `pactl`.
/// Each line of `pactl list short sources` is `id\tname\t...`; a monitor
/// source's name always ends in `.monitor`.
fn list_pulse_monitor_sources() -> Result<Vec<String>> {
    let output = std::process::Command::new("pactl")
        .args(["list", "short", "sources"])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("pactl exited with status {}", output.status));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(|line| line.split('\t').nth(1))
        .filter(|name| name.ends_with(".monitor"))
        .map(|s| s.to_string())
        .collect())
}