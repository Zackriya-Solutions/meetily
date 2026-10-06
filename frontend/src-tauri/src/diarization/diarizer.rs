//! End-to-end diarization: segmentation → embeddings → clustering → turns.
use super::cluster::{cluster_centroids, cluster_speakers};
use super::embedding::EmbeddingModel;
use super::models::{EMBEDDING, SEGMENTATION};
use super::reconstruct::{powerset_to_multilabel, reconstruct, smooth_turns, RawTurn, WindowActivity, NUM_LOCAL};
use super::segmentation::SegmentationModel;
use super::{Cancelled, Turn};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

pub const DEFAULT_THRESHOLD: f32 = 0.5;
pub const WINDOW_STEP_S: f64 = 2.5;
pub const MIN_EMBED_S: f64 = 0.5;
pub const MIN_TURN_S: f64 = 0.3;
pub const MAX_GAP_S: f64 = 0.5;
const BATCH: usize = 8;

#[derive(Debug, Clone)]
pub struct DiarizeOptions {
    pub num_speakers: Option<usize>,
    pub threshold: f32,
}

impl Default for DiarizeOptions {
    fn default() -> Self {
        Self { num_speakers: None, threshold: DEFAULT_THRESHOLD }
    }
}

#[derive(Debug, Clone)]
pub struct SpeakerCentroid {
    pub key: String,
    pub embedding: Vec<f32>,
    pub speech_seconds: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Diarization {
    pub turns: Vec<Turn>,
    pub speakers: Vec<SpeakerCentroid>,
}

pub fn window_starts(total: usize, window: usize, step: usize) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let mut starts = vec![0];
    while starts.last().unwrap() + window < total {
        starts.push(starts.last().unwrap() + step);
    }
    starts
}

/// The part of the `window` samples starting at `start` that lies inside the audio; the
/// segmentation model zero-pads the rest.
pub fn window_slice(samples: &[f32], start: usize, window: usize) -> &[f32] {
    let end = (start + window).min(samples.len());
    &samples[start.min(end)..end]
}

/// Key clusters `spk_0..` by first speech, drop clusters without turns, and build
/// speech-weighted centroids from each embedding, its clean seconds and its cluster label.
pub fn finalize(raw: Vec<RawTurn>, embeddings: &[Vec<f32>], weights: &[f64], labels: &[usize]) -> Diarization {
    let mut order: Vec<usize> = Vec::new();
    for t in &raw {
        if !order.contains(&t.cluster) {
            order.push(t.cluster);
        }
    }
    let key_of: HashMap<usize, String> = order.iter().enumerate().map(|(i, c)| (*c, format!("spk_{i}"))).collect();
    let k = order.iter().max().map_or(0, |m| m + 1);
    let mut centroids = cluster_centroids(embeddings, weights, labels, k);
    let speakers = order
        .iter()
        .map(|c| SpeakerCentroid {
            key: key_of[c].clone(),
            embedding: std::mem::take(&mut centroids[*c]),
            speech_seconds: raw.iter().filter(|t| t.cluster == *c).map(|t| t.end_s - t.start_s).sum(),
        })
        .collect();
    let turns = raw
        .into_iter()
        .map(|t| Turn { start_s: t.start_s, end_s: t.end_s, key: key_of[&t.cluster].clone() })
        .collect();
    Diarization { turns, speakers }
}

pub struct Diarizer {
    segmentation: SegmentationModel,
    embedding: EmbeddingModel,
}

impl Diarizer {
    pub fn load(models_dir: &Path) -> Result<Self> {
        Ok(Self {
            segmentation: SegmentationModel::load(&models_dir.join(SEGMENTATION.file_name))?,
            embedding: EmbeddingModel::load(&models_dir.join(EMBEDDING.file_name))?,
        })
    }

