//! ONNX speaker-embedding extractor (3D-Speaker / WeSpeaker CAM++ exports).
//!
//! Input: `[1, frames, 80]` mean-normalised log-mel fbank. Output: `[1, dim]`
//! voiceprint, which we L2-normalise so cosine similarity is a dot product.

use super::fbank::{FbankExtractor, NUM_MEL_BINS};
use anyhow::{anyhow, Context, Result};
use ndarray::Array3;
use ort::execution_providers::CPUExecutionProvider;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use std::path::Path;

/// Shortest audio (seconds) we will embed. Below this the voiceprint is noise.
pub const MIN_EMBEDDING_SECONDS: f32 = 0.5;
/// Longest audio (seconds) we embed; longer chunks are centre-cropped to bound CPU time.
const MAX_EMBEDDING_SECONDS: f32 = 20.0;

pub struct SpeakerEmbeddingModel {
    session: Session,
    input_name: String,
    output_name: String,
    sample_scale: f32,
    fbank: FbankExtractor,
}

impl SpeakerEmbeddingModel {
    pub fn load(model_path: &Path) -> Result<Self> {
        crate::ensure_onnx_runtime_available()?;
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_execution_providers(vec![CPUExecutionProvider::default().build()])?
            .with_intra_threads(2)?
            .commit_from_file(model_path)
            .with_context(|| format!("loading speaker model {}", model_path.display()))?;

        let input_name = session
            .inputs
            .first()
            .map(|i| i.name.clone())
            .ok_or_else(|| anyhow!("speaker model has no inputs"))?;
        let output_name = session
            .outputs
            .first()
            .map(|o| o.name.clone())
            .ok_or_else(|| anyhow!("speaker model has no outputs"))?;

        // sherpa-onnx exports record whether samples are expected in [-1, 1]
        // (normalize_samples=1, 3D-Speaker) or int16 range (0, WeSpeaker).
        let normalize_samples = session
            .metadata()
            .ok()
            .and_then(|m| m.custom("normalize_samples").ok().flatten())
            .map(|v| v.trim() != "0")
            .unwrap_or(true);
        let sample_scale = if normalize_samples { 1.0 } else { 32768.0 };

        log::info!(
            "Speaker embedding model loaded (input={}, output={}, sample_scale={})",
            input_name,
            output_name,
            sample_scale
        );

        Ok(Self {
            session,
            input_name,
            output_name,
            sample_scale,
            fbank: FbankExtractor::new(),
        })
    }

    /// Computes an L2-normalised voiceprint for 16 kHz mono audio.
    /// Returns `Ok(None)` when the audio is too short to embed reliably.
    pub fn embed(&mut self, samples_16k: &[f32]) -> Result<Option<Vec<f32>>> {
        let min_samples = (MIN_EMBEDDING_SECONDS * 16_000.0) as usize;
        if samples_16k.len() < min_samples {
            return Ok(None);
        }
        let max_samples = (MAX_EMBEDDING_SECONDS * 16_000.0) as usize;
        let samples = if samples_16k.len() > max_samples {
            let start = (samples_16k.len() - max_samples) / 2;
            &samples_16k[start..start + max_samples]
        } else {
            samples_16k
        };

        let mut feats = self.fbank.compute(samples, self.sample_scale);
        let frames = feats.len() / NUM_MEL_BINS;
        if frames == 0 {
            return Ok(None);
        }
        // Cepstral mean normalisation across time, as both toolkits do at training.
        for bin in 0..NUM_MEL_BINS {
            let mean = (0..frames).map(|f| feats[f * NUM_MEL_BINS + bin]).sum::<f32>()
                / frames as f32;
            for f in 0..frames {
                feats[f * NUM_MEL_BINS + bin] -= mean;
            }
        }

        let input = Array3::from_shape_vec((1, frames, NUM_MEL_BINS), feats)?;
        let outputs = self.session.run(ort::inputs![
            self.input_name.as_str() => TensorRef::from_array_view(input.view())?
        ])?;
        let embedding = outputs
            .get(self.output_name.as_str())
            .ok_or_else(|| anyhow!("speaker model output missing"))?
            .try_extract_array::<f32>()?;
        let mut embedding: Vec<f32> = embedding.iter().copied().collect();
        if !l2_normalize(&mut embedding) {
            return Ok(None);
        }
        Ok(Some(embedding))
    }
}

