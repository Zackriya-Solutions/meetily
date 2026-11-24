// diarization/clustering.rs
//
// Online speaker clustering over L2-normalized embeddings.
// Each incoming embedding joins the nearest cluster centroid if cosine
// similarity exceeds the threshold, otherwise starts a new "Speaker N"
// cluster. Centroids are running means, re-normalized after update.

use super::voiceprints::Voiceprint;

/// Minimum cosine similarity for an embedding to join an existing cluster.
/// Tuned for WeSpeaker CAM++ embeddings; raise to split more aggressively.
pub const CLUSTER_SIMILARITY_THRESHOLD: f32 = 0.55;

/// Most anonymous speakers one meeting can mint. Past this, an outlier
/// embedding (noise, music, a cough) joins the nearest speaker instead of
/// inventing a new one. At 6, a staff call with 12 people talking put up
/// to 6 of them under one label; 20 kept different people apart in 97% of
/// pairs instead of 79% (replayed with `replay_meeting` in session.rs).
pub const DEFAULT_MAX_SPEAKERS: usize = 20;

struct SpeakerCluster {
    centroid: Vec<f32>,
    count: usize,
    label: String,
}

pub struct SpeakerClusterer {
    clusters: Vec<SpeakerCluster>,
    last_label: Option<String>,
    max_speakers: usize,
    threshold: f32,
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    // Inputs are L2-normalized, so the dot product is the cosine similarity
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

impl SpeakerClusterer {
    pub fn new() -> Self {
        Self::with_max_speakers(DEFAULT_MAX_SPEAKERS)
    }

    pub fn with_max_speakers(max_speakers: usize) -> Self {
        Self::with_settings(max_speakers, CLUSTER_SIMILARITY_THRESHOLD)
    }

    pub fn with_settings(max_speakers: usize, threshold: f32) -> Self {
        Self {
            clusters: Vec::new(),
            last_label: None,
            max_speakers: max_speakers.max(1),
            threshold,
        }
    }

    /// Label of the most recently assigned segment (used to carry labels
    /// across segments too short for reliable embeddings).
    pub fn last_label(&self) -> Option<String> {
        self.last_label.clone()
    }

    pub fn speaker_count(&self) -> usize {
        self.clusters.len()
    }

    pub fn voiceprints(&self) -> Vec<Voiceprint> {
        self.clusters
            .iter()
            .map(|c| Voiceprint { label: c.label.clone(), segments: c.count, embedding: c.centroid.clone(), name: None, dismissed: false, lines: 0 })
            .collect()
    }

    /// Assign an L2-normalized embedding to a cluster, returning its label.
    pub fn assign(&mut self, embedding: &[f32]) -> String {
        let nearest = self
            .clusters
            .iter()
            .enumerate()
            .map(|(i, c)| (i, cosine_similarity(embedding, &c.centroid)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let label = match nearest {
            Some((idx, similarity)) if similarity >= self.threshold => {
                self.update_cluster(idx, embedding)
            }
            Some((idx, _)) if self.clusters.len() >= self.max_speakers => {
                self.clusters[idx].label.clone()
            }
            _ => self.create_cluster(embedding),
        };

        self.last_label = Some(label.clone());
        label
    }

    /// The label of a speaker already heard whose voice this embedding
    /// matches, without creating a speaker or moving any centroid. No match
    /// also clears the last label, so the next too-short line does not
    /// carry the speaker from before this unknown voice.
    pub fn match_existing(&mut self, embedding: &[f32]) -> Option<String> {
        self.last_label = self
            .clusters
            .iter()
            .map(|c| (c, cosine_similarity(embedding, &c.centroid)))
            .filter(|(_, similarity)| *similarity >= self.threshold)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(c, _)| c.label.clone());
        self.last_label.clone()
    }

    fn update_cluster(&mut self, idx: usize, embedding: &[f32]) -> String {
        let cluster = &mut self.clusters[idx];
        let n = cluster.count as f32;
        for (c, e) in cluster.centroid.iter_mut().zip(embedding.iter()) {
            *c = (*c * n + e) / (n + 1.0);
        }
        let norm = cluster.centroid.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for c in &mut cluster.centroid {
                *c /= norm;
            }
        }
        cluster.count += 1;
        cluster.label.clone()
    }

