//! CAM++ speaker embeddings from Kaldi filterbank features.
use super::fbank::{Fbank, NUM_MEL_BINS};
use anyhow::{anyhow, Result};
use ndarray::Array3;
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use std::path::Path;

const INPUT: &str = "x";
const OUTPUT: &str = "embedding";

pub struct EmbeddingModel {
    session: Session,
    fbank: Fbank,
    /// Model expects int16-scaled samples (metadata normalize_samples = 0).
    scale_to_int16: bool,
    /// Subtract the per-utterance mean of each feature dimension.
    global_mean: bool,
    /// Embedding size from the model metadata (512 for CAM++ VoxCeleb), when present.
    output_dim: Option<usize>,
}

impl EmbeddingModel {
    pub fn load(path: &Path) -> Result<Self> {
        crate::ensure_onnx_runtime_available()?;
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_execution_providers(vec![CPUExecutionProvider::default().build()])?
            .commit_from_file(path)?;
        let meta = |key: &str| session.metadata().ok().and_then(|m| m.custom(key).ok().flatten());
        let scale_to_int16 = meta("normalize_samples").map(|v| v == "0").unwrap_or(false);
        let global_mean = meta("feature_normalize_type").map(|v| v == "global-mean").unwrap_or(true);
        let sample_rate: usize = meta("sample_rate").and_then(|v| v.parse().ok()).unwrap_or(16_000);
        let output_dim: Option<usize> = meta("output_dim").and_then(|v| v.parse().ok());
        log::info!(
            "Loaded speaker embedding model: int16_scale={} global_mean={} output_dim={:?}",
            scale_to_int16, global_mean, output_dim
        );
        Ok(Self { session, fbank: Fbank::new(sample_rate), scale_to_int16, global_mean, output_dim })
    }

    pub fn embed(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        let scaled: Vec<f32>;
        let input_samples = if self.scale_to_int16 {
            scaled = samples.iter().map(|x| x * 32768.0).collect();
            &scaled[..]
        } else {
            samples
        };
        let mut feats = self.fbank.compute(input_samples);
        if feats.is_empty() {
            return Err(anyhow!("audio too short for a speaker embedding"));
        }
        if self.global_mean {
            let n = feats.len() as f32;
            let mut mean = [0f32; NUM_MEL_BINS];
            for f in &feats {
                for (m, x) in mean.iter_mut().zip(f) {
                    *m += x / n;
                }
            }
            for f in &mut feats {
                for (x, m) in f.iter_mut().zip(&mean) {
                    *x -= m;
                }
            }
        }
        let mut input = Array3::<f32>::zeros((1, feats.len(), NUM_MEL_BINS));
        for (t, f) in feats.iter().enumerate() {
            for (d, &x) in f.iter().enumerate() {
                input[[0, t, d]] = x;
            }
        }
        let outputs = self.session.run(inputs![INPUT => TensorRef::from_array_view(input.view())?])?;
        let emb: Vec<f32> = outputs
            .get(OUTPUT)
            .ok_or_else(|| anyhow!("embedding output '{OUTPUT}' missing"))?
            .try_extract_array::<f32>()?
            .iter()
            .copied()
            .collect();
        if let Some(d) = self.output_dim {
            if emb.len() != d {
                return Err(anyhow!("embedding length {} != model output_dim {}", emb.len(), d));
            }
        }
        Ok(emb)
    }
}
