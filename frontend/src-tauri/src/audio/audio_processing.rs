use anyhow::Result;
use chrono::Utc;
use log::{debug, info, warn};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::path::{Path, PathBuf};

/// Sanitize a filename to be safe for filesystem use
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Longest folder name we create: well under the 255-byte limit of common
/// filesystems, leaving room for the timestamp and a collision suffix.
const MAX_FOLDER_NAME_BYTES: usize = 200;
const TIMESTAMP_SUFFIX_BYTES: usize = "_YYYY-MM-DD_HH-MM".len();
const COLLISION_SUFFIX_BYTES: usize = "_1000".len();
const MAX_SANITIZED_NAME_BYTES: usize =
    MAX_FOLDER_NAME_BYTES - TIMESTAMP_SUFFIX_BYTES - COLLISION_SUFFIX_BYTES;
const MAX_FOLDER_ATTEMPTS: u32 = 1000;

/// Creates `base_path/<base_name>`, or `<base_name>_2`, `_3`, ... if that name is
/// taken. `create_dir` (not `create_dir_all`) is what makes the claim exclusive,
/// so two imports or recordings with the same name never share a folder.
fn create_unique_dir(base_path: &Path, base_name: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(base_path)?;
    for attempt in 1..=MAX_FOLDER_ATTEMPTS {
        let name = if attempt == 1 {
            base_name.to_string()
        } else {
            format!("{}_{}", base_name, attempt)
        };
        let candidate = base_path.join(name);
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!(
        "no free meeting folder name for '{}' after {} attempts",
        base_name,
        MAX_FOLDER_ATTEMPTS
    ))
}

/// Create a meeting folder with timestamp and return the path
/// Creates structure: base_path/MeetingName_YYYY-MM-DD_HH-MM/
///                    ├── .checkpoints/  (for incremental saves, optional)
///
/// # Arguments
/// * `base_path` - Base directory for meetings
/// * `meeting_name` - Name of the meeting
/// * `create_checkpoints_dir` - Whether to create .checkpoints/ subdirectory (only needed when auto_save is true)
pub fn create_meeting_folder(
    base_path: &PathBuf,
    meeting_name: &str,
    create_checkpoints_dir: bool,
) -> Result<PathBuf> {
    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M").to_string();
    let mut sanitized_name = sanitize_filename(meeting_name);
    crate::utils::truncate_to_char_boundary(&mut sanitized_name, MAX_SANITIZED_NAME_BYTES);
    let base_name = format!("{}_{}", sanitized_name, timestamp);

    // Create main meeting folder
    let meeting_folder = create_unique_dir(base_path, &base_name)?;

    // Only create .checkpoints subdirectory if requested (when auto_save is true)
    if create_checkpoints_dir {
        let checkpoints_dir = meeting_folder.join(".checkpoints");
        std::fs::create_dir_all(&checkpoints_dir)?;
        log::info!("Created meeting folder with checkpoints");
    } else {
        log::info!("Created meeting folder without checkpoints");
    }

    Ok(meeting_folder)
}

/// True peak limiter with lookahead buffer (prevents clipping)
struct TruePeakLimiter {
    lookahead_samples: usize,
    buffer: Vec<f32>,
    gain_reduction: Vec<f32>,
    current_position: usize,
}

impl TruePeakLimiter {
    fn new(sample_rate: u32) -> Self {
        const LIMITER_LOOKAHEAD_MS: usize = 10;
        let lookahead_samples = ((sample_rate as usize * LIMITER_LOOKAHEAD_MS) / 1000).max(1);

        Self {
            lookahead_samples,
            buffer: vec![0.0; lookahead_samples],
            gain_reduction: vec![1.0; lookahead_samples],
            current_position: 0,
        }
    }