    fn create_cluster(&mut self, embedding: &[f32]) -> String {
        let label = format!("Speaker {}", self.clusters.len() + 1);
        self.clusters.push(SpeakerCluster {
            centroid: embedding.to_vec(),
            count: 1,
            label: label.clone(),
        });
        label
    }
}

/// Indices of the windows where a new voice takes over, given the embeddings
/// of overlapping windows across one segment. A window starts a new run when
/// it and the window after it both fall below `threshold` against the mean
/// of the current run; one odd window (a laugh, a cough) does not. Runs are
/// at least two windows long.
pub fn change_windows(embeddings: &[Vec<f32>], threshold: f32) -> Vec<usize> {
    let mut changes = Vec::new();
    let Some(first) = embeddings.first() else {
        return changes;
    };
    let mut run_sum = first.clone();
    let mut run_start = 0;
    let similarity_to_run = |run_sum: &[f32], e: &[f32]| {
        let norm = run_sum.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            run_sum.iter().zip(e).map(|(s, x)| s * x).sum::<f32>() / norm
        } else {
            0.0
        }
    };

    for i in 1..embeddings.len() {
        let departs = i - run_start >= 2
            && i + 1 < embeddings.len()
            && similarity_to_run(&run_sum, &embeddings[i]) < threshold
            && similarity_to_run(&run_sum, &embeddings[i + 1]) < threshold;
        if departs {
            changes.push(i);
            run_start = i;
            run_sum = embeddings[i].clone();
        } else {
            for (s, e) in run_sum.iter_mut().zip(&embeddings[i]) {
                *s += e;
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_where_a_second_voice_takes_over() {
        let a = unit(vec![1.0, 0.05, 0.0]);
        let b = unit(vec![0.0, 1.0, 0.05]);
        let windows = vec![a.clone(), a.clone(), a.clone(), b.clone(), b.clone(), b.clone()];
        assert_eq!(change_windows(&windows, 0.4), vec![3]);
    }

    #[test]
    fn match_existing_never_creates_a_speaker() {
        let mut c = SpeakerClusterer::new();
        let a = unit(vec![1.0, 0.05, 0.0]);
        let stranger = unit(vec![0.0, 1.0, 0.0]);
        assert_eq!(c.match_existing(&a), None);
        c.assign(&a);
        assert_eq!(c.match_existing(&stranger), None);
        assert_eq!(c.last_label(), None, "an unknown voice clears the carried label");
        assert_eq!(c.match_existing(&a).as_deref(), Some("Speaker 1"));
        assert_eq!(c.speaker_count(), 1);
    }

    #[test]
    fn ignores_one_odd_window() {
        let a = unit(vec![1.0, 0.05, 0.0]);
        let cough = unit(vec![0.0, 0.0, 1.0]);
        let windows = vec![a.clone(), a.clone(), cough, a.clone(), a.clone()];
        assert!(change_windows(&windows, 0.4).is_empty());
    }

    #[test]
    fn finds_a_voice_coming_back() {
        let a = unit(vec![1.0, 0.05, 0.0]);
        let b = unit(vec![0.0, 1.0, 0.05]);
        let windows = vec![a.clone(), a.clone(), b.clone(), b.clone(), a.clone(), a.clone()];
        assert_eq!(change_windows(&windows, 0.4), vec![2, 4]);
    }

    fn unit(v: Vec<f32>) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.into_iter().map(|x| x / n).collect()
    }

    #[test]
    fn same_voice_same_cluster() {
        let mut c = SpeakerClusterer::new();
        let a = unit(vec![1.0, 0.1, 0.0]);
        let a2 = unit(vec![0.95, 0.15, 0.05]);
        assert_eq!(c.assign(&a), "Speaker 1");
        assert_eq!(c.assign(&a2), "Speaker 1");
        let prints = c.voiceprints();
        assert_eq!(prints.len(), 1);
        assert_eq!((prints[0].label.as_str(), prints[0].segments), ("Speaker 1", 2));
    }

    #[test]
    fn different_voice_new_cluster() {
        let mut c = SpeakerClusterer::new();
        let a = unit(vec![1.0, 0.0, 0.0]);
        let b = unit(vec![0.0, 1.0, 0.0]);
        assert_eq!(c.assign(&a), "Speaker 1");
        assert_eq!(c.assign(&b), "Speaker 2");
        assert_eq!(c.last_label().as_deref(), Some("Speaker 2"));
    }

    #[test]
    fn caps_speakers_and_gives_outliers_the_nearest_label() {
        let mut c = SpeakerClusterer::with_max_speakers(2);
        let a = unit(vec![1.0, 0.0, 0.0]);
        let b = unit(vec![0.0, 1.0, 0.0]);
        let outlier = unit(vec![0.1, 0.0, 1.0]);

        assert_eq!(c.assign(&a), "Speaker 1");
        assert_eq!(c.assign(&b), "Speaker 2");
        assert_eq!(c.assign(&outlier), "Speaker 1");
        assert_eq!(c.speaker_count(), 2);
    }
}
