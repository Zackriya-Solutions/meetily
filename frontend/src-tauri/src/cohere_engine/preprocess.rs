//! Audio preprocessing: resampling + log-mel spectrogram.
//!
//! The Cohere Transcribe model expects the same Whisper-style features:
//! 16 kHz mono PCM, 80-bin log-mel, `n_fft = 400`, `hop = 160`,
//! 3000 frames (= 30 seconds) padded/truncated.

use ndarray::Array2;
use realfft::RealFftPlanner;

pub const SAMPLE_RATE: u32 = 16_000;
pub const N_FFT: usize = 400;
pub const HOP_LENGTH: usize = 160;
pub const N_MELS: usize = 80;
pub const N_FRAMES: usize = 3000;
pub const F_MIN: f32 = 0.0;
pub const F_MAX: f32 = 8_000.0;

/// Linear-interpolation resampler. Good enough for speech-band preprocessing;
/// anti-aliasing is handled upstream (the pipeline already downsamples from
/// 48 kHz via high-quality rubato before VAD).
pub fn resample_linear(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if samples.is_empty() || src_rate == dst_rate {
        return samples.to_vec();
    }
    let ratio = dst_rate as f64 / src_rate as f64;
    let out_len = ((samples.len() as f64) * ratio).round() as usize;
    if out_len == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_idx = (i as f64) / ratio;
        let lo = src_idx.floor() as usize;
        let hi = (lo + 1).min(samples.len() - 1);
        let frac = (src_idx - lo as f64) as f32;
        out.push(samples[lo] * (1.0 - frac) + samples[hi] * frac);
    }
    out
}

/// Hertz → Mel (HTK convention, matches Whisper reference).
fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

/// Mel → Hertz.
fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10f32.powf(mel / 2595.0) - 1.0)
}

/// Build an `(N_MELS, N_FFT/2 + 1)` triangular filterbank on the mel scale.
fn mel_filterbank(sample_rate: u32, n_fft: usize, n_mels: usize, f_min: f32, f_max: f32) -> Vec<Vec<f32>> {
    let n_bins = n_fft / 2 + 1;
    let mel_min = hz_to_mel(f_min);
    let mel_max = hz_to_mel(f_max);

    // `n_mels + 2` mel-evenly-spaced edges → `n_mels` triangles.
    let mel_points: Vec<f32> = (0..=n_mels + 1)
        .map(|i| mel_min + (mel_max - mel_min) * (i as f32) / (n_mels as f32 + 1.0))
        .collect();
    let hz_points: Vec<f32> = mel_points.iter().copied().map(mel_to_hz).collect();

    // FFT bin centers in Hz.
    let bin_hz: Vec<f32> = (0..n_bins)
        .map(|k| k as f32 * sample_rate as f32 / n_fft as f32)
        .collect();

    let mut fb = vec![vec![0.0f32; n_bins]; n_mels];
    for m in 0..n_mels {
        let left = hz_points[m];
        let center = hz_points[m + 1];
        let right = hz_points[m + 2];
        for (k, &f) in bin_hz.iter().enumerate() {
            if f >= left && f <= center && center > left {
                fb[m][k] = (f - left) / (center - left);
            } else if f > center && f <= right && right > center {
                fb[m][k] = (right - f) / (right - center);
            }
        }
    }
    fb
}

/// Hann window of length `n`.
fn hann_window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n as f32 - 1.0)).cos())
        .collect()
}

