// diarization/offline.rs
//
// Offline diarization pipeline: segmentation → embeddings → AHC clustering → profile matching.
// Orchestrates the complete offline diarization for retranscription and post-recording refinement.

use super::clustering::{cosine_similarity, PROFILE_MATCH_THRESHOLD};
use super::clustering_offline::{self, AHC_DISTANCE_THRESHOLD};
use super::embedding::{EmbeddingError, EmbeddingExtractor};
use super::segmentation::{SegmentationError, SegmentationSession};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct SpeakerSegment {
    pub start_time_s: f64,
    pub end_time_s: f64,
    pub label: String,
    pub is_overlap: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum OfflineDiarizationError {
    #[error("Segmentation error: {0}")]
    Segmentation(#[from] SegmentationError),
    #[error("Embedding error: {0}")]
    Embedding(#[from] EmbeddingError),
    #[error("File error: {0}")]
    File(#[from] std::io::Error),
}

/// Complete offline diarization pipeline.
///
/// # Arguments
/// * `segmentation_model_path` - Path to pyannote-segmentation-3.0 ONNX model
/// * `embedding_model_path` - Path to WeSpeaker embedding model (V2, 256-dim)
/// * `samples_16k` - Complete recording as 16kHz mono f32 samples
/// * `expected_speakers` - K for clustering (from user input)
/// * `profiles` - Pre-computed speaker profiles [(name, 256-dim embedding)]
///
/// # Returns
/// Vec<SpeakerSegment> sorted by start_time_s
pub async fn diarize_offline(
    segmentation_model_path: &Path,
    embedding_model_path: &Path,
    samples_16k: &[f32],
    expected_speakers: usize,
    profiles: &[(String, Vec<f32>)],
) -> Result<Vec<SpeakerSegment>, OfflineDiarizationError> {
    let start = Instant::now();
    let total_duration = samples_16k.len() as f64 / 16_000.0;

    log::info!(
        "Diarization offline: {} samples ({:.1}s), {} expected speakers",
        samples_16k.len(),
        total_duration,
        expected_speakers
    );

    // 1. Segmentation: identify speech boundaries
    let mut segmentation_session = SegmentationSession::new(segmentation_model_path)?;
    let local_segments = segmentation_session.segment(samples_16k)?;
    log::debug!("Segmentation produced {} local segments", local_segments.len());

    // 2. Extract embeddings for each segment (skip short ones)
    let mut embedding_extractor = EmbeddingExtractor::new(embedding_model_path)?;
    let embedding_dim = detect_embedding_dim(&mut embedding_extractor, samples_16k)?;
    log::debug!("Detected embedding dimension: {}", embedding_dim);

    let mut segment_embeddings: Vec<Vec<f32>> = Vec::new();
    let mut segment_times: Vec<(f64, f64)> = Vec::new();

    for segment in &local_segments {
        let segment_start = (segment.start_time_s * 16_000.0) as usize;
        let segment_end = (segment.end_time_s * 16_000.0) as usize;

        if segment_end > samples_16k.len() {
            continue; // Skip segments that extend past audio
        }

        let segment_samples = &samples_16k[segment_start..segment_end];

        // Skip segments shorter than ~0.5s (too unreliable for embedding)
        if segment_samples.len() < 8_000 {
            continue;
        }

        match embedding_extractor.compute(segment_samples) {
            Ok(embedding) => {
                segment_embeddings.push(embedding);
                segment_times.push((segment.start_time_s, segment.end_time_s));
            }
            Err(e) => {
                log::warn!(
                    "Embedding extraction failed for segment [{}, {}s): {}",
                    segment.start_time_s,
                    segment.end_time_s,
                    e
                );
            }
        }
    }

    if segment_embeddings.is_empty() {
        log::warn!("No valid embeddings extracted; returning empty diarization");
        return Ok(Vec::new());
    }

    log::debug!(
        "Extracted {} embeddings from {} local segments",
        segment_embeddings.len(),
        local_segments.len()
    );

    // 3. Cluster embeddings with AHC
    let cluster_ids = clustering_offline::cluster(
        &segment_embeddings,
        expected_speakers,
        AHC_DISTANCE_THRESHOLD,
    );

    // 4. Compute centroids per cluster
    let mut cluster_centroids: Vec<Vec<f32>> = vec![Vec::new(); expected_speakers];
    let mut cluster_counts: Vec<usize> = vec![0; expected_speakers];

    for (embedding, &cluster_id) in segment_embeddings.iter().zip(cluster_ids.iter()) {
        if cluster_id < expected_speakers {
            if cluster_centroids[cluster_id].is_empty() {
                cluster_centroids[cluster_id] = embedding.clone();
            } else {
                let n = cluster_counts[cluster_id] as f32;
                for (c, e) in cluster_centroids[cluster_id].iter_mut().zip(embedding.iter()) {
                    *c = (*c * n + e) / (n + 1.0);
                }
                let norm: f32 = cluster_centroids[cluster_id].iter().map(|v| v * v).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for c in &mut cluster_centroids[cluster_id] {
                        *c /= norm;
                    }
                }
            }
            cluster_counts[cluster_id] += 1;
        }
    }

    let num_clusters = cluster_counts.iter().filter(|&&c| c > 0).count();
    log::debug!("Clustering produced {} active clusters", num_clusters);

    // 5. Match clusters to saved profiles
    let mut cluster_labels: Vec<String> = (0..expected_speakers)
        .map(|i| format!("Speaker {}", i + 1))
        .collect();

    let mut profile_matches = 0;
    for (profile_name, profile_centroid) in profiles {
        // Check embedding dimension compatibility
        if profile_centroid.len() != embedding_dim {
            log::warn!(
                "Profile '{}' dimension {} != embedding dimension {}; skipping",
                profile_name,
                profile_centroid.len(),
                embedding_dim
            );
            continue;
        }

        // Find best-matching cluster
        let mut best_cluster_idx = None;
        let mut best_similarity = PROFILE_MATCH_THRESHOLD;

        for (cluster_id, centroid) in cluster_centroids.iter().enumerate() {
            if cluster_counts[cluster_id] == 0 || !centroid.is_empty() {
                let similarity = cosine_similarity(profile_centroid, centroid);
                if similarity > best_similarity {
                    best_similarity = similarity;
                    best_cluster_idx = Some(cluster_id);
                }
            }
        }

        if let Some(idx) = best_cluster_idx {
            cluster_labels[idx] = profile_name.clone();
            profile_matches += 1;
            log::debug!(
                "Profile '{}' matched to cluster {} (similarity {:.3})",
                profile_name,
                idx,
                best_similarity
            );
        }
    }

    // 6. Assign cluster labels to segments
    let mut result = Vec::new();
    for (embedding, (segment_start, segment_end)) in segment_embeddings.iter().zip(segment_times.iter()) {
        // Find which cluster this embedding belongs to
        let mut best_cluster = 0;
        let mut best_sim = -1.0;
        for (cluster_id, centroid) in cluster_centroids.iter().enumerate() {
            if cluster_counts[cluster_id] > 0 {
                let sim = cosine_similarity(embedding, centroid);
                if sim > best_sim {
                    best_sim = sim;
                    best_cluster = cluster_id;
                }
            }
        }

        result.push(SpeakerSegment {
            start_time_s: *segment_start,
            end_time_s: *segment_end,
            label: cluster_labels[best_cluster].clone(),
            is_overlap: false, // TODO: could use segmentation overlap flag
        });
    }

    result.sort_by(|a, b| a.start_time_s.partial_cmp(&b.start_time_s).unwrap_or(std::cmp::Ordering::Equal));

    let elapsed = start.elapsed().as_secs_f64();
    log::info!(
        "Diarization offline complete: {:.1}s audio, {} clusters, {} profiles matched, {} segments, {:.1}s elapsed",
        total_duration,
        num_clusters,
        profile_matches,
        result.len(),
        elapsed
    );

    Ok(result)
}

/// Detect embedding output dimension by running inference on a small sample.
fn detect_embedding_dim(
    extractor: &mut EmbeddingExtractor,
    samples_16k: &[f32],
) -> Result<usize, OfflineDiarizationError> {
    // Try to extract from first 1 second
    let sample_size = (16_000).min(samples_16k.len());
    let sample = &samples_16k[..sample_size];

    let embedding = extractor.compute(sample)?;
    Ok(embedding.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_segment_basic_construction() {
        let segment = SpeakerSegment {
            start_time_s: 0.0,
            end_time_s: 5.0,
            label: "Speaker 1".to_string(),
            is_overlap: false,
        };
        assert_eq!(segment.label, "Speaker 1");
        assert!((segment.end_time_s - segment.start_time_s - 5.0).abs() < 0.01);
    }

    #[test]
    fn segments_sorted_by_time() {
        let mut segments = vec![
            SpeakerSegment { start_time_s: 10.0, end_time_s: 15.0, label: "A".to_string(), is_overlap: false },
            SpeakerSegment { start_time_s: 0.0, end_time_s: 5.0, label: "B".to_string(), is_overlap: false },
            SpeakerSegment { start_time_s: 5.0, end_time_s: 10.0, label: "C".to_string(), is_overlap: false },
        ];
        segments.sort_by(|a, b| a.start_time_s.partial_cmp(&b.start_time_s).unwrap_or(std::cmp::Ordering::Equal));
        assert_eq!(segments[0].start_time_s, 0.0);
        assert_eq!(segments[1].start_time_s, 5.0);
        assert_eq!(segments[2].start_time_s, 10.0);
    }
}