    fn process(&mut self, sample: f32, true_peak_limit: f32) -> f32 {
        self.buffer[self.current_position] = sample;

        let sample_abs = sample.abs();
        if sample_abs > true_peak_limit {
            let reduction = true_peak_limit / sample_abs;
            self.gain_reduction[self.current_position] = reduction;
        } else {
            self.gain_reduction[self.current_position] = 1.0;
        }

        // Wrap with a compare instead of `%`: this runs 48,000 times a second
        // on the realtime audio thread, and an integer division per sample is
        // the most expensive thing in the loop.
        let mut output_position = self.current_position + 1;
        if output_position >= self.lookahead_samples {
            output_position = 0;
        }
        let output_sample = self.buffer[output_position] * self.gain_reduction[output_position];

        self.current_position = output_position;
        output_sample
    }
}

/// Professional loudness normalizer using EBU R128 standard
/// This is a STATEFUL normalizer that tracks cumulative loudness over time
///
/// EBU R128 is the broadcast industry standard for loudness normalization:
/// - Target: -23 LUFS (Loudness Units relative to Full Scale)
/// - Used by: Netflix, YouTube, Spotify, all professional broadcast
/// - Perceptually accurate (not just simple RMS)
///
pub struct LoudnessNormalizer {
    ebur128: ebur128::EbuR128,
    limiter: TruePeakLimiter,
    gain_linear: f32,
    loudness_buffer: Vec<f32>,
    true_peak_limit: f32,
    /// Analysis chunks seen since the last `loudness_global()` call.
    chunks_since_gain_update: u32,
}

impl LoudnessNormalizer {
    /// Create a new EBU R128 loudness normalizer
    ///
    /// # Arguments
    /// * `channels` - Number of audio channels (1 for mono, 2 for stereo)
    /// * `sample_rate` - Sample rate in Hz (e.g., 48000)
    pub fn new(channels: u32, sample_rate: u32) -> Result<Self> {
        const TRUE_PEAK_LIMIT: f64 = -1.0;
        const ANALYZE_CHUNK_SIZE: usize = 512;

        // Mode::I only. TRUE_PEAK makes ebur128 oversample every frame
        // internally, and nothing here ever reads `true_peak()` — peak control
        // is done by the separate TruePeakLimiter below.
        let ebur128 = ebur128::EbuR128::new(channels, sample_rate, ebur128::Mode::I)
            .map_err(|e| anyhow::anyhow!("Failed to create EBU R128 normalizer: {}", e))?;

        let true_peak_limit = 10_f32.powf(TRUE_PEAK_LIMIT as f32 / 20.0);

        Ok(Self {
            ebur128,
            limiter: TruePeakLimiter::new(sample_rate),
            gain_linear: 1.0,
            loudness_buffer: Vec::with_capacity(ANALYZE_CHUNK_SIZE * 2),
            true_peak_limit,
            chunks_since_gain_update: 0,
        })
    }

    /// Normalize loudness using EBU R128 standard with true peak limiting
    ///
    /// This maintains cumulative loudness measurements across all processed audio,
    /// resulting in consistent normalization that sounds natural.
    ///
    /// Target: -23 LUFS (professional broadcast standard for speech/dialog)
    /// Applies sample-by-sample with 10ms lookahead limiter to prevent clipping
    /// Allocating wrapper for offline callers. The realtime capture path uses
    /// [`normalize_in_place`](Self::normalize_in_place).
    pub fn normalize_loudness(&mut self, samples: &[f32]) -> Vec<f32> {
        let mut out = samples.to_vec();
        self.normalize_in_place(&mut out);
        out
    }

