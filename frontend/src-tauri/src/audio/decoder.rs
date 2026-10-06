// Audio file decoder for retranscription feature
// Uses Symphonia to decode MP4/AAC audio files, with ffmpeg fallback for
// formats Symphonia can't handle (MKV, WebM, WMA)

use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use rayon::prelude::*;
use std::borrow::Cow;
use std::path::Path;
use std::process::{Command, Stdio};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::TimeBase;

use super::audio_processing::{audio_to_mono, resample, resample_audio};
use super::ffmpeg::find_ffmpeg_path;

/// Extensions requiring ffmpeg pre-conversion (Symphonia lacks these demuxers/codecs)
const FFMPEG_ONLY_EXTENSIONS: &[&str] = &["mkv", "webm", "wma"];

/// Progress callback for long-running operations
/// Returns current progress (0-100) and a message
pub type ProgressCallback = Box<dyn Fn(u32, &str) + Send>;

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
    /// Convert decoded audio to Whisper-compatible 16kHz mono f32 format.
    ///
    /// Performs mono conversion, normalization, and resampling. Large files
    /// (>5 min at 48kHz) use chunked sinc resampling to keep memory bounded
    /// while preserving audio quality for downstream VAD and transcription.
    pub fn to_whisper_format(&self) -> Vec<f32> {
        self.to_whisper_format_with_progress(None)
    }

    /// Convert decoded audio to Whisper format with optional progress callback
    pub fn to_whisper_format_with_progress(&self, progress_callback: Option<ProgressCallback>) -> Vec<f32> {
        whisper_format_from(Cow::Borrowed(&self.samples), self.sample_rate, self.channels, progress_callback)
    }

    /// Like `to_whisper_format`, but consumes the decoded audio instead of copying its samples,
    /// which keeps the peak memory of long recordings down.
    pub fn into_whisper_format(self) -> Vec<f32> {
        self.into_whisper_format_with_progress(None)
    }

    /// Consuming variant of `to_whisper_format_with_progress`.
    pub fn into_whisper_format_with_progress(self, progress_callback: Option<ProgressCallback>) -> Vec<f32> {
        let DecodedAudio { samples, sample_rate, channels, .. } = self;
        whisper_format_from(Cow::Owned(samples), sample_rate, channels, progress_callback)
    }
}

/// Mono conversion, normalisation and resampling to 16 kHz. Owned `samples` are consumed, so no
/// full-length copy is made; borrowed ones are copied only when already mono.
fn whisper_format_from(
    samples: Cow<'_, [f32]>,
    sample_rate: u32,
    channels: u16,
    progress_callback: Option<ProgressCallback>,
) -> Vec<f32> {
    // Step 1: Convert to mono if needed (the interleaved samples are freed right away)
    let mono_samples = if channels > 1 {
        info!("Converting {} channels to mono ({} samples)", channels, samples.len());
        let mono = audio_to_mono(&samples, channels);
        drop(samples);
        mono
    } else {
        samples.into_owned()
    };

    // Step 1.5: Normalize samples to valid range (-1.0 to 1.0)
    // Some audio files may have samples slightly outside this range
    let mono_samples = normalize_audio_samples(mono_samples);

    // Step 2: Resample to 16kHz if needed
    const WHISPER_SAMPLE_RATE: u32 = 16000;
    if sample_rate != WHISPER_SAMPLE_RATE {
        // Large files are processed in chunks through the sinc resampler
        // to keep memory bounded while preserving audio quality.
        // Linear interpolation (fast_resample) was removed because it lacks
        // an anti-aliasing filter, causing aliasing artifacts that make VAD
        // miss ~99% of speech in long recordings.
        const LARGE_FILE_THRESHOLD: usize = 14_400_000;

        let mut resampled = if mono_samples.len() > LARGE_FILE_THRESHOLD {
            info!(
                "Chunked sinc resampling {} samples from {}Hz to {}Hz (large file mode)",
                mono_samples.len(),
                sample_rate,
                WHISPER_SAMPLE_RATE
            );
            chunked_resample_with_progress(&mono_samples, sample_rate, WHISPER_SAMPLE_RATE, progress_callback)
        } else {
            info!(
                "Resampling {} samples from {}Hz to {}Hz",
                mono_samples.len(),
                sample_rate,
                WHISPER_SAMPLE_RATE
            );
            resample_audio(&mono_samples, sample_rate, WHISPER_SAMPLE_RATE)
        };
        drop(mono_samples);

        // Clamp after resampling: the sinc resampler can overshoot
        // slightly beyond [-1.0, 1.0] (Gibbs phenomenon), which causes
        // VAD to reject samples with "Float sample must be in the range -1.0 to 1.0"
        for s in &mut resampled {
            *s = s.clamp(-1.0, 1.0);
        }
        resampled
    } else {
        mono_samples
    }
}

