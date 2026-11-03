use ndarray::{Array1, Array2, Array3};
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const NEMOTRON_NEMO128_URL: &str =
    "https://meetily.towardsgeneralintelligence.com/models/parakeet-tdt-0.6b-v3-onnx/nemo128.onnx";
const FIFO_MAX_LEN: usize = 120; // ~9.6 seconds of recent continuous frames
const SPKCACHE_MAX_LEN: usize = 264; // ~21.1 seconds of historical speaker cache (NVIDIA standard)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedWord {
    pub word: String,
    pub start_time: f64,
    pub end_time: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerTurn {
    pub speaker: String,
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerSegment {
    pub speaker_id: usize,
    pub speaker_label: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationResult {
    pub segments: Vec<SpeakerSegment>,
    pub num_speakers: usize,
    pub primary_speaker: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum DiarizationError {
    #[error("ORT runtime error: {0}")]
    Ort(#[from] ort::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Tensor shape error: {0}")]
    Shape(#[from] ndarray::ShapeError),
    #[error("Model input not found: {0}")]
    InputNotFound(String),
    #[error("Model output not found: {0}")]
    OutputNotFound(String),
    #[error("Runtime unavailable: {0}")]
    RuntimeUnavailable(String),
    #[error("Audio processing error: {0}")]
    ProcessingError(String),
    #[error("Preprocessor missing: {0}")]
    PreprocessorMissing(String),
}

pub struct NemotronDiarizationModel {
    session: Session,
    preprocessor: Session,
    max_speakers: usize,
    threshold: f32,
    spkcache: Array3<f32>,
    fifo: Array3<f32>,
}

impl Drop for NemotronDiarizationModel {
    fn drop(&mut self) {
        log::info!("Dropping NemotronDiarizationModel");
    }
}

/// Helper to locate or download `nemo128.onnx` preprocessor
fn resolve_or_fetch_nemo128(models_dir: &Path) -> Result<PathBuf, DiarizationError> {
    let direct_path = models_dir.join("nemo128.onnx");
    if direct_path.exists() {
        return Ok(direct_path);
    }

    // Check parent models directory (e.g. Parakeet models directory)
    if let Some(parent) = models_dir.parent() {
        let candidate_paths = [
            parent.join("parakeet").join("parakeet-tdt-0.6b-v3-int8").join("nemo128.onnx"),
            parent.join("parakeet").join("parakeet-tdt-0.6b-v2-int8").join("nemo128.onnx"),
            parent.join("parakeet").join("nemo128.onnx"),
            parent.join("nemo128.onnx"),
        ];

        for cand in candidate_paths {
            if cand.exists() {
                log::info!("Found existing nemo128.onnx at {:?}, copying to diarization models dir", cand);
                let _ = std::fs::copy(&cand, &direct_path);
                return Ok(direct_path);
            }
        }
    }

    // Download nemo128.onnx (~140KB) if not present anywhere
    log::info!("nemo128.onnx not found locally, downloading from {}", NEMOTRON_NEMO128_URL);
    let resp = reqwest::blocking::get(NEMOTRON_NEMO128_URL)
        .map_err(|e| DiarizationError::PreprocessorMissing(format!("Failed to fetch nemo128: {}", e)))?;
    if !resp.status().is_success() {
        return Err(DiarizationError::PreprocessorMissing(format!(
            "Failed to download nemo128: HTTP {}",
            resp.status()
        )));
    }

    let bytes = resp
        .bytes()
        .map_err(|e| DiarizationError::PreprocessorMissing(format!("Failed to read nemo128 bytes: {}", e)))?;
    std::fs::write(&direct_path, &bytes)?;
    log::info!("✅ Successfully downloaded nemo128.onnx to {:?}", direct_path);
    Ok(direct_path)
}

impl NemotronDiarizationModel {
    pub fn new<P: AsRef<Path>>(
        model_path: P,
        max_speakers: usize,
        threshold: f32,
    ) -> Result<Self, DiarizationError> {
        crate::ensure_onnx_runtime_available()
            .map_err(|e| DiarizationError::RuntimeUnavailable(e.to_string()))?;

        let model_path = model_path.as_ref();
        let (model_file, models_dir) = if model_path.is_file() {
            (
                model_path.to_path_buf(),
                model_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
            )
        } else {
            (
                model_path.join("nemotron3_diar_v3.onnx"),
                model_path.to_path_buf(),
            )
        };

        let nemo128_path = resolve_or_fetch_nemo128(&models_dir)?;

        log::info!("Loading Nemotron-3 Diarization model from {:?}...", model_file);
        let providers = vec![CPUExecutionProvider::default().build()];

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_execution_providers(providers.clone())?
            .with_parallel_execution(true)?
            .commit_from_file(&model_file)?;

        log::info!("Loading Nemo 128 Mel preprocessor from {:?}...", nemo128_path);
        let preprocessor = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_execution_providers(providers)?
            .commit_from_file(&nemo128_path)?;

        log::info!("✅ Nemotron-3 Diarization model and Mel preprocessor loaded successfully");

        Ok(Self {
            session,
            preprocessor,
            max_speakers: max_speakers.clamp(1, 8),
            threshold: threshold.clamp(0.1, 0.9),
            spkcache: Array3::<f32>::zeros((1, 0, 512)),
            fifo: Array3::<f32>::zeros((1, 0, 512)),
        })
    }

    /// Reset persistent streaming cache between distinct recording sessions
    pub fn reset_streaming_state(&mut self) {
        self.spkcache = Array3::<f32>::zeros((1, 0, 512));
        self.fifo = Array3::<f32>::zeros((1, 0, 512));
        log::info!("🔄 Reset Nemotron-3 streaming diarization state");
    }

    /// Run speaker diarization on 16kHz mono audio samples.
    pub fn diarize(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
    ) -> Result<DiarizationResult, DiarizationError> {
        if samples.is_empty() {
            return Ok(DiarizationResult {
                segments: Vec::new(),
                num_speakers: 0,
                primary_speaker: None,
            });
        }

        // Resample to 16kHz if necessary
        let audio_16k = if sample_rate != 16000 {
            crate::audio::audio_processing::resample_audio(samples, sample_rate, 16000)
        } else {
            samples.to_vec()
        };

        let duration_secs = audio_16k.len() as f64 / 16000.0;
        if duration_secs < 0.25 {
            // Audio too short for meaningful multi-speaker clustering
            return Ok(DiarizationResult {
                segments: vec![SpeakerSegment {
                    speaker_id: 1,
                    speaker_label: "Speaker 1".to_string(),
                    start_seconds: 0.0,
                    end_seconds: duration_secs,
                    confidence: 0.9,
                }],
                num_speakers: 1,
                primary_speaker: Some("Speaker 1".to_string()),
            });
        }

        let (speaker_probs, embs_opt) = self.run_inference(&audio_16k)?;

        // Update FIFO queue and cascade overflow into speaker cache
        if let Some(embs) = embs_opt {
            let new_len = embs.shape()[1];
            if new_len > 0 {
                let old_fifo_len = self.fifo.shape()[1];
                let total_fifo_len = old_fifo_len + new_len;
                let mut combined_fifo = Array3::<f32>::zeros((1, total_fifo_len, 512));

                for t in 0..old_fifo_len {
                    for d in 0..512 {
                        combined_fifo[[0, t, d]] = self.fifo[[0, t, d]];
                    }
                }
                for t in 0..new_len {
                    for d in 0..512 {
                        combined_fifo[[0, old_fifo_len + t, d]] = embs[[0, t, d]];
                    }
                }

                if total_fifo_len > FIFO_MAX_LEN {
                    let overflow = total_fifo_len - FIFO_MAX_LEN;
                    // Oldest `overflow` frames from FIFO transfer into spkcache
                    let old_cache_len = self.spkcache.shape()[1];
                    let total_cache_len = old_cache_len + overflow;
                    let mut combined_cache = Array3::<f32>::zeros((1, total_cache_len, 512));

                    for t in 0..old_cache_len {
                        for d in 0..512 {
                            combined_cache[[0, t, d]] = self.spkcache[[0, t, d]];
                        }
                    }
                    for t in 0..overflow {
                        for d in 0..512 {
                            combined_cache[[0, old_cache_len + t, d]] = combined_fifo[[0, t, d]];
                        }
                    }

                    if total_cache_len > SPKCACHE_MAX_LEN {
                        let start_idx = total_cache_len - SPKCACHE_MAX_LEN;
                        let mut trimmed_cache = Array3::<f32>::zeros((1, SPKCACHE_MAX_LEN, 512));
                        for t in 0..SPKCACHE_MAX_LEN {
                            for d in 0..512 {
                                trimmed_cache[[0, t, d]] = combined_cache[[0, start_idx + t, d]];
                            }
                        }
                        self.spkcache = trimmed_cache;
                    } else {
                        self.spkcache = combined_cache;
                    }

                    // Trim FIFO to retain most recent FIFO_MAX_LEN frames
                    let mut trimmed_fifo = Array3::<f32>::zeros((1, FIFO_MAX_LEN, 512));
                    for t in 0..FIFO_MAX_LEN {
                        for d in 0..512 {
                            trimmed_fifo[[0, t, d]] = combined_fifo[[0, overflow + t, d]];
                        }
                    }
                    self.fifo = trimmed_fifo;
                } else {
                    self.fifo = combined_fifo;
                }
            }
        }

        let segments = self.post_process_predictions(&speaker_probs, duration_secs)?;

        // Find unique speakers and primary speaker
        let mut speaker_counts: std::collections::HashMap<usize, f64> =
            std::collections::HashMap::new();
        for seg in &segments {
            let dur = seg.end_seconds - seg.start_seconds;
            *speaker_counts.entry(seg.speaker_id).or_insert(0.0) += dur;
        }

        let num_speakers = speaker_counts.len();
        let primary_speaker = speaker_counts
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(spk_id, _)| format!("Speaker {}", spk_id));

        let final_primary = primary_speaker.or_else(|| Some("Speaker 1".to_string()));

        log::info!(
            "🎤 Nemotron-3 Diarization: primary={:?}, total_detected_speakers={}, duration={:.2}s",
            final_primary,
            num_speakers,
            duration_secs
        );

        Ok(DiarizationResult {
            segments,
            num_speakers: num_speakers.max(1),
            primary_speaker: final_primary,
        })
    }

    fn run_inference(
        &mut self,
        audio_16k: &[f32],
    ) -> Result<(Array2<f32>, Option<Array3<f32>>), DiarizationError> {
        let num_samples = audio_16k.len();
        let wave_arr = Array2::from_shape_vec((1, num_samples), audio_16k.to_vec())?;
        let wave_len = Array1::from_vec(vec![num_samples as i64]);

        // 1. Run Nemo 128 Mel Spectrogram Preprocessor
        let prep_res = self.preprocessor.run(inputs![
            "waveforms" => TensorRef::from_array_view(wave_arr.view())?,
            "waveforms_lens" => TensorRef::from_array_view(wave_len.view())?,
        ])?;

        let features_out = prep_res
            .get("features")
            .ok_or_else(|| DiarizationError::OutputNotFound("features".to_string()))?;
        let features = features_out.try_extract_array::<f32>()?;

        // features shape is [1, 128, T_raw]
        let t_raw = features.shape()[2];
        // Sortformer requires T to be a multiple of 8 (subsampling factor 8)
        let t_dim = ((t_raw / 8) * 8).max(8);

        let mut chunk_features = Array3::<f32>::zeros((1, t_dim, 128));
        for t in 0..t_dim.min(t_raw) {
            for m in 0..128 {
                chunk_features[[0, t, m]] = features[[0, m, t]];
            }
        }

        let chunk_lengths = Array1::<i64>::from_vec(vec![t_dim as i64]);
        let spkcache_len = self.spkcache.shape()[1];
        let spkcache_lengths = Array1::<i64>::from_vec(vec![spkcache_len as i64]);
        let fifo_len = self.fifo.shape()[1];
        let fifo_lengths = Array1::<i64>::from_vec(vec![fifo_len as i64]);

        // 2. Run Nemotron-3 Diarization Model
        let diar_res = self.session.run(inputs![
            "chunk" => TensorRef::from_array_view(chunk_features.view())?,
            "chunk_lengths" => TensorRef::from_array_view(chunk_lengths.view())?,
            "spkcache" => TensorRef::from_array_view(self.spkcache.view())?,
            "spkcache_lengths" => TensorRef::from_array_view(spkcache_lengths.view())?,
            "fifo" => TensorRef::from_array_view(self.fifo.view())?,
            "fifo_lengths" => TensorRef::from_array_view(fifo_lengths.view())?,
        ])?;

        // 3. Extract high-resolution predictions
        let hires_out = diar_res
            .get("preds_hires")
            .or_else(|| diar_res.get("preds_diar"))
            .ok_or_else(|| DiarizationError::OutputNotFound("preds_hires".to_string()))?;
        let hires = hires_out.try_extract_array::<f32>()?;
        let total_frames = hires.shape()[1];

        // Slice current chunk frames (Sortformer outputs predictions over history + chunk)
        let chunk_start_frame = if total_frames >= t_dim {
            total_frames - t_dim
        } else {
            0
        };

        let spk_dim = self.max_speakers.min(8);
        let mut probs = Array2::<f32>::zeros((t_dim, spk_dim));
        for t in 0..t_dim {
            let src_t = (chunk_start_frame + t).min(total_frames - 1);
            for s in 0..spk_dim {
                probs[[t, s]] = hires[[0, src_t, s]];
            }
        }

        let embs_opt: Option<Array3<f32>> = diar_res
            .get("chunk_pre_encode_embs")
            .and_then(|val| val.try_extract_array::<f32>().ok())
            .and_then(|arr| arr.to_owned().into_dimensionality::<ndarray::Ix3>().ok());

        Ok((probs, embs_opt))
    }

    /// Convert frame probabilities [T, Speakers] into contiguous speaker segments.
    fn post_process_predictions(
        &self,
        probs: &Array2<f32>,
        total_duration: f64,
    ) -> Result<Vec<SpeakerSegment>, DiarizationError> {
        let num_frames = probs.nrows();
        let num_spks = probs.ncols();

        if num_frames == 0 {
            return Ok(Vec::new());
        }

        let frame_duration = total_duration / num_frames as f64;
        let mut segments: Vec<SpeakerSegment> = Vec::new();

        // Assign dominant speaker for each frame if above threshold
        let mut frame_speakers: Vec<Option<usize>> = Vec::with_capacity(num_frames);
        let mut max_overall_prob = 0.0f32;
        let mut best_overall_spk = 1;

        for t in 0..num_frames {
            let mut best_spk = None;
            let mut best_prob = self.threshold;

            for s in 0..num_spks {
                let p = probs[[t, s]];
                let prob = if p < 0.0 || p > 1.0 {
                    1.0 / (1.0 + (-p).exp())
                } else {
                    p
                };

                if prob > max_overall_prob {
                    max_overall_prob = prob;
                    best_overall_spk = s + 1;
                }

                if prob > best_prob {
                    best_prob = prob;
                    best_spk = Some(s + 1); // 1-indexed speaker ID
                }
            }
            frame_speakers.push(best_spk);
        }

        // Smooth out short gaps (median filtering / smoothing)
        let smoothed_speakers = smooth_frame_sequence(&frame_speakers, 3);

        // Group consecutive frames into segments
        let mut current_speaker: Option<usize> = None;
        let mut start_frame = 0;

        for (frame_idx, &spk) in smoothed_speakers.iter().enumerate() {
            if spk != current_speaker {
                if let Some(prev_spk) = current_speaker {
                    let start_sec = start_frame as f64 * frame_duration;
                    let end_sec = frame_idx as f64 * frame_duration;
                    if end_sec - start_sec >= 0.15 {
                        segments.push(SpeakerSegment {
                            speaker_id: prev_spk,
                            speaker_label: format!("Speaker {}", prev_spk),
                            start_seconds: start_sec,
                            end_seconds: end_sec,
                            confidence: 0.92,
                        });
                    }
                }
                current_speaker = spk;
                start_frame = frame_idx;
            }
        }

        // Add trailing segment
        if let Some(prev_spk) = current_speaker {
            let start_sec = start_frame as f64 * frame_duration;
            let end_sec = total_duration;
            if end_sec - start_sec >= 0.15 {
                segments.push(SpeakerSegment {
                    speaker_id: prev_spk,
                    speaker_label: format!("Speaker {}", prev_spk),
                    start_seconds: start_sec,
                    end_seconds: end_sec,
                    confidence: 0.92,
                });
            }
        }

        // If no segment was long enough, use the overall dominant speaker
        if segments.is_empty() {
            segments.push(SpeakerSegment {
                speaker_id: best_overall_spk,
                speaker_label: format!("Speaker {}", best_overall_spk),
                start_seconds: 0.0,
                end_seconds: total_duration,
                confidence: 0.85,
            });
        }

        Ok(segments)
    }

    pub fn set_max_speakers(&mut self, max_speakers: usize) {
        self.max_speakers = max_speakers.clamp(1, 8);
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.clamp(0.1, 0.9);
    }
}

/// Align timestamped words to diarization speaker segments and segment into speaker turns.
/// Follows the NVIDIA midpoint speaker assignment algorithm:
/// for each word, find active speaker at word midpoint, and cut turn whenever speaker changes.
pub fn align_words_to_speaker_turns(
    words: &[TimedWord],
    segments: &[SpeakerSegment],
) -> Vec<SpeakerTurn> {
    if words.is_empty() {
        return Vec::new();
    }

    if segments.is_empty() {
        let full_text = words.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");
        let start = words.first().map(|w| w.start_time).unwrap_or(0.0);
        let end = words.last().map(|w| w.end_time).unwrap_or(start);
        return vec![SpeakerTurn {
            speaker: "Speaker 1".to_string(),
            text: full_text,
            start_seconds: start,
            end_seconds: end,
        }];
    }

    // Helper to find speaker at midpoint
    let speaker_at = |midpoint: f64, prev: Option<&str>| -> String {
        let mut active = Vec::new();
        for seg in segments {
            if seg.start_seconds <= midpoint && midpoint <= seg.end_seconds {
                active.push(seg);
            }
        }
        if active.len() == 1 {
            active[0].speaker_label.clone()
        } else if active.len() > 1 {
            // Overlap: prefer the one where midpoint is closest to segment center
            let mut sorted = active;
            sorted.sort_by(|a, b| {
                let dist_a = ((a.start_seconds + a.end_seconds) / 2.0 - midpoint).abs();
                let dist_b = ((b.start_seconds + b.end_seconds) / 2.0 - midpoint).abs();
                dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
            });
            sorted[0].speaker_label.clone()
        } else if let Some(p) = prev {
            p.to_string()
        } else if let Some(first) = segments.first() {
            first.speaker_label.clone()
        } else {
            "Speaker 1".to_string()
        }
    };

    // Label each word
    let mut word_speakers: Vec<String> = Vec::with_capacity(words.len());
    let mut current_speaker: Option<String> = None;
    for word in words {
        let midpoint = (word.start_time + word.end_time) / 2.0;
        let spk = speaker_at(midpoint, current_speaker.as_deref());
        current_speaker = Some(spk.clone());
        word_speakers.push(spk);
    }

    // Smooth single-word blips (e.g. Speaker 1, Speaker 2 for 1 word <0.4s, Speaker 1)
    if word_speakers.len() >= 3 {
        for i in 1..(word_speakers.len() - 1) {
            if word_speakers[i - 1] == word_speakers[i + 1] && word_speakers[i] != word_speakers[i - 1] {
                let word_dur = words[i].end_time - words[i].start_time;
                if word_dur < 0.40 {
                    word_speakers[i] = word_speakers[i - 1].clone();
                }
            }
        }
    }

    // Group consecutive words with identical speaker into distinct turns
    let mut turns: Vec<SpeakerTurn> = Vec::new();
    let mut turn_words: Vec<String> = Vec::new();
    let mut turn_speaker = String::new();
    let mut turn_start = 0.0;
    let mut turn_end = 0.0;

    for (i, word) in words.iter().enumerate() {
        let spk = &word_speakers[i];
        if turn_words.is_empty() {
            turn_speaker = spk.clone();
            turn_start = word.start_time;
            turn_end = word.end_time;
            turn_words.push(word.word.clone());
        } else if spk == &turn_speaker {
            turn_words.push(word.word.clone());
            turn_end = word.end_time;
        } else {
            // Cut speaker turn!
            let text = turn_words.join(" ").trim().to_string();
            if !text.is_empty() {
                turns.push(SpeakerTurn {
                    speaker: turn_speaker,
                    text,
                    start_seconds: turn_start,
                    end_seconds: turn_end,
                });
            }
            turn_speaker = spk.clone();
            turn_start = word.start_time;
            turn_end = word.end_time;
            turn_words = vec![word.word.clone()];
        }
    }

    if !turn_words.is_empty() {
        let text = turn_words.join(" ").trim().to_string();
        if !text.is_empty() {
            turns.push(SpeakerTurn {
                speaker: turn_speaker,
                text,
                start_seconds: turn_start,
                end_seconds: turn_end,
            });
        }
    }

    turns
}

/// Simple majority smoothing over a window to remove erratic single-frame flips.
fn smooth_frame_sequence(sequence: &[Option<usize>], window_size: usize) -> Vec<Option<usize>> {
    let len = sequence.len();
    let mut smoothed = sequence.to_vec();

    if len <= window_size {
        return smoothed;
    }

    let half = window_size / 2;
    for i in half..(len - half) {
        let window = &sequence[i - half..=i + half];
        let mut counts = std::collections::HashMap::new();
        for &item in window {
            if let Some(val) = item {
                *counts.entry(val).or_insert(0) += 1;
            }
        }

        if let Some((&most_common, &count)) = counts.iter().max_by_key(|entry| entry.1) {
            if count > half {
                smoothed[i] = Some(most_common);
            }
        }
    }

    smoothed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_and_run_nemotron() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let ort_dll = std::path::PathBuf::from(manifest_dir)
            .join("binaries")
            .join("onnxruntime")
            .join("onnxruntime.dll");
        if ort_dll.exists() {
            let _ = ort::init_from(ort_dll.to_string_lossy().into_owned())
                .with_telemetry(false)
                .commit();
        }

        let model_path = std::path::PathBuf::from(
            r"C:\Users\nguye\AppData\Roaming\com.meetily.ai\models\diarization\nemotron3_diar_v3.onnx",
        );
        if !model_path.exists() {
            println!("Model file not found at {:?}", model_path);
            return;
        }

        println!("Initializing NemotronDiarizationModel...");
        let mut model = NemotronDiarizationModel::new(&model_path, 4, 0.5)
            .expect("Failed to initialize model");

        println!("Model initialized successfully. Running diarize on dummy audio...");
        let dummy_audio = vec![0.02f32; 16000 * 2]; // 2 seconds
        let result = model.diarize(&dummy_audio, 16000).expect("Failed to diarize");
        println!("Diarization result: {:?}", result);
        assert!(result.primary_speaker.is_some());
    }
}