    /// Same normalization, applied in place — no allocation per audio callback.
    pub fn normalize_in_place(&mut self, samples: &mut [f32]) {
        if samples.is_empty() {
            return;
        }

        const TARGET_LUFS: f64 = -23.0;
        const MAX_GAIN_DB: f64 = 12.0;
        const NOISE_FLOOR_LUFS: f64 = -50.0;
        const ANALYZE_CHUNK_SIZE: usize = 512;
        // `loudness_global()` integrates the whole gated histogram. The gain it
        // produces is a slow-moving normalization target, so recomputing it on
        // every 512-sample chunk (~94x/s) is wasted work; ~2x/s tracks it just
        // as well.
        const GAIN_UPDATE_EVERY_CHUNKS: u32 = 48;

        for block in samples.chunks_mut(ANALYZE_CHUNK_SIZE) {
            // Accumulate samples for loudness analysis in bulk rather than
            // pushing one at a time.
            self.loudness_buffer.extend_from_slice(block);

            if self.loudness_buffer.len() >= ANALYZE_CHUNK_SIZE {
                if let Err(e) = self.ebur128.add_frames_f32(&self.loudness_buffer) {
                    warn!("Failed to add frames to EBU R128: {}", e);
                } else {
                    self.chunks_since_gain_update += 1;
                    if self.chunks_since_gain_update >= GAIN_UPDATE_EVERY_CHUNKS {
                        self.chunks_since_gain_update = 0;

                        // Update gain based on cumulative loudness
                        if let Ok(current_lufs) = self.ebur128.loudness_global() {
                            // Below the noise floor there is nothing to
                            // normalise: the absolute gate only drops audio
                            // under -70 LUFS, so a muted mic's room noise
                            // would otherwise get +30 dB or more.
                            if current_lufs.is_finite()
                                && current_lufs < 0.0
                                && current_lufs >= NOISE_FLOOR_LUFS
                            {
                                let gain_db = (TARGET_LUFS - current_lufs)
                                    .clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
                                self.gain_linear = 10_f32.powf(gain_db as f32 / 20.0);
                            }
                        }
                    }
                }
                self.loudness_buffer.clear();
            }

            // Apply gain and true peak limiting
            let gain = self.gain_linear;
            let limit = self.true_peak_limit;
            for sample in block.iter_mut() {
                *sample = self.limiter.process(*sample * gain, limit);
            }
        }
    }
}

/// High-pass filter to remove low-frequency rumble and noise
/// Removes frequencies below cutoff_hz (typically 80-100 Hz for speech)
pub struct HighPassFilter {
    #[allow(dead_code)]
    sample_rate: f32,
    #[allow(dead_code)]
    cutoff_hz: f32,
    // First-order IIR filter coefficients
    alpha: f32,
    prev_input: f32,
    prev_output: f32,
}

impl HighPassFilter {
    /// Create a new high-pass filter
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `cutoff_hz` - Cutoff frequency in Hz (typical: 80-100 Hz for speech)
    pub fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        let sample_rate_f = sample_rate as f32;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
        let dt = 1.0 / sample_rate_f;
        let alpha = rc / (rc + dt);

        info!("Initializing high-pass filter: cutoff={}Hz @ {}Hz", cutoff_hz, sample_rate);

        Self {
            sample_rate: sample_rate_f,
            cutoff_hz,
            alpha,
            prev_input: 0.0,
            prev_output: 0.0,
        }
    }

    /// Apply high-pass filter to audio samples
    /// Uses first-order IIR (Infinite Impulse Response) filter
    ///
    /// Allocating wrapper for offline callers (decoder, import). The realtime
    /// capture path uses [`process_in_place`](Self::process_in_place).
    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        let mut output = samples.to_vec();
        self.process_in_place(&mut output);
        output
    }

    /// Same filter, applied in place — no allocation per audio callback.
    pub fn process_in_place(&mut self, samples: &mut [f32]) {
        for sample in samples.iter_mut() {
            // First-order high-pass IIR filter formula:
            // y[n] = alpha * (y[n-1] + x[n] - x[n-1])
            let filtered = self.alpha * (self.prev_output + *sample - self.prev_input);

            self.prev_input = *sample;
            self.prev_output = filtered;

            *sample = filtered;
        }
    }

    /// Reset filter state (call when starting new recording)
    pub fn reset(&mut self) {
        self.prev_input = 0.0;
        self.prev_output = 0.0;
    }
}

pub fn audio_to_mono(audio: &[f32], channels: u16) -> Vec<f32> {
    let mut mono_samples = Vec::with_capacity(audio.len() / channels.max(1) as usize);
    audio_to_mono_into(audio, channels, &mut mono_samples);
    mono_samples
}

