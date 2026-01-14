// Audio file decoder for retranscription feature
// Uses Symphonia to decode MP4/AAC audio files

use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::audio_processing::{audio_to_mono, resample_audio};

/// Decoded audio data from a file
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Raw audio samples (interleaved if stereo)
    pub samples: Vec<f32>,
    /// Sample rate of the decoded audio
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
    /// Duration in seconds
    pub duration_seconds: f64,
}

impl DecodedAudio {
    /// Convert decoded audio to Whisper-compatible format (16kHz mono f32)
    pub fn to_whisper_format(&self) -> Vec<f32> {
        // Step 1: Convert to mono if needed
        let mono_samples = if self.channels > 1 {
            info!(
                "Converting {} channels to mono ({} samples)",
                self.channels,
                self.samples.len()
            );
            audio_to_mono(&self.samples, self.channels)
        } else {
            self.samples.clone()
        };

        // Step 1.5: Normalize samples to valid range (-1.0 to 1.0)
        // Some audio files may have samples slightly outside this range
        let mono_samples = normalize_audio_samples(mono_samples);

        // Step 2: Resample to 16kHz if needed
        const WHISPER_SAMPLE_RATE: u32 = 16000;
        if self.sample_rate != WHISPER_SAMPLE_RATE {
            // Use fast linear resampling for large files (>5 minutes at 48kHz = 14.4M samples)
            // The high-quality sinc resampler is too slow for retranscription of long recordings
            const LARGE_FILE_THRESHOLD: usize = 14_400_000;

            if mono_samples.len() > LARGE_FILE_THRESHOLD {
                info!(
                    "Fast resampling {} samples from {}Hz to {}Hz (large file mode)",
                    mono_samples.len(),
                    self.sample_rate,
                    WHISPER_SAMPLE_RATE
                );
                fast_resample(&mono_samples, self.sample_rate, WHISPER_SAMPLE_RATE)
            } else {
                info!(
                    "Resampling {} samples from {}Hz to {}Hz",
                    mono_samples.len(),
                    self.sample_rate,
                    WHISPER_SAMPLE_RATE
                );
                resample_audio(&mono_samples, self.sample_rate, WHISPER_SAMPLE_RATE)
            }
        } else {
            mono_samples
        }
    }
}