/// Resample large audio files in fixed-size chunks through the sinc resampler.
///
/// Processes `input` in 60-second chunks using the high-quality sinc resampler
/// from [`resample_audio`], concatenating the results. This avoids the memory
/// spike of resampling the entire file at once while preserving anti-aliasing
/// quality that is critical for downstream VAD accuracy.
///
/// Chunked resampling with optional progress callback.
///
/// Resamples `input` via [`rayon`] in bounded parallel batches of 60-second
/// chunks (one chunk per worker thread per batch), and merges each batch
/// sequentially with a 100ms cross-fade before the next batch starts, so at
/// most one batch of resampled chunks is held beside the output. Each chunk's
/// [`resample`] call is independent and CPU-bound.
///
/// Falls back to [`resample_audio`] (single-pass sinc) if any chunk fails.
fn chunked_resample_with_progress(
    input: &[f32],
    from_rate: u32,
    to_rate: u32,
    progress_callback: Option<ProgressCallback>,
) -> Vec<f32> {
    if input.is_empty() || from_rate == to_rate {
        return input.to_vec();
    }

    // 60 seconds of audio at the source sample rate per chunk
    let chunk_samples = from_rate as usize * 60;
    // 100ms overlap in the input domain to cross-fade between chunks
    let overlap_input = from_rate as usize / 10;
    let ratio = to_rate as f64 / from_rate as f64;
    let overlap_output = (overlap_input as f64 * ratio) as usize;
    let estimated_output = (input.len() as f64 * ratio) as usize + 1024;

    // Build overlapping chunk boundaries
    let mut chunk_ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    while start < input.len() {
        let end = (start + chunk_samples + overlap_input).min(input.len());
        chunk_ranges.push((start, end));
        start += chunk_samples;
    }

    let total_chunks = chunk_ranges.len();
    info!(
        "Parallel chunked sinc resampling: {} chunks of ~60s each with 100ms cross-fade ({} total samples)",
        total_chunks,
        input.len()
    );

    // Resample one batch of chunks in parallel (one chunk per worker thread) and merge it before
    // the next batch, so at most one batch of resampled chunks is held beside the output.
    let batch_size = rayon::current_num_threads().max(1);
    let mut output = Vec::with_capacity(estimated_output);
    for (batch_idx, batch) in chunk_ranges.chunks(batch_size).enumerate() {
        let resampled_batch: Vec<Result<Vec<f32>>> = batch
            .par_iter()
            .map(|&(chunk_start, chunk_end)| resample(&input[chunk_start..chunk_end], from_rate, to_rate))
            .collect();

        // Merge sequentially with cross-fade (order-dependent, must be serial)
        for (offset, result) in resampled_batch.into_iter().enumerate() {
            let chunk_idx = batch_idx * batch_size + offset;
            match result {
                Ok(resampled) => {
                    if chunk_idx == 0 {
                        output.extend_from_slice(&resampled);
                    } else {
                        // Cross-fade the overlap region with the tail of the previous output
                        let fade_len = overlap_output.min(resampled.len()).min(output.len());
                        if fade_len > 0 {
                            let out_start = output.len() - fade_len;
                            for i in 0..fade_len {
                                let t = i as f32 / fade_len as f32;
                                output[out_start + i] =
                                    output[out_start + i] * (1.0 - t) + resampled[i] * t;
                            }
                            if fade_len < resampled.len() {
                                output.extend_from_slice(&resampled[fade_len..]);
                            }
                        } else {
                            output.extend_from_slice(&resampled);
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Resampling failed on chunk {}/{}: {}, falling back to single-pass sinc resampler",
                        chunk_idx + 1,
                        total_chunks,
                        e
                    );
                    return resample_audio(input, from_rate, to_rate);
                }
            }

            if let Some(callback) = &progress_callback {
                let progress_pct = ((chunk_idx + 1) as f64 / total_chunks as f64) * 100.0;
                if (chunk_idx + 1) % 10 == 0 || chunk_idx + 1 == total_chunks {
                    info!(
                        "Resampling progress: {}/{} chunks ({:.0}%)",
                        chunk_idx + 1,
                        total_chunks,
                        progress_pct
                    );
                }
                callback(
                    progress_pct as u32,
                    &format!("Resampling audio: {:.0}%", progress_pct),
                );
            }
        }
    }

    info!(
        "Parallel chunked sinc resampling complete: {} -> {} samples",
        input.len(),
        output.len()
    );
    output
}

/// Normalize audio samples to the valid range (-1.0 to 1.0)
/// This handles audio files that may have samples slightly outside the expected range
fn normalize_audio_samples(mut samples: Vec<f32>) -> Vec<f32> {
    // First, find the maximum absolute value
    let max_abs = samples
        .iter()
        .filter(|s| s.is_finite())
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

/// Check if a file extension requires ffmpeg pre-conversion
fn needs_ffmpeg_conversion(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| FFMPEG_ONLY_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Convert an audio file to WAV using ffmpeg for formats Symphonia can't decode.
///
/// Returns a `TempPath` that auto-deletes the temporary WAV file when dropped.
/// The caller must keep the `TempPath` alive until decoding of the WAV is complete.
fn convert_to_wav_with_ffmpeg(
    input_path: &Path,
    progress_callback: Option<&ProgressCallback>,
) -> Result<tempfile::TempPath> {
    let ffmpeg_path = find_ffmpeg_path().ok_or_else(|| {
        anyhow!(
            "FFmpeg not found. FFmpeg is required to decode .{} files. \
             It will be downloaded automatically on next launch, or install it manually.",
            input_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("this format")
        )
    })?;

    // Create temp file in the same directory as the input to avoid cross-device issues
    let parent_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let temp_file = tempfile::Builder::new()
        .prefix(".meetily_decode_")
        .suffix(".wav")
        .tempfile_in(parent_dir)
        .map_err(|e| anyhow!("Failed to create temporary WAV file: {}", e))?;

    let temp_path = temp_file.into_temp_path();

    info!(
        "Converting .{} to temporary WAV via ffmpeg: {} -> {}",
        input_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown"),
        input_path.display(),
        temp_path.display()
    );

    if let Some(cb) = progress_callback {
        cb(0, "Converting audio format with FFmpeg...");
    }

    let input_str = input_path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid input path (non-UTF8)"))?;
    let output_str = temp_path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid temp path (non-UTF8)"))?;

    let mut command = Command::new(&ffmpeg_path);
    command
        .args([
            "-i", input_str,
            "-vn",                  // Strip video tracks
            "-acodec", "pcm_s16le", // Output PCM WAV (Symphonia handles natively)
            "-y",                   // Overwrite without prompt
            output_str,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    debug!("FFmpeg conversion command: {:?}", command);

    #[allow(clippy::zombie_processes)]
    let child = command
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn ffmpeg process: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| anyhow!("Failed to wait for ffmpeg process: {}", e))?;

    let stderr_text = String::from_utf8_lossy(&output.stderr);
    debug!("FFmpeg stderr: {}", stderr_text);

    if !output.status.success() {
        error!(
            "FFmpeg conversion failed (exit code: {}): {}",
            output.status, stderr_text
        );
        return Err(anyhow!(
            "FFmpeg conversion failed with exit code: {}. \
             The file may be corrupted or in an unsupported format.",
            output.status
        ));
    }

    // Verify output file exists and has content
    let output_meta = std::fs::metadata(&temp_path)
        .map_err(|e| anyhow!("FFmpeg output file not found: {}", e))?;

    if output_meta.len() == 0 {
        return Err(anyhow!(
            "FFmpeg produced an empty output file. The input may contain no audio."
        ));
    }

    if let Some(cb) = progress_callback {
        cb(100, "FFmpeg conversion complete");
    }

    info!(
        "FFmpeg conversion complete: {} bytes output",
        output_meta.len()
    );

    Ok(temp_path)
}

/// Decode an audio file (MP4, M4A, WAV, etc.) to raw samples
pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio> {
    decode_audio_file_with_progress(path, None)
}

/// Decode an audio file with optional progress callback
pub fn decode_audio_file_with_progress(
    path: &Path,
    progress_callback: Option<ProgressCallback>,
) -> Result<DecodedAudio> {
    info!("Decoding audio file: {}", path.display());

    // FFmpeg pre-conversion for unsupported formats (MKV, WebM, WMA).
    // If the file is in a format Symphonia can't decode, use ffmpeg to convert
    // it to a temporary WAV file first, then decode the WAV with Symphonia.
    // The _temp_wav_guard keeps the temp file alive until decoding completes,
    // then auto-deletes it when dropped (even on error/panic).
    let (_temp_wav_guard, decode_path): (Option<tempfile::TempPath>, Cow<'_, Path>) =
        if needs_ffmpeg_conversion(path) {
            info!(
                "Format requires ffmpeg pre-conversion: .{}",
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("unknown")
            );
            let temp_path = convert_to_wav_with_ffmpeg(path, progress_callback.as_ref())?;
            let wav_path = temp_path.to_path_buf();
            (Some(temp_path), Cow::Owned(wav_path))
        } else {
            (None, Cow::Borrowed(path))
        };

    // Open the file (use decode_path which may be the temp WAV)
    let file = std::fs::File::open(decode_path.as_ref())
        .map_err(|e| anyhow!("Failed to open audio file '{}': {}", decode_path.display(), e))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Set up format hint based on file extension
    let mut hint = Hint::new();
    if let Some(ext) = decode_path.extension().and_then(|e| e.to_str()) {
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
    let mut sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("Unknown sample rate"))?;

    let mut channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(1);

    debug!(
        "Audio track: {}Hz, {} channels (from metadata)",
        sample_rate, channels
    );

    // Create the decoder
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("Failed to create decoder: {}", e))?;

    // Calculate expected samples for progress tracking (and to size the buffer once)
    let expected_duration = track.codec_params.n_frames
        .map(|frames| frames as f64 / sample_rate as f64);
    let expected_samples = expected_duration
        .map(|dur| (dur * sample_rate as f64 * channels as f64) as usize);

    // Decode all packets. The frame count comes from the file header and can be bogus
    // (streamed WAV writes u32::MAX, MP3 Xing counts are unchecked), so a failed
    // reservation falls back to growing the buffer while decoding.
    let mut all_samples: Vec<f32> = Vec::new();
    if let Some(n) = expected_samples {
        if all_samples.try_reserve_exact(n).is_err() {
            warn!("Could not pre-allocate {} samples; growing the buffer while decoding", n);
        }
    }
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    let mut last_progress = 0u32;

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
                    // Detect actual channel count from decoded audio (metadata may be wrong/missing)
                    let actual_channels = spec.channels.count() as u16;
                    if actual_channels != channels {
                        info!(
                            "Channel count corrected: metadata={} actual={} (using actual)",
                            channels, actual_channels
                        );
                        channels = actual_channels;
                    }
                    // Detect actual sample rate from decoded audio. The container can
                    // declare a different rate than the decoder produces: HE-AAC (SBR)
                    // files declare e.g. 48000 Hz, but Symphonia has no SBR support and
                    // decodes the AAC-LC core at half that rate. Trusting the container
                    // rate makes the audio play at 2x speed and halves the duration.
                    let actual_rate = spec.rate;
                    if actual_rate != sample_rate {
                        info!(
                            "Sample rate corrected: metadata={} actual={} (using actual)",
                            sample_rate, actual_rate
                        );
                        sample_rate = actual_rate;
                    }
                    sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                }

                // Copy samples to buffer
                if let Some(ref mut buf) = sample_buf {
                    buf.copy_interleaved_ref(decoded);
                    all_samples.extend_from_slice(buf.samples());
                }

                // Emit progress updates (every 10%)
                if let (Some(callback), Some(expected)) = (&progress_callback, expected_samples) {
                    let current_progress = ((all_samples.len() as f64 / expected as f64) * 100.0) as u32;
                    if current_progress >= last_progress + 10 && current_progress <= 100 {
                        last_progress = current_progress;
                        callback(current_progress, &format!("Decoding audio: {}%", current_progress));
                    }
                }
            }
            Err(e) => {
                warn!("Error decoding packet: {}", e);
                continue;
            }
        }
    }

    // Ensure we report 100% completion
    if let Some(callback) = &progress_callback {
        callback(100, "Decoding complete");
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

/// The file's demuxer, at the start, with the id and parameters of its first audio track.
fn open_format(path: &Path) -> Result<(Box<dyn FormatReader>, u32, CodecParameters)> {
    let file = std::fs::File::open(path).map_err(|e| anyhow!("Failed to open audio file '{}': {}", path.display(), e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("Failed to probe audio format: {}", e))?;
    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("No audio track found in file"))?;
    let (track_id, params) = (track.id, track.codec_params.clone());
    Ok((format, track_id, params))
}

/// Seconds of container timestamp `ts`, or of `ts` frames at `rate` when the track has no time base.
fn ts_seconds(time_base: Option<TimeBase>, ts: u64, rate: u32) -> f64 {
    match time_base {
        Some(tb) => {
            let t = tb.calc_time(ts);
            t.seconds as f64 + t.frac
        }
        None => ts as f64 / rate.max(1) as f64,
    }
}

/// Length of the first audio track on the container timeline: the sum of its packet durations.
/// Only demuxes, so a long recording is read in well under a second. For a live recording this
/// is the recording clock (plus the first checkpoint's 21 ms of encoder priming): the joined
/// checkpoints advance the container by exactly 30 s each, while decoding yields 1792 more frames
/// per checkpoint.
pub fn container_duration_s(path: &Path) -> Result<f64> {
    if needs_ffmpeg_conversion(path) {
        // Symphonia cannot demux these; their decoded length is the best measure available.
        return Ok(decode_audio_file(path)?.duration_seconds);
    }
    let (mut format, track_id, params) = open_format(path)?;
    let rate = params.sample_rate.unwrap_or(0);
    if params.time_base.is_none() && rate == 0 {
        return Err(anyhow!("Unknown sample rate"));
    }
    let mut duration_ts: u64 = 0;
    loop {
        match format.next_packet() {
            Ok(packet) if packet.track_id() == track_id => duration_ts += packet.dur(),
            Ok(_) => {}
            Err(symphonia::core::errors::Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                warn!("Error reading packet while measuring {}: {}", path.display(), e);
                break;
            }
        }
    }
    Ok(ts_seconds(params.time_base, duration_ts, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_he_aac_uses_decoded_rate_not_container_rate() {
        // HE-AAC (SBR) fixture: the container declares 48 kHz stereo, but
        // Symphonia has no SBR support and decodes only the AAC-LC core at
        // 24 kHz. Trusting the container rate halves the duration and makes
        // the audio play at 2x speed, which truncated long imported
        // recordings to half their length.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/he_aac_48k_5s.m4a");
        let decoded = decode_audio_file(&path).expect("failed to decode HE-AAC fixture");

        assert_eq!(
            decoded.sample_rate, 24000,
            "sample_rate must come from the decoder output, not container metadata"
        );
        assert!(
            (decoded.duration_seconds - 5.16).abs() < 0.5,
            "duration must be ~5s (was reported as ~2.6s before the fix), got {:.2}s",
            decoded.duration_seconds
        );

        // The 16 kHz conversion must preserve the real duration as well.
        let whisper_samples = decoded.to_whisper_format();
        let duration_16k = whisper_samples.len() as f64 / 16000.0;
        assert!(
            (duration_16k - 5.16).abs() < 0.5,
            "whisper-format duration must be ~5s, got {:.2}s",
            duration_16k
        );
    }

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
    fn test_chunked_resample_same_rate() {
        let input = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let result = chunked_resample_with_progress(&input, 16000, 16000, None);
        assert_eq!(result.len(), input.len());
        for (i, &sample) in result.iter().enumerate() {
            assert!((sample - input[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_chunked_resample_empty_input() {
        let input: Vec<f32> = vec![];
        let result = chunked_resample_with_progress(&input, 48000, 16000, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_chunked_resample_downsamples_correctly() {
        // 48kHz to 16kHz = 3x downsampling with a 2-second signal
        let input: Vec<f32> = (0..96000).map(|i| (i as f32 / 96000.0)).collect();
        let result = chunked_resample_with_progress(&input, 48000, 16000, None);

        // Output should be approximately 1/3 the length
        let expected_len = 96000.0 * (16000.0 / 48000.0);
        assert!(
            (result.len() as f64 - expected_len).abs() < 200.0,
            "Expected ~{} samples, got {}",
            expected_len,
            result.len()
        );
    }

    #[test]
    fn test_chunked_resample_preserves_signal_range() {
        // 1 second of sine wave at 44100Hz
        let input: Vec<f32> = (0..44100)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        let result = chunked_resample_with_progress(&input, 44100, 16000, None);

        for sample in &result {
            assert!(
                *sample >= -1.1 && *sample <= 1.1,
                "Sample {} out of expected range",
                sample
            );
        }
    }

    #[test]
    fn test_chunked_resample_matches_single_pass() {
        // Verify chunked output is close to single-pass for small files
        let input: Vec<f32> = (0..48000)
            .map(|i| (2.0 * std::f32::consts::PI * 300.0 * i as f32 / 48000.0).sin() * 0.5)
            .collect();

        let single_pass = resample_audio(&input, 48000, 16000);
        let chunked = chunked_resample_with_progress(&input, 48000, 16000, None);

        // Lengths should be very close
        let len_diff = (single_pass.len() as i64 - chunked.len() as i64).unsigned_abs();
        assert!(
            len_diff < 50,
            "Length mismatch: single_pass={}, chunked={}",
            single_pass.len(),
            chunked.len()
        );

        // Compare overlapping samples (allow some tolerance at chunk boundaries)
        let compare_len = single_pass.len().min(chunked.len());
        let mut max_diff = 0.0f32;
        for i in 0..compare_len {
            let diff = (single_pass[i] - chunked[i]).abs();
            max_diff = max_diff.max(diff);
        }
        // Chunk boundaries may introduce small discontinuities
        assert!(
            max_diff < 0.15,
            "Max sample difference too large: {}",
            max_diff
        );
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
        // Test that large files use chunked sinc resampling path
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
        assert!((result[0] - 0.5).abs() < 0.001); // preserved
        assert_eq!(result[1], 0.0); // infinity → 0
        assert!((result[2] - (-0.3)).abs() < 0.001); // preserved
    }

    #[test]
    fn test_needs_ffmpeg_conversion() {
        assert!(needs_ffmpeg_conversion(Path::new("video.mkv")));
        assert!(needs_ffmpeg_conversion(Path::new("audio.webm")));
        assert!(needs_ffmpeg_conversion(Path::new("audio.wma")));
        // Case insensitive
        assert!(needs_ffmpeg_conversion(Path::new("meeting.MKV")));
        assert!(needs_ffmpeg_conversion(Path::new("audio.WMA")));
        assert!(needs_ffmpeg_conversion(Path::new("audio.WebM")));
        // Symphonia-native formats should NOT need ffmpeg
        assert!(!needs_ffmpeg_conversion(Path::new("audio.mp4")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.wav")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.mp3")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.flac")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.ogg")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.aac")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.m4a")));
        // No extension
        assert!(!needs_ffmpeg_conversion(Path::new("noext")));
    }

    #[test]
    fn test_into_whisper_format_matches_to_whisper_format() {
        let mono = DecodedAudio {
            samples: (0..48_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect(),
            sample_rate: 48_000,
            channels: 1,
            duration_seconds: 1.0,
        };
        assert_eq!(mono.to_whisper_format(), mono.clone().into_whisper_format());
        let stereo = DecodedAudio {
            samples: (0..96_000).map(|i| if i % 2 == 0 { 0.25 } else { -0.5 }).collect(),
            sample_rate: 48_000,
            channels: 2,
            duration_seconds: 1.0,
        };
        assert_eq!(stereo.to_whisper_format(), stereo.clone().into_whisper_format());
    }

    #[test]
    fn container_duration_is_the_recording_clock() {
        use super::test_audio::{joined_checkpoints, silence_then_tone, write_wav};
        let dir = tempfile::tempdir().unwrap();

        let wav = dir.path().join("speech.wav");
        write_wav(&wav, 16_000, 1, &silence_then_tone(16_000, 0.5, 1.5));
        assert_eq!(container_duration_s(&wav).unwrap(), 1.5);

        // Three checkpoints joined as a live recording: the container advances exactly 30 s per
        // checkpoint; only the first checkpoint's encoder priming (1024 frames) is added.
        let live = joined_checkpoints(dir.path(), 3, |_| false);
        let duration = container_duration_s(&live).unwrap();
        assert!((duration - (90.0 + 1024.0 / 48_000.0)).abs() < 1e-6, "container duration {duration}");
        // Counting decoded frames instead gains priming and padding at every checkpoint. That
        // drift is the diarization time map's concern, not the player's.
        let decoded = decode_audio_file(&live).unwrap();
        let decoded_s = decoded.samples.len() as f64 / decoded.channels.max(1) as f64 / decoded.sample_rate as f64;
        assert!(decoded_s - duration > 0.07, "decoded {decoded_s} s vs container {duration} s");
    }
}

/// Audio files for tests.
#[cfg(test)]
pub(crate) mod test_audio {
    use std::path::{Path, PathBuf};

    /// 16-bit PCM WAV of interleaved `samples`.
    pub(crate) fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[f32]) {
        std::fs::write(path, crate::audio::encode::pcm16_wav(rate, channels, samples)).unwrap();
    }

    /// Mono: silence for `silent_s`, then a 440 Hz tone at half scale until `total_s`.
    pub(crate) fn silence_then_tone(rate: u32, silent_s: f64, total_s: f64) -> Vec<f32> {
        let onset = (silent_s * rate as f64) as usize;
        (0..(total_s * rate as f64) as usize)
            .map(|i| if i < onset { 0.0 } else { 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() })
            .collect()
    }

    /// AAC in MP4 encoded from `wav` with ffmpeg.
    pub(crate) fn aac_mp4_from_wav(wav: &Path) -> PathBuf {
        let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().expect("ffmpeg is needed for this test");
        let out = wav.with_extension("mp4");
        let status = std::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(wav)
            .args(["-c:a", "aac", "-b:a", "128k"])
            .arg(&out)
            .status()
            .expect("run ffmpeg");
        assert!(status.success(), "ffmpeg could not encode {}", wav.display());
        out
    }

    /// `dir/audio.mp4` built as a live recording is: 30 s checkpoints of 48 kHz mono encoded by
    /// `encode_single_audio`, joined with the ffmpeg concat demuxer and `-c copy`. The signal is
    /// a 440 Hz tone at half scale wherever `tone_at(recording clock seconds)` holds, else silence.
    pub(crate) fn joined_checkpoints(dir: &Path, checkpoints: usize, tone_at: impl Fn(f64) -> bool) -> PathBuf {
        const RATE: usize = 48_000;
        const CHECKPOINT: usize = 30 * RATE;
        let mut list = String::new();
        for k in 0..checkpoints {
            let samples: Vec<f32> = (k * CHECKPOINT..(k + 1) * CHECKPOINT)
                .map(|n| {
                    let t = n as f64 / RATE as f64;
                    if tone_at(t) { 0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() as f32 } else { 0.0 }
                })
                .collect();
            let chunk = dir.join(format!("audio_chunk_{:03}.mp4", k));
            crate::audio::encode::encode_single_audio(bytemuck::cast_slice(&samples), RATE as u32, 1, &chunk)
                .expect("encode a checkpoint");
            list.push_str(&format!("file '{}'\n", chunk.display()));
        }
        let list_file = dir.join("concat_list.txt");
        std::fs::write(&list_file, list).unwrap();
        let out = dir.join("audio.mp4");
        let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().expect("ffmpeg is needed for this test");
        let status = std::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i"])
            .arg(&list_file)
            .args(["-c", "copy", "-y"])
            .arg(&out)
            .status()
            .expect("run ffmpeg");
        assert!(status.success(), "ffmpeg could not join the checkpoints");
        out
    }

    /// Seconds into mono `samples` of the first 2 ms window whose RMS exceeds 0.1.
    pub(crate) fn onset_s(samples: &[f32], rate: u32) -> Option<f64> {
        let window = (rate as usize / 500).max(1);
        (0..samples.len().saturating_sub(window))
            .find(|&i| (samples[i..i + window].iter().map(|x| x * x).sum::<f32>() / window as f32).sqrt() > 0.1)
            .map(|i| i as f64 / rate as f64)
    }
}