/// Downmix into a caller-owned buffer, which the realtime capture path reuses
/// across callbacks instead of allocating one per chunk. `out` is cleared first.
pub fn audio_to_mono_into(audio: &[f32], channels: u16, out: &mut Vec<f32>) {
    out.clear();

    if channels <= 1 {
        out.extend_from_slice(audio);
        return;
    }

    out.reserve(audio.len() / channels as usize);

    // For microphone arrays (> 2 channels), only use first 2 channels
    // Many microphone arrays have auxiliary channels for beam-forming/noise cancellation
    // that can contain anti-phase signals. Averaging all channels can cause destructive
    // interference resulting in near-zero output.
    let effective_channels = if channels > 2 { 2 } else { channels };
    let scale = 1.0 / effective_channels as f32;

    // Iterate over the audio slice in chunks, each containing `channels` samples
    for chunk in audio.chunks(channels as usize) {
        // Sum only the first effective_channels (typically 1-2 for mic arrays)
        let sum: f32 = chunk.iter().take(effective_channels as usize).sum();
        out.push(sum * scale);
    }
}

/// High-quality audio resampling with adaptive parameters based on sample rate ratio
///
/// This function automatically selects the best resampling parameters based on:
/// - Sample rate ratio (upsampling vs downsampling)
/// - Quality requirements (integer ratios get optimized paths)
/// - Anti-aliasing needs
///
/// Supports all common sample rates: 8kHz, 16kHz, 24kHz, 44.1kHz, 48kHz, etc.
pub fn resample(input: &[f32], from_sample_rate: u32, to_sample_rate: u32) -> Result<Vec<f32>> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    // Fast path: No resampling needed
    if from_sample_rate == to_sample_rate {
        return Ok(input.to_vec());
    }

    let ratio = to_sample_rate as f64 / from_sample_rate as f64;

    // Adaptive parameters based on sample rate ratio
    let (sinc_len, interpolation_type, oversampling) = if ratio >= 2.0 {
        // Large upsampling (e.g., 8kHz → 16kHz, 16kHz → 48kHz, 24kHz → 48kHz)
        // Needs high quality to avoid artifacts
        debug!("High-quality upsampling: {}Hz → {}Hz (ratio: {:.2}x)",
               from_sample_rate, to_sample_rate, ratio);
        (
            512,                              // Longer sinc for smoother interpolation
            SincInterpolationType::Cubic,     // Cubic for best quality
            512,                              // Higher oversampling
        )
    } else if ratio >= 1.5 {
        // Moderate upsampling (e.g., 32kHz → 48kHz)
        debug!("Moderate upsampling: {}Hz → {}Hz (ratio: {:.2}x)",
               from_sample_rate, to_sample_rate, ratio);
        (
            384,
            SincInterpolationType::Cubic,
            384,
        )
    } else if ratio > 1.0 {
        // Small upsampling (e.g., 44.1kHz → 48kHz)
        debug!("Small upsampling: {}Hz → {}Hz (ratio: {:.2}x)",
               from_sample_rate, to_sample_rate, ratio);
        (
            256,
            SincInterpolationType::Linear,
            256,
        )
    } else if ratio <= 0.5 {
        // Large downsampling (e.g., 48kHz → 16kHz, 48kHz → 8kHz)
        // Needs strong anti-aliasing
        debug!("Anti-aliased downsampling: {}Hz → {}Hz (ratio: {:.2}x)",
               from_sample_rate, to_sample_rate, ratio);
        (
            512,                              // Longer sinc for anti-aliasing
            SincInterpolationType::Cubic,     // Cubic for quality
            512,
        )
    } else {
        // Moderate downsampling (e.g., 48kHz → 24kHz, 48kHz → 32kHz)
        debug!("Moderate downsampling: {}Hz → {}Hz (ratio: {:.2}x)",
               from_sample_rate, to_sample_rate, ratio);
        (
            384,
            SincInterpolationType::Linear,
            384,
        )
    };

    let params = SincInterpolationParameters {
        sinc_len,
        f_cutoff: 0.95,                      // Preserve most of the frequency content
        interpolation: interpolation_type,
        oversampling_factor: oversampling,
        window: WindowFunction::BlackmanHarris2,  // Best window for audio
    };

    let mut resampler = SincFixedIn::<f32>::new(
        ratio,
        2.0,  // Maximum relative deviation
        params,
        input.len(),
        1,    // Mono
    )?;

    let waves_in = vec![input.to_vec()];
    let waves_out = resampler.process(&waves_in, None)?;

    debug!("Resampling complete: {} samples → {} samples",
           input.len(), waves_out[0].len());

    Ok(waves_out.into_iter().next().unwrap())
}