    /// Diarize 16 kHz mono samples. `progress` receives 0–100.
    pub fn diarize(
        &mut self,
        samples: &[f32],
        opts: &DiarizeOptions,
        progress: &mut dyn FnMut(u32),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Diarization> {
        let started = Instant::now();
        let geo = self.segmentation.geometry;
        let window = self.segmentation.window_samples;
        let step = (WINDOW_STEP_S * geo.sample_rate as f64) as usize;
        let starts = window_starts(samples.len(), window, step);
        if starts.is_empty() {
            return Ok(Diarization::default());
        }
        // Report each percent once.
        let mut last_percent = None;
        let mut progress = |p: u32| {
            if last_percent.replace(p) != Some(p) {
                progress(p);
            }
        };

        // 1. Segmentation (0–40 %).
        let mut activities: Vec<Vec<[f32; NUM_LOCAL]>> = Vec::with_capacity(starts.len());
        for (b, chunk) in starts.chunks(BATCH).enumerate() {
            if cancelled() {
                return Err(Cancelled.into());
            }
            let windows: Vec<&[f32]> = chunk.iter().map(|&s| window_slice(samples, s, window)).collect();
            for scores in self.segmentation.run_batch(&windows)? {
                activities.push(powerset_to_multilabel(&scores));
            }
            progress((((b + 1) * BATCH).min(starts.len()) * 40 / starts.len()) as u32);
        }
        let seg_done = started.elapsed();

        // 2. One embedding per (window, local speaker) with enough clean speech (40–90 %).
        // Each frame's slice is centred on the frame, where `reconstruct` places it.
        let half_gap = geo.frame_size.saturating_sub(geo.frame_shift) / 2;
        let mut members_raw: Vec<(usize, usize, Vec<f32>, f64)> = Vec::new();
        for (w, activity) in activities.iter().enumerate() {
            if cancelled() {
                return Err(Cancelled.into());
            }
            for local in 0..NUM_LOCAL {
                let clean: Vec<usize> = activity
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| f[local] > 0.5 && f.iter().sum::<f32>() < 1.5)
                    .map(|(i, _)| i)
                    .collect();
                let clean_s = clean.len() as f64 * geo.frame_shift as f64 / geo.sample_rate as f64;
                if clean_s < MIN_EMBED_S {
                    continue;
                }
                let mut audio = Vec::with_capacity(clean.len() * geo.frame_shift);
                for i in clean {
                    let a = starts[w] + i * geo.frame_shift + half_gap;
                    if a >= samples.len() {
                        break;
                    }
                    audio.extend_from_slice(&samples[a..(a + geo.frame_shift).min(samples.len())]);
                }
                if audio.len() < (MIN_EMBED_S * geo.sample_rate as f64) as usize {
                    continue;
                }
                let emb = self.embedding.embed(&audio)?;
                // The embedding model can emit NaN on odd input; such vectors would poison clustering.
                if emb.is_empty() || !emb.iter().all(|x| x.is_finite()) || emb.iter().all(|&x| x == 0.0) {
                    log::debug!("Skipping non-finite or empty speaker embedding (window {w}, local {local})");
                    continue;
                }
                members_raw.push((w, local, emb, clean_s));
            }
            progress(40 + ((w + 1) * 50 / activities.len()) as u32);
        }
        let emb_done = started.elapsed();
        if members_raw.is_empty() {
            return Ok(Diarization::default());
        }

        // 3. Cluster and reconstruct (90–100 %). Clustering memory is bounded by
        // cluster::MAX_CLUSTER_POINTS.
        if cancelled() {
            return Err(Cancelled.into());
        }
        let embeddings: Vec<Vec<f32>> = members_raw.iter().map(|m| m.2.clone()).collect();
        let weights: Vec<f64> = members_raw.iter().map(|m| m.3).collect();
        let labels = cluster_speakers(&embeddings, &weights, opts.num_speakers, opts.threshold);

        let mut local_to_global = vec![[None; NUM_LOCAL]; activities.len()];
        for ((w, local, _, _), &label) in members_raw.iter().zip(&labels) {
            local_to_global[*w][*local] = Some(label);
        }
        let windows: Vec<WindowActivity> = activities
            .into_iter()
            .enumerate()
            .map(|(w, activity)| WindowActivity { start_sample: starts[w], activity, local_to_global: local_to_global[w] })
            .collect();
        let raw = smooth_turns(reconstruct(&windows, samples.len(), geo), MIN_TURN_S, MAX_GAP_S);
        let result = finalize(raw, &embeddings, &weights, &labels);
        progress(100);

        log::info!(
            "Diarized {:.1}s audio: {} windows, {} embeddings, {} speakers, {} turns (segmentation {:?}, embeddings {:?}, total {:?})",
            samples.len() as f64 / geo.sample_rate as f64,
            starts.len(),
            embeddings.len(),
            result.speakers.len(),
            result.turns.len(),
            seg_done,
            emb_done - seg_done,
            started.elapsed()
        );
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diarization::assign::{label_rows, RowLabel, RowSpan};
    use crate::diarization::reconstruct::RawTurn;

    #[test]
    fn window_starts_cover_short_and_long_audio() {
        assert_eq!(window_starts(0, 100, 25), Vec::<usize>::new());
        assert_eq!(window_starts(40, 100, 25), vec![0]);
        assert_eq!(window_starts(150, 100, 25), vec![0, 25, 50]);
    }

    #[test]
    fn finalize_orders_keys_by_first_speech_and_drops_silent_clusters() {
        let raw = vec![
            RawTurn { start_s: 0.0, end_s: 2.0, cluster: 2 },
            RawTurn { start_s: 2.0, end_s: 3.0, cluster: 0 },
            RawTurn { start_s: 3.0, end_s: 5.0, cluster: 2 },
        ];
        let embeddings = vec![vec![1.0, 0.0], vec![0.0, 1.0], vec![0.6, 0.8]];
        let d = finalize(raw, &embeddings, &[1.0, 1.0, 2.0], &[0, 1, 2]);
        assert_eq!(d.speakers.len(), 2);
        assert_eq!(d.speakers[0].key, "spk_0");
        assert_eq!(d.speakers[0].speech_seconds, 4.0);
        assert!((d.speakers[0].embedding[0] - 0.6).abs() < 1e-5);
        assert_eq!(d.turns.iter().map(|t| t.key.as_str()).collect::<Vec<_>>(), vec!["spk_0", "spk_1", "spk_0"]);
    }

    #[test]
    fn short_clip_fills_part_of_one_window() {
        let clip = vec![0.5f32; 40];
        assert_eq!(window_starts(clip.len(), 100, 25), vec![0]);
        assert_eq!(window_slice(&clip, 0, 100), &clip[..]);
        assert_eq!(window_slice(&clip, 25, 100), &clip[25..]);
        assert!(window_slice(&clip, 50, 100).is_empty());
    }

    #[test]
    fn finalize_of_nothing_is_empty() {
        let d = finalize(Vec::new(), &[], &[], &[]);
        assert!(d.speakers.is_empty() && d.turns.is_empty());
    }

    #[test]
    #[ignore = "needs DIARIZATION_REF_DIR with models"]
    fn silence_yields_no_speakers() {
        let dir = std::path::PathBuf::from(std::env::var("DIARIZATION_REF_DIR").expect("DIARIZATION_REF_DIR"));
        let mut d = Diarizer::load(&dir).unwrap();
        let out = d.diarize(&vec![0.0f32; 16000 * 3], &DiarizeOptions::default(), &mut |_| {}, &|| false).unwrap();
        assert!(out.speakers.is_empty() && out.turns.is_empty());
    }

    #[test]
    #[ignore = "needs DIARIZATION_REF_DIR with models, mix_en.wav and reference.json"]
    fn separates_speakers_in_the_synthetic_meeting() {
        let dir = std::path::PathBuf::from(std::env::var("DIARIZATION_REF_DIR").expect("DIARIZATION_REF_DIR"));
        let reference: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("reference.json")).unwrap()).unwrap();
        let samples = crate::audio::decoder::decode_audio_file(&dir.join("mix_en.wav")).unwrap().to_whisper_format();

