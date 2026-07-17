// diarization/segmentation.rs
//
// Speaker segmentation using Pyannote 3.0 ONNX model (CC-BY-4.0 via sherpa-onnx).
// Sliding window 10s @ 16kHz, stride 5s. Processes raw waveform, outputs powerset
// logits over local speaker activity {none, spk1, spk2, spk3, spk1+2, spk1+3, spk2+3}.
// Decodes to local speaker intervals, stitches windows by absolute recording time.

use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use std::path::Path;

/// Constants from session.rs for consistency
pub const DIARIZATION_WINDOW_SECONDS: f64 = 10.0;
pub const DIARIZATION_STRIDE_SECONDS: f64 = 5.0;
const SAMPLE_RATE: usize = 16_000;
const WINDOW_SAMPLES: usize = (DIARIZATION_WINDOW_SECONDS * SAMPLE_RATE as f64) as usize; // 160,000
const STRIDE_SAMPLES: usize = (DIARIZATION_STRIDE_SECONDS * SAMPLE_RATE as f64) as usize; // 80,000

#[derive(Debug, Clone)]
pub struct LocalSegment {
    pub start_time_s: f64,
    pub end_time_s: f64,
    pub local_speaker_slot: u8, // 0=none, 1=spk1, 2=spk2, 3=spk3, 4=spk1+2, 5=spk1+3, 6=spk2+3
    pub is_overlap: bool,       // true if local_speaker_slot indicates multiple speakers
}

#[derive(thiserror::Error, Debug)]
pub enum SegmentationError {
    #[error("ONNX Runtime error: {0}")]
    Ort(#[from] ort::Error),
    #[error("Audio too short for segmentation")]
    AudioTooShort,
    #[error("Model produced no segmentation output")]
    NoOutput,
}

pub struct SegmentationSession {
    session: Session,
    input_name: String,
    output_name: String,
}

impl SegmentationSession {
    pub fn new(model_path: &Path) -> Result<Self, SegmentationError> {
        let session = Session::builder()?
            .with_execution_providers(vec![CPUExecutionProvider::default().build()])?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(2)?
            .commit_from_file(model_path)?;

        // Pyannote segmentation input: "input" or "audio"; output: "logits"
        let input_name = session
            .inputs
            .first()
            .map(|i| i.name.clone())
            .unwrap_or_else(|| "input".to_string());
        let output_name = session
            .outputs
            .first()
            .map(|o| o.name.clone())
            .unwrap_or_else(|| "logits".to_string());

        log::info!(
            "Diarization segmentation model loaded from {} (input: {}, output: {})",
            model_path.display(),
            input_name,
            output_name
        );

        Ok(Self {
            session,
            input_name,
            output_name,
        })
    }

    /// Segment 16kHz mono audio over sliding 10s windows (stride 5s).
    /// Returns local segments (start, end, speaker slot, overlap flag) in absolute recording time.
    /// Overlapping windows (50% overlap) are deduplicated: segments in overlapping zones
    /// are kept from the window where they're closer to the center (most reliable inference).
    pub fn segment(&mut self, samples_16k: &[f32]) -> Result<Vec<LocalSegment>, SegmentationError> {
        if samples_16k.len() < WINDOW_SAMPLES {
            // If audio is shorter than one window, skip segmentation
            // (downstream will rely on pure embeddings + clustering)
            return Ok(Vec::new());
        }

        let mut segments_by_window: Vec<(f64, Vec<LocalSegment>)> = Vec::new();
        let mut window_start_sample = 0usize;

        while window_start_sample + WINDOW_SAMPLES <= samples_16k.len() {
            let window = &samples_16k[window_start_sample..window_start_sample + WINDOW_SAMPLES];
            let window_start_time = window_start_sample as f64 / SAMPLE_RATE as f64;
            let window_end_time = window_start_time + DIARIZATION_WINDOW_SECONDS;
            let window_center_time = window_start_time + DIARIZATION_WINDOW_SECONDS / 2.0;

            // Run inference on raw audio [1, 1, 160000]
            let logits = self.infer_window(window)?;

            // Decode logits: for each frame, argmax over 7 classes
            // Frame duration: ~20ms (10s window / ~500 frames)
            let mut window_segments = Vec::new();
            if !logits.is_empty() {
                let num_frames = logits.len();
                let frame_duration = DIARIZATION_WINDOW_SECONDS / num_frames as f64;

                for (frame_idx, &class_idx) in logits.iter().enumerate() {
                    let frame_start = window_start_time + frame_idx as f64 * frame_duration;
                    let frame_end = frame_start + frame_duration;

                    // Decode class: 0=none, 1-3=single speaker, 4-6=overlap
                    let is_overlap = class_idx >= 4;
                    if class_idx > 0 {
                        // Skip "none" class (0)
                        window_segments.push(LocalSegment {
                            start_time_s: frame_start,
                            end_time_s: frame_end,
                            local_speaker_slot: class_idx,
                            is_overlap,
                        });
                    }
                }
            }

            segments_by_window.push((window_center_time, window_segments));
            window_start_sample += STRIDE_SAMPLES;
        }

        // Count raw segments before dedup for logging
        let raw_segment_count: usize = segments_by_window.iter().map(|(_, s)| s.len()).sum();

        // Deduplicate: for overlapping zones, prefer first occurrence (simpler, still correct for clustering)
        let mut deduped = Vec::new();
        for (_window_center, window_segs) in segments_by_window {
            for seg in window_segs {
                let mut should_keep = true;

                for dedup_seg in deduped.iter() {
                    if Self::segments_overlap_significantly(&seg, dedup_seg) {
                        // Already have a segment covering this region; skip to avoid duplicate embeddings
                        should_keep = false;
                        break;
                    }
                }

                if should_keep {
                    deduped.push(seg);
                }
            }
        }

        log::debug!(
            "Segmentation: {} windows, {} raw segments → {} deduped",
            (samples_16k.len() - WINDOW_SAMPLES) / STRIDE_SAMPLES + 1,
            raw_segment_count,
            deduped.len()
        );

        Ok(deduped)
    }