// Alias for compatibility with existing code
pub fn resample_audio(input: &[f32], from_sample_rate: u32, to_sample_rate: u32) -> Vec<f32> {
    match resample(input, from_sample_rate, to_sample_rate) {
        Ok(result) => result,
        Err(e) => {
            debug!("Resampling failed: {}, returning original audio", e);
            input.to_vec()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_title_same_minute_gets_separate_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let first = create_meeting_folder(&base, "Weekly Sync", false).unwrap();
        let second = create_meeting_folder(&base, "Weekly Sync", false).unwrap();
        assert_ne!(first, second, "two imports of the same title got the same folder");
    }

    #[test]
    fn long_title_folder_stays_within_filename_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        // 250 ASCII bytes + "_YYYY-MM-DD_HH-MM" (17 bytes) = 267 bytes, over the 255-byte
        // NAME_MAX of ext4 and the 255-character APFS/NTFS limits. (85 CJK chars
        // are only 85 characters on APFS/NTFS, so they do not hit the limit there.)
        let result = create_meeting_folder(&base, &"a".repeat(250), false);
        assert!(result.is_ok(), "create_meeting_folder failed: {:?}", result.err());
    }

    #[test]
    fn long_multibyte_title_folder_is_created_within_byte_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let folder = create_meeting_folder(&base, &"会".repeat(85), false).unwrap();
        assert!(folder.is_dir());
        let name = folder.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.len() <= 200, "folder name is {} bytes", name.len());
    }

    const SR: u32 = 48_000;

    /// Deterministic white noise scaled so its integrated EBU R128 loudness is `lufs`.
    fn noise_at_lufs(lufs: f64, seconds: usize) -> Vec<f32> {
        let mut state: u32 = 0x1234_5678;
        let mut samples: Vec<f32> = (0..SR as usize * seconds)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
            })
            .collect();
        let mut meter = ebur128::EbuR128::new(1, SR, ebur128::Mode::I).unwrap();
        meter.add_frames_f32(&samples).unwrap();
        let measured = meter.loudness_global().unwrap();
        let scale = 10_f32.powf((lufs - measured) as f32 / 20.0);
        for s in samples.iter_mut() {
            *s *= scale;
        }
        samples
    }

    fn gain_db_after(input: &[f32]) -> f32 {
        let mut normalizer = LoudnessNormalizer::new(1, SR).unwrap();
        // Feed in callback-sized pieces, as the capture path does.
        let mut buf = input.to_vec();
        for chunk in buf.chunks_mut(480) {
            normalizer.normalize_in_place(chunk);
        }
        20.0 * normalizer.gain_linear.log10()
    }

    #[test]
    fn mic_noise_floor_is_not_amplified() {
        let gain_db = gain_db_after(&noise_at_lufs(-58.0, 5));
        assert!(gain_db <= 0.01, "room noise at -58 LUFS got {:.1} dB of gain", gain_db);
    }

    #[test]
    fn quiet_speech_gain_is_capped() {
        let gain_db = gain_db_after(&noise_at_lufs(-40.0, 5));
        assert!(gain_db > 11.5 && gain_db <= 12.01, "-40 LUFS got {:.1} dB, want the +12 dB cap", gain_db);
    }

    #[test]
    fn moderate_speech_is_raised_to_target_within_cap() {
        let gain_db = gain_db_after(&noise_at_lufs(-30.0, 5));
        assert!((gain_db - 7.0).abs() < 0.5, "-30 LUFS got {:.1} dB, want ~+7 dB", gain_db);
    }

    #[test]
    fn loud_content_still_converges_to_target() {
        let gain_db = gain_db_after(&noise_at_lufs(-20.0, 5));
        assert!((gain_db + 3.0).abs() < 0.5, "-20 LUFS got {:.1} dB, want ~-3 dB", gain_db);
    }
}
