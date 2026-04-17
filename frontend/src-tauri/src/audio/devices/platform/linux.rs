use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};

use crate::audio::devices::configuration::{AudioDevice, DeviceType};

/// Decide whether a Linux device name is worth showing to the user.
///
/// On PipeWire/PulseAudio systems, cpal's ALSA backend enumerates every raw
/// ALSA PCM (`sysdefault:CARD=X`, `front:CARD=X,DEV=0`, `surround51:CARD=X`,
/// `iec958:CARD=X`, `hw:CARD=X`, etc.). None of these are meaningful choices
/// — the user should pick a logical endpoint (`pipewire`, `default`, `pulse`)
/// and let PipeWire route to the currently selected source/sink.
///
/// Returns true when `name` should appear in the picker.
pub fn is_user_facing_linux_device(name: &str) -> bool {
    // Always-show logical endpoints.
    const LOGICAL: &[&str] = &["default", "pipewire", "pulse", "jack", "sysdefault"];
    if LOGICAL.contains(&name) {
        return true;
    }

    // Raw ALSA PCM profile entries — hide.
    const RAW_ALSA_PREFIXES: &[&str] = &[
        "front:", "rear:", "center_lfe:", "side:",
        "surround21:", "surround40:", "surround41:",
        "surround50:", "surround51:", "surround71:",
        "iec958:", "hdmi:", "dmix:", "dsnoop:",
        "hw:", "plughw:", "plug:",
        "sysdefault:",
    ];
    if RAW_ALSA_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return false;
    }

    // Otherwise keep — covers PulseAudio/PipeWire named nodes like
    // `alsa_input.usb-Rode_NT-USB-00.pro-input-0` and anything else cpal
    // surfaces that doesn't match the raw-ALSA pattern.
    true
}

/// Configure Linux audio devices using ALSA / PulseAudio / PipeWire.
pub fn configure_linux_audio(host: &cpal::Host) -> Result<Vec<AudioDevice>> {
    let mut devices = Vec::new();

    // Microphones.
    for device in host.input_devices()? {
        if let Ok(name) = device.name() {
            if is_user_facing_linux_device(&name) {
                devices.push(AudioDevice::new(name, DeviceType::Input));
            }
        }
    }

    // Monitor sources = system-audio capture (meeting participants).
    if let Ok(alsa_host) = cpal::host_from_id(cpal::HostId::Alsa) {
        for device in alsa_host.input_devices()? {
            if let Ok(name) = device.name() {
                if name.contains("monitor") && is_user_facing_linux_device(&name) {
                    devices.push(AudioDevice::new(
                        format!("{} (System Audio)", name),
                        DeviceType::Output,
                    ));
                }
            }
        }
    }

    Ok(devices)
}