        let started = std::time::Instant::now();
        let mut diarizer = Diarizer::load(&dir).unwrap();
        let d = diarizer.diarize(&samples, &DiarizeOptions::default(), &mut |_| {}, &|| false).unwrap();
        eprintln!("diarized {:.1}s of audio in {:?}", samples.len() as f64 / 16000.0, started.elapsed());

        // Each true speaker maps to exactly one key, and different speakers to different keys.
        let mut key_of: std::collections::HashMap<String, String> = Default::default();
        for seg in reference["en_truth"].as_array().unwrap() {
            let (s, e, who) = (seg[0].as_f64().unwrap(), seg[1].as_f64().unwrap(), seg[2].as_str().unwrap().to_string());
            let key = match &label_rows(&[Some(RowSpan { start_s: s, end_s: e })], &d.turns)[0] {
                RowLabel::Single(k) => k.clone(),
                RowLabel::Mixed { majority, .. } => majority.clone(),
                other => panic!("segment {s}-{e} unlabelled: {other:?}"),
            };
            if let Some(prev) = key_of.insert(who.clone(), key.clone()) {
                assert_eq!(prev, key, "{who} got two keys");
            }
        }
        let distinct: std::collections::HashSet<_> = key_of.values().collect();
        assert_eq!(distinct.len(), key_of.len(), "two speakers share a key: {key_of:?}");
        assert_eq!(d.speakers.len(), key_of.len());
    }

    /// Our embedding of every clip listed under `key` in reference.json, checked against the
    /// reference embedding of the same clip. Returns (speaker, embedding) pairs.
    fn embed_reference_clips(dir: &std::path::Path, reference: &serde_json::Value, key: &str) -> Vec<(String, Vec<f32>)> {
        let mut model = crate::diarization::embedding::EmbeddingModel::load(&dir.join("campplus-voxceleb.onnx")).unwrap();
        let mut ours = Vec::new();
        for (file, expected) in reference[key].as_object().unwrap_or_else(|| panic!("reference.json has no {key}")) {
            let samples = crate::audio::decoder::decode_audio_file(&dir.join(file)).unwrap().to_whisper_format();
            let e = model.embed(&samples).unwrap();
            let expected: Vec<f32> = expected.as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
            let sim = crate::diarization::cluster::cosine(&e, &expected);
            assert!(sim >= 0.999, "{file}: cosine to reference {sim}");
            let name = std::path::Path::new(file).file_name().unwrap().to_str().unwrap();
            ours.push((name.split('-').next().unwrap().to_string(), e));
        }
        ours
    }

    #[test]
    #[ignore = "needs DIARIZATION_REF_DIR with models, wavs and reference.json"]
    fn embeddings_match_reference() {
        let dir = std::path::PathBuf::from(std::env::var("DIARIZATION_REF_DIR").expect("DIARIZATION_REF_DIR"));
        let reference: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("reference.json")).unwrap()).unwrap();
        let mut checked = 0;
        for key in ["embeddings", "en_embeddings"] {
            let clips = embed_reference_clips(&dir, &reference, key).len();
            assert!(clips > 0, "reference.json has no clips under {key}");
            checked += clips;
        }
        eprintln!("{checked} clips match the reference embeddings");
    }

    #[test]
    #[ignore = "needs DIARIZATION_REF_DIR with models, English wavs and reference.json"]
    fn embeddings_separate_english_speakers() {
        let dir = std::path::PathBuf::from(std::env::var("DIARIZATION_REF_DIR").expect("DIARIZATION_REF_DIR"));
        let reference: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("reference.json")).unwrap()).unwrap();
        let ours = embed_reference_clips(&dir, &reference, "en_embeddings");
        for (i, (a, ea)) in ours.iter().enumerate() {
            for (b, eb) in ours.iter().skip(i + 1) {
                let s = crate::diarization::cluster::cosine(ea, eb);
                eprintln!("{a} vs {b}: {s:.3}");
                if a == b {
                    assert!(s > 0.5, "same speaker {a} too far apart: {s}");
                } else {
                    assert!(s < 0.5, "different speakers {a}/{b} too close: {s}");
                }
            }
        }
    }
}