/// Normalises in place; returns false for a zero/NaN vector.
pub fn l2_normalize(v: &mut [f32]) -> bool {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return false;
    }
    v.iter_mut().for_each(|x| *x /= norm);
    true
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a 16 kHz mono 16-bit PCM WAV into [-1, 1] floats.
    fn read_wav_16k(path: &Path) -> Vec<f32> {
        let bytes = std::fs::read(path).unwrap();
        let mut pos = 12;
        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            if id == b"data" {
                return bytes[pos + 8..(pos + 8 + size).min(bytes.len())]
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect();
            }
            pos += 8 + size + (size & 1);
        }
        panic!("no data chunk in {}", path.display());
    }

    /// Real-model speaker-change check on a scripted conversation. Run with:
    /// SPEAKER_MODEL_PATH=... SPEAKER_CONVO_DIR=/path cargo test --lib speaker::embedding -- --ignored --nocapture
    /// where the dir holds l01.wav.. (one line each, alternating speakers) and
    /// two-person-meeting.wav (the lines joined with 1.2 s of silence).
    #[test]
    #[ignore]
    fn real_model_finds_speaker_changes() {
        let model_path = std::env::var("SPEAKER_MODEL_PATH").expect("SPEAKER_MODEL_PATH");
        let dir = std::path::PathBuf::from(std::env::var("SPEAKER_CONVO_DIR").expect("SPEAKER_CONVO_DIR"));
        let mut model = SpeakerEmbeddingModel::load(Path::new(&model_path)).unwrap();

        // Ground-truth line boundaries (seconds) from the individual line files.
        let mut truth = Vec::new();
        let mut t = 0.0f32;
        for n in 1.. {
            let p = dir.join(format!("l{n:02}.wav"));
            if !p.exists() {
                break;
            }
            let len = read_wav_16k(&p).len() as f32 / 16_000.0;
            truth.push((t, t + len));
            t += len + 1.2;
        }
        let audio = read_wav_16k(&dir.join("two-person-meeting.wav"));
        // Worst case for batch import: the whole conversation as one segment.
        let spans = crate::speaker::segmentation::segment_by_speaker(&[&audio], |w| model.embed(w).ok().flatten());
        for s in &spans[0] {
            println!("{:6.2}-{:6.2}s {:?}", s.start as f32 / 16_000.0, s.end as f32 / 16_000.0, s.speaker);
        }
        println!("truth: {:?}", truth);

        // Each line's midpoint should carry the speaker of its turn (alternating A/B).
        let label_at = |sec: f32| {
            let sample = (sec * 16_000.0) as usize;
            spans[0].iter().find(|s| s.start <= sample && sample < s.end).and_then(|s| s.speaker.clone())
        };
        let labels: Vec<_> = truth.iter().map(|(a, b)| label_at((a + b) / 2.0)).collect();
        println!("line labels: {labels:?}");
        for (i, l) in labels.iter().enumerate() {
            assert_eq!(l, &labels[i % 2], "line {} has the wrong speaker", i + 1);
        }
        assert_ne!(labels[0], labels[1], "speakers merged");
    }

    /// Real-model check. Run with:
    /// SPEAKER_MODEL_PATH=/path/model.onnx SPEAKER_TEST_DIR=/path/voices \
    ///   cargo test --lib speaker::embedding -- --ignored --nocapture
    /// where SPEAKER_TEST_DIR holds v<N>_a.wav / v<N>_b.wav: two sentences per voice.
    #[test]
    #[ignore]
    fn real_model_separates_voices() {
        let model_path = std::env::var("SPEAKER_MODEL_PATH").expect("SPEAKER_MODEL_PATH");
        let dir = std::path::PathBuf::from(std::env::var("SPEAKER_TEST_DIR").expect("SPEAKER_TEST_DIR"));
        let mut model = SpeakerEmbeddingModel::load(Path::new(&model_path)).unwrap();

        let mut voices = Vec::new();
        for n in 1.. {
            let (a, b) = (dir.join(format!("v{n}_a.wav")), dir.join(format!("v{n}_b.wav")));
            if !a.exists() {
                break;
            }
            let ea = model.embed(&read_wav_16k(&a)).unwrap().unwrap();
            let eb = model.embed(&read_wav_16k(&b)).unwrap().unwrap();
            assert_eq!(ea.len(), 192);
            voices.push((ea, eb));
        }
        assert!(voices.len() >= 2);

        let mut min_same = f32::MAX;
        let mut max_diff = f32::MIN;
        for (i, (a, b)) in voices.iter().enumerate() {
            min_same = min_same.min(cosine(a, b));
            for (j, (c, d)) in voices.iter().enumerate() {
                if i != j {
                    max_diff = max_diff.max(cosine(a, c)).max(cosine(a, d)).max(cosine(b, d));
                }
            }
        }
        for (i, (a, _)) in voices.iter().enumerate() {
            let row: Vec<String> = voices
                .iter()
                .map(|(c, d)| format!("{:.2}/{:.2}", cosine(a, c), cosine(a, d)))
                .collect();
            println!("voice {}: {}", i + 1, row.join("  "));
        }
        println!("min same-voice cosine={min_same:.3} max cross-voice cosine={max_diff:.3}");
        assert!(min_same > max_diff, "voices not separable");

        // Production window-clustering threshold: no voice may be split, and any
        // two voices clearly below the threshold must end up apart.
        let threshold = crate::speaker::segmentation::WINDOW_CLUSTER_THRESHOLD;
        let interleaved: Vec<Option<Vec<f32>>> = voices
            .iter()
            .map(|(a, _)| Some(a.clone()))
            .chain(voices.iter().map(|(_, b)| Some(b.clone())))
            .collect();
        let labels = crate::speaker::clustering::cluster_offline_with_threshold(&interleaved, threshold);
        println!("offline labels: {labels:?}");
        let n = voices.len();
        for i in 0..n {
            assert_eq!(labels[i], labels[i + n], "voice {} split across clusters", i + 1);
        }
        for i in 0..n {
            for j in (i + 1)..n {
                let (a, b) = (&voices[i], &voices[j]);
                let max_sim = [cosine(&a.0, &b.0), cosine(&a.0, &b.1), cosine(&a.1, &b.0), cosine(&a.1, &b.1)]
                    .into_iter()
                    .fold(f32::MIN, f32::max);
                if max_sim < threshold - 0.1 {
                    assert_ne!(labels[i], labels[j], "voices {} and {} merged (max sim {max_sim:.2})", i + 1, j + 1);
                }
            }
        }

        // Online clustering (system audio, no mic) on the same order.
        let mut online = crate::speaker::OnlineSpeakerClusterer::new(true);
        let online_labels: Vec<_> = interleaved
            .iter()
            .map(|e| online.assign(e.as_deref(), crate::speaker::SpeakerSource::System, 4.0))
            .collect();
        println!("online labels: {online_labels:?}");
        for i in 0..n {
            assert_eq!(online_labels[i], online_labels[i + n], "online: voice {} split", i + 1);
        }
    }
}
