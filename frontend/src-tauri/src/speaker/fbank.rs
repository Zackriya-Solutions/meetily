//! Kaldi-compatible log-mel filterbank features for the speaker embedding model.
//!
//! Speaker-embedding models exported from 3D-Speaker / WeSpeaker are trained on
//! Kaldi `compute-fbank-feats` output, so this mirrors Kaldi's defaults:
//! 25 ms Povey-windowed frames every 10 ms, DC removal, 0.97 pre-emphasis,
//! power spectrum on a 512-point FFT, 80 triangular mel bins (20 Hz..Nyquist),
//! natural-log energies, `snip_edges = true`, no dither.

use realfft::RealFftPlanner;
use std::sync::Arc;

pub const SAMPLE_RATE: usize = 16_000;
pub const NUM_MEL_BINS: usize = 80;
const FRAME_LENGTH: usize = 400; // 25 ms at 16 kHz
const FRAME_SHIFT: usize = 160; // 10 ms at 16 kHz
const FFT_SIZE: usize = 512;
const PREEMPHASIS: f32 = 0.97;
const LOW_FREQ: f32 = 20.0;

pub struct FbankExtractor {
    window: Vec<f32>,
    /// mel_banks[bin] = (first FFT bin index, weights)
    mel_banks: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn realfft::RealToComplex<f32>>,
}

fn mel_scale(freq: f32) -> f32 {
    1127.0 * (1.0 + freq / 700.0).ln()
}

impl FbankExtractor {
    pub fn new() -> Self {
        let window = (0..FRAME_LENGTH)
            .map(|i| {
                let hann = 0.5
                    - 0.5
                        * (2.0 * std::f32::consts::PI * i as f32 / (FRAME_LENGTH - 1) as f32)
                            .cos();
                hann.powf(0.85)
            })
            .collect();

        let num_fft_bins = FFT_SIZE / 2;
        let nyquist = SAMPLE_RATE as f32 / 2.0;
        let fft_bin_width = SAMPLE_RATE as f32 / FFT_SIZE as f32;
        let mel_low = mel_scale(LOW_FREQ);
        let mel_high = mel_scale(nyquist);
        let mel_delta = (mel_high - mel_low) / (NUM_MEL_BINS + 1) as f32;

        let mel_banks = (0..NUM_MEL_BINS)
            .map(|bin| {
                let left = mel_low + bin as f32 * mel_delta;
                let center = left + mel_delta;
                let right = center + mel_delta;
                let mut first = None;
                let mut weights = Vec::new();
                for i in 0..num_fft_bins {
                    let mel = mel_scale(fft_bin_width * i as f32);
                    if mel > left && mel < right {
                        let weight = if mel <= center {
                            (mel - left) / (center - left)
                        } else {
                            (right - mel) / (right - center)
                        };
                        first.get_or_insert(i);
                        weights.push(weight);
                    } else if first.is_some() {
                        break;
                    }
                }
                (first.unwrap_or(0), weights)
            })
            .collect();

        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        Self { window, mel_banks, fft }
    }

    /// Number of frames Kaldi produces for `num_samples` with `snip_edges = true`.
    pub fn num_frames(num_samples: usize) -> usize {
        if num_samples < FRAME_LENGTH {
            0
        } else {
            1 + (num_samples - FRAME_LENGTH) / FRAME_SHIFT
        }
    }

    /// Computes `[num_frames * NUM_MEL_BINS]` row-major features for 16 kHz audio.
    /// `scale` is applied to samples first (1.0 for [-1, 1] models, 32768.0 for
    /// models trained on int16-range input).
    pub fn compute(&self, samples: &[f32], scale: f32) -> Vec<f32> {
        let num_frames = Self::num_frames(samples.len());
        let mut features = Vec::with_capacity(num_frames * NUM_MEL_BINS);
        let mut frame = vec![0.0f32; FFT_SIZE];
        let mut spectrum = self.fft.make_output_vec();
        let mut scratch = self.fft.make_scratch_vec();
        let mut power = vec![0.0f32; FFT_SIZE / 2 + 1];

        for f in 0..num_frames {
            let start = f * FRAME_SHIFT;
            let src = &samples[start..start + FRAME_LENGTH];
            for (dst, &s) in frame.iter_mut().zip(src) {
                *dst = s * scale;
            }
            let mean = frame[..FRAME_LENGTH].iter().sum::<f32>() / FRAME_LENGTH as f32;
            frame[..FRAME_LENGTH].iter_mut().for_each(|s| *s -= mean);
            for i in (1..FRAME_LENGTH).rev() {
                frame[i] -= PREEMPHASIS * frame[i - 1];
            }
            frame[0] -= PREEMPHASIS * frame[0];
            for (s, w) in frame[..FRAME_LENGTH].iter_mut().zip(&self.window) {
                *s *= w;
            }
            frame[FRAME_LENGTH..].iter_mut().for_each(|s| *s = 0.0);

            if self
                .fft
                .process_with_scratch(&mut frame, &mut spectrum, &mut scratch)
                .is_err()
            {
                features.extend(std::iter::repeat(f32::EPSILON.ln()).take(NUM_MEL_BINS));
                continue;
            }
            for (p, c) in power.iter_mut().zip(&spectrum) {
                *p = c.norm_sqr();
            }
            for (first, weights) in &self.mel_banks {
                let energy: f32 = weights
                    .iter()
                    .zip(&power[*first..])
                    .map(|(w, p)| w * p)
                    .sum();
                features.push(energy.max(f32::EPSILON).ln());
            }
        }
        features
    }
}

impl Default for FbankExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_count_matches_kaldi_snip_edges() {
        assert_eq!(FbankExtractor::num_frames(399), 0);
        assert_eq!(FbankExtractor::num_frames(400), 1);
        assert_eq!(FbankExtractor::num_frames(16_000), 98);
    }

    #[test]
    fn tone_energy_lands_in_matching_mel_bin() {
        let extractor = FbankExtractor::new();
        let tone: Vec<f32> = (0..16_000)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 16_000.0).sin() * 0.5)
            .collect();
        let feats = extractor.compute(&tone, 1.0);
        assert_eq!(feats.len(), 98 * NUM_MEL_BINS);
        let frame = &feats[40 * NUM_MEL_BINS..41 * NUM_MEL_BINS];
        let peak = frame
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap();
        // 1 kHz (1000 mel) is centred on bin ~27 of 80 for a 20 Hz..8 kHz filterbank.
        assert!((25..=29).contains(&peak), "peak bin {peak}");
    }
}
