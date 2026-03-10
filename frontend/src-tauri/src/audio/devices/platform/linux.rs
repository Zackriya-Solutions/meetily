use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
use log::{debug, info, warn};
use std::process::Command;

use crate::audio::devices::configuration::{AudioDevice, DeviceType};

/// Query pactl for PulseAudio/PipeWire monitor sources (system audio)
/// Returns source names ending in ".monitor"
fn get_pulseaudio_monitors() -> Vec<String> {
    let output = match Command::new("pactl").args(["list", "sources", "short"]).output() {
        Ok(o) => o,
        Err(e) => {
            debug!("[Linux Audio] pactl not found: {}", e);
            return Vec::new();
        }
    };

    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            // Format: ID\tNAME\tMODULE\tFORMAT\tSTATE
            let name = line.split('\t').nth(1)?.trim();
            if name.ends_with(".monitor") {
                info!("[Linux Audio] Found monitor: {}", name);
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Query pactl for PulseAudio/PipeWire input sources (microphones)
fn get_pulseaudio_inputs() -> Vec<String> {
    let output = match Command::new("pactl").args(["list", "sources", "short"]).output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let name = line.split('\t').nth(1)?.trim();
            if !name.ends_with(".monitor") {
                debug!("[Linux Audio] Found input: {}", name);
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Configure Linux audio devices using PulseAudio/PipeWire and ALSA fallback.
///
/// Monitor sources (names ending in ".monitor") are exposed as DeviceType::Output
/// so they appear in the "System Audio" device selector in the UI.
/// Regular sources are exposed as DeviceType::Input (microphones).
pub fn configure_linux_audio(host: &cpal::Host) -> Result<Vec<AudioDevice>> {
    let mut devices: Vec<AudioDevice> = Vec::new();

    // Prefer PulseAudio/PipeWire enumeration via pactl
    let monitors = get_pulseaudio_monitors();
    let inputs = get_pulseaudio_inputs();

    if !monitors.is_empty() || !inputs.is_empty() {
        info!("[Linux Audio] Using pactl: {} monitors, {} inputs", monitors.len(), inputs.len());

        for name in monitors {
            devices.push(AudioDevice::new(name, DeviceType::Output));
        }
        for name in inputs {
            devices.push(AudioDevice::new(name, DeviceType::Input));
        }
    }

    // ALSA fallback / supplement: add any devices not already listed
    for device in host.input_devices()? {
        let Ok(name) = device.name() else { continue };
        if devices.iter().any(|d| d.name == name) {
            continue;
        }
        debug!("[Linux Audio] ALSA device: {}", name);
        if name.contains(".monitor") || name.to_lowercase().contains("loopback") {
            devices.push(AudioDevice::new(name, DeviceType::Output));
        } else {
            devices.push(AudioDevice::new(name, DeviceType::Input));
        }
    }

    let mic_count = devices.iter().filter(|d| d.device_type == DeviceType::Input).count();
    let sys_count = devices.iter().filter(|d| d.device_type == DeviceType::Output).count();
    info!("[Linux Audio] {} microphones, {} system audio sources", mic_count, sys_count);

    if sys_count == 0 {
        warn!("[Linux Audio] No monitor sources found. Run 'pactl list sources short' to verify PulseAudio/PipeWire is running.");
    }

    Ok(devices)
}

/// Get the PulseAudio default sink name via `pactl get-default-sink`
fn get_default_sink() -> Option<String> {
    let output = Command::new("pactl").arg("get-default-sink").output().ok()?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Return the best system audio device for Linux (monitor source).
///
/// Priority:
/// 1. Monitor of the current default PulseAudio/PipeWire sink (follows whatever
///    the user has set as output — built-in, Bluetooth, USB, HDMI, etc.)
/// 2. First available monitor
/// 3. ALSA loopback
pub fn default_system_audio_device() -> Result<AudioDevice> {
    let monitors = get_pulseaudio_monitors();

    // Follow the actual default sink so we capture whatever the user hears,
    // regardless of whether it is Bluetooth, built-in, HDMI, or USB.
    if let Some(default_sink) = get_default_sink() {
        let monitor_name = format!("{}.monitor", default_sink);
        if let Some(name) = monitors.iter().find(|n| **n == monitor_name) {
            info!("[Linux] Default system audio (follows default sink '{}'): {}", default_sink, name);
            return Ok(AudioDevice::new(name.clone(), DeviceType::Output));
        }
        // Sink exists but its monitor wasn't listed — construct the name anyway
        // and let the capture path resolve it via PULSE_SOURCE
        info!("[Linux] Constructing monitor name for default sink '{}': {}", default_sink, monitor_name);
        return Ok(AudioDevice::new(monitor_name, DeviceType::Output));
    }

    // Fall back to first available monitor
    if let Some(name) = monitors.into_iter().next() {
        info!("[Linux] Default system audio (first monitor fallback): {}", name);
        return Ok(AudioDevice::new(name, DeviceType::Output));
    }

    // Last resort: ALSA loopback
    let host = cpal::default_host();
    for device in host.input_devices()? {
        if let Ok(name) = device.name() {
            if name.to_lowercase().contains("loopback") {
                info!("[Linux] Using ALSA loopback as system audio: {}", name);
                return Ok(AudioDevice::new(name, DeviceType::Output));
            }
        }
    }

    warn!("[Linux] No system audio device found. Ensure PulseAudio/PipeWire is running.");
    anyhow::bail!("No system audio monitor source found on Linux. Run 'pactl list sources short' to check.")
}