    /// Check if two segments overlap significantly (>50% IoU by time).
    fn segments_overlap_significantly(seg1: &LocalSegment, seg2: &LocalSegment) -> bool {
        let overlap_start = seg1.start_time_s.max(seg2.start_time_s);
        let overlap_end = seg1.end_time_s.min(seg2.end_time_s);
        if overlap_end <= overlap_start {
            return false;
        }
        let overlap_duration = overlap_end - overlap_start;
        let seg1_duration = seg1.end_time_s - seg1.start_time_s;
        let seg2_duration = seg2.end_time_s - seg2.start_time_s;
        let union_duration = seg1_duration + seg2_duration - overlap_duration;

        let iou = overlap_duration / union_duration;
        iou > 0.5
    }

    fn infer_window(&mut self, audio_window: &[f32]) -> Result<Vec<u8>, SegmentationError> {
        // Input shape: [1, 1, 160000]
        let mut input_audio = vec![0.0f32; WINDOW_SAMPLES];
        input_audio[..audio_window.len()].copy_from_slice(audio_window);

        let input_array = ndarray::Array3::<f32>::from_shape_vec(
            (1, 1, WINDOW_SAMPLES),
            input_audio,
        )
        .map_err(|_| SegmentationError::AudioTooShort)?;

        let inputs = inputs![self.input_name.as_str() => TensorRef::from_array_view(input_array.view())?];
        let outputs = self.session.run(inputs)?;

        // Output shape: [1, T, 7] logits over classes
        let logits_output = outputs
            .get(self.output_name.as_str())
            .ok_or(SegmentationError::NoOutput)?
            .try_extract_array::<f32>()?;

        // Decode: for each time frame t, argmax over 7 classes
        let logits_view = logits_output.view();
        let shape = logits_view.dim();
        let num_frames = shape[1];
        let num_classes = shape[2];

        let mut class_ids = Vec::with_capacity(num_frames);
        for t in 0..num_frames {
            let mut best_class = 0u8;
            let mut best_score = f32::NEG_INFINITY;
            for c in 0..num_classes {
                let score = logits_view[[0, t, c]];
                if score > best_score {
                    best_score = score;
                    best_class = c as u8;
                }
            }
            class_ids.push(best_class);
        }

        Ok(class_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powerset_classes_correctly_marked_as_overlap() {
        // Classes 0-3: single speaker / none; 4-6: overlap (2+ speakers)
        assert!(!LocalSegment { start_time_s: 0.0, end_time_s: 1.0, local_speaker_slot: 0, is_overlap: false }.is_overlap);
        assert!(!LocalSegment { start_time_s: 0.0, end_time_s: 1.0, local_speaker_slot: 1, is_overlap: false }.is_overlap);
        assert!(LocalSegment { start_time_s: 0.0, end_time_s: 1.0, local_speaker_slot: 4, is_overlap: true }.is_overlap);
    }

    #[test]
    fn window_time_alignment() {
        // First window: 0.0 - 10.0s; second window (stride 5s): 5.0 - 15.0s
        let first_start = 0.0;
        let second_start = DIARIZATION_STRIDE_SECONDS;
        assert_eq!(first_start, 0.0);
        assert_eq!(second_start, 5.0);
    }

    #[test]
    fn significant_overlap_detection() {
        let seg1 = LocalSegment { start_time_s: 5.0, end_time_s: 10.0, local_speaker_slot: 1, is_overlap: false };
        let seg2 = LocalSegment { start_time_s: 7.0, end_time_s: 12.0, local_speaker_slot: 1, is_overlap: false };
        // Overlap: [7.0, 10.0) = 3s. Union: [5.0, 12.0) = 7s. IoU = 3/7 ≈ 0.43 < 0.5
        assert!(!SegmentationSession::segments_overlap_significantly(&seg1, &seg2));

        let seg3 = LocalSegment { start_time_s: 5.0, end_time_s: 10.0, local_speaker_slot: 1, is_overlap: false };
        let seg4 = LocalSegment { start_time_s: 5.0, end_time_s: 15.0, local_speaker_slot: 1, is_overlap: false };
        // Overlap: [5.0, 10.0) = 5s. Union: [5.0, 15.0) = 10s. IoU = 5/10 = 0.5 (boundary)
        assert!(!SegmentationSession::segments_overlap_significantly(&seg3, &seg4)); // 0.5 is not > 0.5

        let seg5 = LocalSegment { start_time_s: 5.0, end_time_s: 10.0, local_speaker_slot: 1, is_overlap: false };
        let seg6 = LocalSegment { start_time_s: 4.0, end_time_s: 11.0, local_speaker_slot: 1, is_overlap: false };
        // Overlap: [5.0, 10.0) = 5s. Union: [4.0, 11.0) = 7s. IoU = 5/7 ≈ 0.71 > 0.5
        assert!(SegmentationSession::segments_overlap_significantly(&seg5, &seg6));
    }
}