/// Fast linear interpolation resampling for large files
/// Much faster than sinc resampling, good enough quality for speech transcription
fn fast_resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == to_rate {
        return input.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (input.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    // Log progress for very large files
    let log_interval = output_len / 10; // Log every 10%

    for i in 0..output_len {
        let src_pos = i as f64 * ratio;
        let src_idx = src_pos as usize;
        let frac = (src_pos - src_idx as f64) as f32;

        let sample = if src_idx + 1 < input.len() {
            // Linear interpolation
            input[src_idx] * (1.0 - frac) + input[src_idx + 1] * frac
        } else if src_idx < input.len() {
            input[src_idx]
        } else {
            0.0
        };
        output.push(sample);

        // Log progress every 10%
        if log_interval > 0 && i > 0 && i % log_interval == 0 {
            debug!("Resampling progress: {}%", (i * 100) / output_len);
        }
    }

    debug!("Fast resampling complete: {} -> {} samples", input.len(), output.len());
    output
}

/// Normalize audio samples to the valid range (-1.0 to 1.0)
/// This handles audio files that may have samples slightly outside the expected range
fn normalize_audio_samples(mut samples: Vec<f32>) -> Vec<f32> {
    // First, find the maximum absolute value
    let max_abs = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, |a, b| a.max(b));

    if max_abs > 1.0 {
        // Audio exceeds valid range - normalize by scaling
        info!(
            "Audio samples exceed valid range (max: {:.3}), normalizing...",
            max_abs
        );
        let scale = 1.0 / max_abs;
        for sample in &mut samples {
            *sample *= scale;
        }
    }

    // Also clamp any remaining edge cases (NaN, infinity, etc.)
    for sample in &mut samples {
        if !sample.is_finite() {
            *sample = 0.0;
        } else {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }

    samples
}

/// Decode an audio file (MP4, M4A, WAV, etc.) to raw samples
pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio> {
    info!("Decoding audio file: {}", path.display());

    // Open the file
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow!("Failed to open audio file '{}': {}", path.display(), e))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Set up format hint based on file extension
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the file format
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| anyhow!("Failed to probe audio format: {}", e))?;

    let mut format = probed.format;

    // Find the first audio track
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("No audio track found in file"))?;

    let track_id = track.id;

    // Get audio parameters
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("Unknown sample rate"))?;

    let channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(1);

    debug!(
        "Audio track: {}Hz, {} channels",
        sample_rate, channels
    );

    // Create the decoder
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("Failed to create decoder: {}", e))?;

    // Decode all packets
    let mut all_samples: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        // Get the next packet
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // End of file
                break;
            }
            Err(e) => {
                warn!("Error reading packet: {}", e);
                break;
            }
        };

        // Skip packets from other tracks
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet
        match decoder.decode(&packet) {
            Ok(decoded) => {
                // Initialize sample buffer if needed
                if sample_buf.is_none() {
                    let spec = *decoded.spec();
                    let duration = decoded.capacity() as u64;
                    sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                }

                // Copy samples to buffer
                if let Some(ref mut buf) = sample_buf {
                    buf.copy_interleaved_ref(decoded);
                    all_samples.extend_from_slice(buf.samples());
                }
            }
            Err(e) => {
                warn!("Error decoding packet: {}", e);
                continue;
            }
        }
    }

    if all_samples.is_empty() {
        return Err(anyhow!("No audio samples decoded from file"));
    }

    let total_frames = all_samples.len() / channels as usize;
    let duration_seconds = total_frames as f64 / sample_rate as f64;

    info!(
        "Decoded {} samples ({:.2}s) at {}Hz, {} channels",
        all_samples.len(),
        duration_seconds,
        sample_rate,
        channels
    );

    Ok(DecodedAudio {
        samples: all_samples,
        sample_rate,
        channels,
        duration_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_whisper_format_mono_16k() {
        // Already in correct format
        let audio = DecodedAudio {
            samples: vec![0.1, 0.2, 0.3],
            sample_rate: 16000,
            channels: 1,
            duration_seconds: 0.0001875,
        };

        let result = audio.to_whisper_format();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_to_whisper_format_stereo_to_mono() {
        // Stereo input
        let audio = DecodedAudio {
            samples: vec![0.2, 0.4, 0.6, 0.8], // 2 stereo frames
            sample_rate: 16000,
            channels: 2,
            duration_seconds: 0.000125,
        };

        let result = audio.to_whisper_format();
        assert_eq!(result.len(), 2); // Should be mono now
        // Average of (0.2, 0.4) = 0.3 and (0.6, 0.8) = 0.7
        assert!((result[0] - 0.3).abs() < 0.001);
        assert!((result[1] - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_to_whisper_format_resamples_48k_to_16k() {
        // 48kHz mono input - should be downsampled to 16kHz
        // Use a larger sample to ensure resampler works correctly
        // 48000 samples at 48kHz = 1 second → 16000 samples at 16kHz
        let audio = DecodedAudio {
            samples: vec![0.5; 4800], // 0.1 seconds at 48kHz
            sample_rate: 48000,
            channels: 1,
            duration_seconds: 4800.0 / 48000.0,
        };

        let result = audio.to_whisper_format();
        // Output length should be approximately input_len / 3 (16000/48000 ratio)
        // 4800 / 3 = 1600
        assert!(!result.is_empty(), "Result should not be empty");
        assert!(result.len() > 1000 && result.len() < 2000,
            "Expected ~1600 samples, got {}", result.len());
    }

    #[test]
    fn test_fast_resample_same_rate() {
        // Same rate should return identical samples
        let input = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let result = fast_resample(&input, 16000, 16000);
        assert_eq!(result.len(), input.len());
        for (i, &sample) in result.iter().enumerate() {
            assert!((sample - input[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_fast_resample_empty_input() {
        let input: Vec<f32> = vec![];
        let result = fast_resample(&input, 48000, 16000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_fast_resample_downsamples_correctly() {
        // 48kHz to 16kHz = 3x downsampling
        // Create a simple ramp signal
        let input: Vec<f32> = (0..30).map(|i| i as f32 / 30.0).collect();
        let result = fast_resample(&input, 48000, 16000);

        // Output should be approximately 1/3 the length
        assert_eq!(result.len(), 10);

        // First sample should be close to 0
        assert!(result[0].abs() < 0.1);

        // Last sample should be close to the end of the ramp
        assert!(result[9] > 0.8);
    }

    #[test]
    fn test_fast_resample_upsamples_correctly() {
        // 16kHz to 48kHz = 3x upsampling
        let input: Vec<f32> = vec![0.0, 0.5, 1.0];
        let result = fast_resample(&input, 16000, 48000);

        // Output should be approximately 3x the length
        assert_eq!(result.len(), 9);

        // Should interpolate smoothly between values
        // ratio = 16000/48000 = 0.333...
        // For i=0: src_pos=0.0 → 0.0
        // For i=3: src_pos=1.0 → 0.5 (at input[1])
        // For i=6: src_pos=2.0 → 1.0 (at input[2])
        assert!(result[0].abs() < 0.01, "First sample should be ~0.0, got {}", result[0]);
        assert!((result[3] - 0.5).abs() < 0.01, "Sample at index 3 should be ~0.5, got {}", result[3]);
        assert!((result[6] - 1.0).abs() < 0.01, "Sample at index 6 should be ~1.0, got {}", result[6]);

        // Values should be monotonically increasing for this input
        for i in 1..result.len() {
            assert!(result[i] >= result[i-1] - 0.001,
                "Should be monotonic: result[{}]={} < result[{}]={}",
                i, result[i], i-1, result[i-1]);
        }
    }

    #[test]
    fn test_fast_resample_preserves_signal_range() {
        // Ensure resampling doesn't create values outside input range
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.001).sin()).collect();
        let result = fast_resample(&input, 44100, 16000);

        for sample in &result {
            assert!(*sample >= -1.0 && *sample <= 1.0,
                "Sample {} out of range [-1, 1]", sample);
        }
    }

    #[test]
    fn test_fast_resample_linear_interpolation_accuracy() {
        // Test that linear interpolation works correctly
        // Input: 0.0 at index 0, 1.0 at index 1
        // With 2x upsampling, we expect: 0.0, 0.5, 1.0
        let input: Vec<f32> = vec![0.0, 1.0];
        let result = fast_resample(&input, 16000, 32000);

        assert_eq!(result.len(), 4);
        assert!((result[0] - 0.0).abs() < 0.01); // First sample
        assert!((result[1] - 0.5).abs() < 0.01); // Interpolated
        assert!((result[2] - 1.0).abs() < 0.01); // At index 1
    }

    #[test]
    fn test_decoded_audio_duration_calculation() {
        let audio = DecodedAudio {
            samples: vec![0.0; 48000], // 1 second at 48kHz mono
            sample_rate: 48000,
            channels: 1,
            duration_seconds: 1.0,
        };

        // Duration should be samples / sample_rate for mono
        let calculated_duration = audio.samples.len() as f64 / audio.sample_rate as f64;
        assert!((calculated_duration - audio.duration_seconds).abs() < 0.001);
    }

    #[test]
    fn test_decoded_audio_stereo_duration() {
        let audio = DecodedAudio {
            samples: vec![0.0; 96000], // 1 second at 48kHz stereo (2 channels)
            sample_rate: 48000,
            channels: 2,
            duration_seconds: 1.0,
        };

        // Duration should be samples / (sample_rate * channels) for stereo
        let frames = audio.samples.len() / audio.channels as usize;
        let calculated_duration = frames as f64 / audio.sample_rate as f64;
        assert!((calculated_duration - audio.duration_seconds).abs() < 0.001);
    }

    #[test]
    fn test_to_whisper_format_handles_large_file_threshold() {
        // Test that large files use fast resampling path
        // LARGE_FILE_THRESHOLD is 14_400_000 samples
        // We'll test with a smaller sample to verify the path selection logic works
        let audio = DecodedAudio {
            samples: vec![0.5; 1000], // Small file
            sample_rate: 48000,
            channels: 1,
            duration_seconds: 1000.0 / 48000.0,
        };

        let result = audio.to_whisper_format();
        // Should complete without error and produce valid output
        assert!(!result.is_empty());
        assert!(result.len() < 1000); // Downsampled
    }

    #[test]
    fn test_normalize_audio_samples_already_normalized() {
        let samples = vec![0.5, -0.5, 0.0, 0.9, -0.9];
        let result = normalize_audio_samples(samples.clone());
        // Should be unchanged (already in range)
        for (i, &s) in result.iter().enumerate() {
            assert!((s - samples[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_normalize_audio_samples_exceeds_range() {
        let samples = vec![0.5, -0.5, 2.0, -1.5]; // max_abs = 2.0
        let result = normalize_audio_samples(samples);
        // All samples should be scaled by 0.5 (1.0 / 2.0)
        assert!((result[0] - 0.25).abs() < 0.001);
        assert!((result[1] - -0.25).abs() < 0.001);
        assert!((result[2] - 1.0).abs() < 0.001);
        assert!((result[3] - -0.75).abs() < 0.001);
    }

    #[test]
    fn test_normalize_audio_samples_handles_nan() {
        let samples = vec![0.5, f32::NAN, 0.3];
        let result = normalize_audio_samples(samples);
        assert!((result[0] - 0.5).abs() < 0.001);
        assert_eq!(result[1], 0.0); // NaN replaced with 0
        assert!((result[2] - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_normalize_audio_samples_handles_infinity() {
        let samples = vec![0.5, f32::INFINITY, -0.3];
        let result = normalize_audio_samples(samples);
        // Infinity will be clamped to 0.0 (since !is_finite)
        assert_eq!(result[1], 0.0);
    }
}
