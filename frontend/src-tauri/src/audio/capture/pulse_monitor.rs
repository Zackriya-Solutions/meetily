// Linux system-audio capture via PulseAudio/PipeWire monitor sources.
//
// cpal has no PulseAudio host on Linux — its only Linux backend is plain
// ALSA, which never exposes a sink's virtual ".monitor" source (that name
// lives purely in PulseAudio/PipeWire's own namespace, not in any ALSA PCM
// list). So a monitor source can't be opened through cpal at all; this
// shells out to `parec` (the PulseAudio/PipeWire-compatible recorder) to
// pull PCM directly from the named monitor, independent of whatever the
// system's current default source/sink is.

use std::io::Read;
use std::process::{Child, Command, Stdio};

use anyhow::{anyhow, Result};
use log::{info, warn};

use crate::audio::pipeline::AudioCapture;

/// Sample rate and channel count requested from `parec`. Matching the
/// pipeline's expected rate here means no resampling is needed downstream.
pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u16 = 2;

const CHUNK_FRAMES: usize = 1024;

pub struct PulseMonitorCapture {
    child: Child,
}

impl PulseMonitorCapture {
    /// Spawn `parec` against `source_name` (e.g.
    /// `alsa_output.usb-....analog-surround-71.monitor`) and forward decoded
    /// samples into `capture` on a dedicated OS thread until the process
    /// exits or is killed.
    pub fn spawn(source_name: &str, capture: AudioCapture) -> Result<Self> {
        let mut child = Command::new("parec")
            .arg("--device")
            .arg(source_name)
            .arg("--rate")
            .arg(SAMPLE_RATE.to_string())
            .arg("--channels")
            .arg(CHANNELS.to_string())
            .arg("--format=float32le")
            .arg("--raw")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to start parec for monitor source '{}': {}", source_name, e))?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("parec produced no stdout handle"))?;

        let device_name = source_name.to_string();
        std::thread::spawn(move || {
            let chunk_bytes = CHUNK_FRAMES * CHANNELS as usize * 4; // f32le = 4 bytes/sample
            let mut raw = vec![0u8; chunk_bytes];

            info!("🔊 [PulseMonitor] Capture thread started for '{}'", device_name);

            loop {
                match stdout.read_exact(&mut raw) {
                    Ok(()) => {
                        let samples: Vec<f32> = raw
                            .chunks_exact(4)
                            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                            .collect();
                        capture.process_audio_data(&samples);
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::UnexpectedEof {
                            warn!("[PulseMonitor] parec read error: {}", e);
                        }
                        break;
                    }
                }
            }

            info!("🔊 [PulseMonitor] Capture thread ended for '{}'", device_name);
        });

        Ok(Self { child })
    }

    pub fn stop(mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for PulseMonitorCapture {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