/// Compute an 80 × 3000 log-mel spectrogram from 16 kHz mono samples.
///
/// Frames beyond 30 seconds are truncated; shorter inputs are zero-padded.
pub fn log_mel_spectrogram(samples: &[f32], sample_rate: u32) -> Array2<f32> {
    let window = hann_window(N_FFT);
    let filterbank = mel_filterbank(sample_rate, N_FFT, N_MELS, F_MIN, F_MAX);

    // Pad so the last frame fits.
    let total_samples = (N_FRAMES - 1) * HOP_LENGTH + N_FFT;
    let mut padded = vec![0.0f32; total_samples];
    let copy_len = samples.len().min(total_samples);
    padded[..copy_len].copy_from_slice(&samples[..copy_len]);

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N_FFT);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let mut mel = Array2::<f32>::zeros((N_MELS, N_FRAMES));

    for frame_idx in 0..N_FRAMES {
        let start = frame_idx * HOP_LENGTH;
        for i in 0..N_FFT {
            input[i] = padded[start + i] * window[i];
        }
        if fft.process(&mut input, &mut spectrum).is_err() {
            // Impossible given the fixed sizes, but keep the function infallible.
            continue;
        }
        // Power spectrum.
        let power: Vec<f32> = spectrum
            .iter()
            .map(|c| c.re * c.re + c.im * c.im)
            .collect();
        for m in 0..N_MELS {
            let mut energy = 0.0f32;
            for (k, p) in power.iter().enumerate() {
                energy += p * filterbank[m][k];
            }
            // log10 with a small floor to avoid -inf (Whisper uses 1e-10).
            let log_energy = (energy.max(1e-10)).log10();
            mel[[m, frame_idx]] = log_energy;
        }
    }

    // Whisper-style normalization: clip to a narrow dynamic range then rescale to [-1, 1].
    let max = mel.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let floor = max - 8.0;
    for v in mel.iter_mut() {
        if *v < floor {
            *v = floor;
        }
        *v = (*v + 4.0) / 4.0;
    }

    mel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_linear_identity() {
        let input = vec![0.1f32, 0.2, 0.3, 0.4];
        let out = resample_linear(&input, 16_000, 16_000);
        assert_eq!(out, input);
    }

    #[test]
    fn test_resample_linear_downsample_3x() {
        let input: Vec<f32> = (0..48).map(|i| i as f32).collect();
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16);
    }

    #[test]
    fn test_log_mel_shape_for_30s_clip() {
        let samples = vec![0.0f32; 16_000 * 30];
        let mel = log_mel_spectrogram(&samples, SAMPLE_RATE);
        assert_eq!(mel.shape(), &[N_MELS, N_FRAMES]);
    }

    #[test]
    fn test_log_mel_short_clip_is_zero_padded() {
        let samples = vec![0.0f32; 16_000]; // 1 second of silence
        let mel = log_mel_spectrogram(&samples, SAMPLE_RATE);
        assert_eq!(mel.shape(), &[N_MELS, N_FRAMES]);
    }

    #[test]
    fn test_mel_filterbank_has_expected_shape() {
        let fb = mel_filterbank(SAMPLE_RATE, N_FFT, N_MELS, F_MIN, F_MAX);
        assert_eq!(fb.len(), N_MELS);
        assert_eq!(fb[0].len(), N_FFT / 2 + 1);
        // Filter energy should be non-zero for a midband filter.
        let mid_sum: f32 = fb[N_MELS / 2].iter().sum();
        assert!(mid_sum > 0.0);
    }

    #[test]
    fn test_log_mel_sine_peaks_at_correct_bin() {
        // 1 kHz sine for 1 second at 16 kHz.
        let freq = 1_000.0f32;
        let samples: Vec<f32> = (0..16_000)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / 16_000.0).sin())
            .collect();
        let mel = log_mel_spectrogram(&samples, SAMPLE_RATE);
        // Locate the bin with the highest time-averaged energy.
        let mut best = (0usize, f32::NEG_INFINITY);
        for m in 0..N_MELS {
            let avg: f32 = (0..N_FRAMES).map(|t| mel[[m, t]]).sum::<f32>() / N_FRAMES as f32;
            if avg > best.1 {
                best = (m, avg);
            }
        }
        // 1 kHz falls roughly in the middle third of the 80-bin filterbank.
        assert!(best.0 > 10 && best.0 < 60, "peak mel bin {} unexpected", best.0);
    }
}
