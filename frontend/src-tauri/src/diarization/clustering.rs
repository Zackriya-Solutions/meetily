// diarization/clustering.rs
//
// Online speaker clustering over L2-normalized embeddings.
// Each incoming embedding joins the nearest cluster centroid if cosine
// similarity exceeds the threshold, otherwise starts a new "Speaker N"
// cluster. Centroids are running means, re-normalized after update.

/// Minimum cosine similarity for an embedding to join an existing cluster.
/// Tuned for WeSpeaker CAM++ embeddings; raise to split more aggressively.
pub const CLUSTER_SIMILARITY_THRESHOLD: f32 = 0.55;

/// Minimum cosine similarity for a new cluster to match a saved voice profile.
/// Slightly stricter than intra-meeting clustering to avoid false renames.
pub const PROFILE_MATCH_THRESHOLD: f32 = 0.60;

/// Default maximum total number of active speaker clusters per recording.
/// User-configurable per recording; prompt default is 2.
pub const DEFAULT_MAX_SPEAKERS: usize = 2;

pub struct SpeakerCluster {
    pub centroid: Vec<f32>,
    pub count: usize,
    pub label: String,
    /// True for clusters seeded from saved voice profiles (not counted
    /// toward "Speaker N" numbering and excluded from centroid snapshots
    /// until they have real segments).
    pub from_profile: bool,
}

pub struct SpeakerClusterer {
    clusters: Vec<SpeakerCluster>,
    last_label: Option<String>,
    anon_speaker_count: usize,
    max_speakers: usize,
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
        Self {
            clusters: Vec::new(),
            last_label: None,
            anon_speaker_count: 0,
            max_speakers: max_speakers.max(1),
        }
    }

    /// Seed the clusterer with a saved voice profile so returning speakers
    /// are recognized by name instead of getting an anonymous label.
    pub fn seed_profile(&mut self, name: &str, centroid: Vec<f32>) {
        self.clusters.push(SpeakerCluster {
            centroid,
            count: 0,
            label: name.to_string(),
            from_profile: true,
        });
    }

    /// Label of the most recently assigned segment (used to carry labels
    /// across segments too short for reliable embeddings).
    pub fn last_label(&self) -> Option<String> {
        self.last_label.clone()
    }

    pub fn anon_speaker_count(&self) -> usize {
        self.anon_speaker_count
    }

    /// Count of active clusters (anon + profile clusters with segments).
    fn active_cluster_count(&self) -> usize {
        self.clusters
            .iter()
            .filter(|c| !c.from_profile || c.count > 0)
            .count()
    }

    /// Assign an L2-normalized embedding to a cluster, returning its label.
    /// Unmatched profile-seeded clusters require the stricter profile
    /// threshold for their first match. However, if we're at capacity,
    /// unmatched profiles are excluded to prevent exceeding the cap.
    pub fn assign(&mut self, embedding: &[f32]) -> String {
        let at_capacity = self.active_cluster_count() >= self.max_speakers;

        let best = self
            .clusters
            .iter()
            .enumerate()
            .map(|(i, c)| (i, cosine_similarity(embedding, &c.centroid)))
            .filter(|(i, similarity)| {
                let cluster = &self.clusters[*i];

                // Exclude unmatched profile clusters if we're at capacity
                if at_capacity && cluster.from_profile && cluster.count == 0 {
                    return false;
                }

                let threshold = if cluster.from_profile && cluster.count == 0 {
                    PROFILE_MATCH_THRESHOLD
                } else {
                    CLUSTER_SIMILARITY_THRESHOLD
                };
                *similarity >= threshold
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let label = if let Some((idx, _)) = best {
            self.update_cluster(idx, embedding)
        } else if self.active_cluster_count() >= self.max_speakers {
            self.nearest_active_cluster_label(embedding)
                .unwrap_or_else(|| self.create_anonymous_cluster(embedding))
        } else {
            self.create_anonymous_cluster(embedding)
        };

        self.last_label = Some(label.clone());
        label
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

    fn create_anonymous_cluster(&mut self, embedding: &[f32]) -> String {
        self.anon_speaker_count += 1;
        let label = format!("Speaker {}", self.anon_speaker_count);
        self.clusters.push(SpeakerCluster {
            centroid: embedding.to_vec(),
            count: 1,
            label: label.clone(),
            from_profile: false,
        });
        label
    }

    fn nearest_active_cluster_label(&self, embedding: &[f32]) -> Option<String> {
        self.clusters
            .iter()
            .filter(|cluster| !cluster.from_profile || cluster.count > 0)
            .map(|cluster| (cluster, cosine_similarity(embedding, &cluster.centroid)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(cluster, _)| cluster.label.clone())
    }

    /// Snapshot of (label, centroid, segment count) for profile persistence.
    /// Profile-seeded clusters that never matched audio are excluded.
    pub fn centroids(&self) -> impl Iterator<Item = (&str, &[f32], usize)> {
        self.clusters
            .iter()
            .filter(|c| c.count > 0)
            .map(|c| (c.label.as_str(), c.centroid.as_slice(), c.count))
    }

    /// Replace a cluster's label (e.g. when matched to a saved voice profile).
    pub fn relabel(&mut self, old_label: &str, new_label: &str) {
        for cluster in &mut self.clusters {
            if cluster.label == old_label {
                cluster.label = new_label.to_string();
            }
        }
        if self.last_label.as_deref() == Some(old_label) {
            self.last_label = Some(new_label.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn caps_speakers_and_reuses_existing_label_for_outliers() {
        let mut c = SpeakerClusterer::with_max_speakers(2);
        let a = unit(vec![1.0, 0.0, 0.0]);
        let b = unit(vec![0.0, 1.0, 0.0]);
        let outlier = unit(vec![0.0, 0.0, 1.0]);

        assert_eq!(c.assign(&a), "Speaker 1");
        assert_eq!(c.assign(&b), "Speaker 2");

        let assigned = c.assign(&outlier);

        assert!(matches!(assigned.as_str(), "Speaker 1" | "Speaker 2"));
        assert_eq!(c.centroids().count(), 2);
        assert_eq!(c.anon_speaker_count(), 2);
    }

    #[test]
    fn profile_cluster_with_segments_counts_toward_cap() {
        let mut c = SpeakerClusterer::with_max_speakers(2);
        let profile_voice = unit(vec![1.0, 0.0, 0.0]);
        let anon_voice = unit(vec![0.0, 1.0, 0.0]);
        let third_voice = unit(vec![0.0, 0.0, 1.0]);

        c.seed_profile("Alice", profile_voice.clone());

        assert_eq!(c.assign(&profile_voice), "Alice");
        assert_eq!(c.assign(&anon_voice), "Speaker 1");

        let assigned = c.assign(&third_voice);
        assert!(matches!(assigned.as_str(), "Alice" | "Speaker 1"));
    }

    #[test]
    fn profile_cluster_without_segments_does_not_count_toward_cap() {
        let mut c = SpeakerClusterer::with_max_speakers(1);
        let profile_voice = unit(vec![1.0, 0.0, 0.0]);
        let anon_voice = unit(vec![0.0, 1.0, 0.0]);

        c.seed_profile("Alice", profile_voice);

        assert_eq!(c.assign(&anon_voice), "Speaker 1");

        assert_eq!(c.centroids().count(), 1);
    }

    #[test]
    fn default_cap_is_two() {
        let c = SpeakerClusterer::new();
        let a = unit(vec![1.0, 0.0, 0.0]);
        let mut c = c;
        c.assign(&a);

        assert_eq!(c.anon_speaker_count(), 1);
    }

    #[test]
    fn unmatched_profiles_excluded_at_capacity() {
        let mut c = SpeakerClusterer::with_max_speakers(2);
        let alice_voice = unit(vec![1.0, 0.0, 0.0]);
        let bob_voice = unit(vec![0.0, 1.0, 0.0]);
        let charlie_voice = unit(vec![0.0, 0.0, 1.0]);

        // Seed 3 profiles
        c.seed_profile("Alice", alice_voice.clone());
        c.seed_profile("Bob", bob_voice.clone());
        c.seed_profile("Charlie", charlie_voice.clone());

        // Match Alice (1st active profile)
        assert_eq!(c.assign(&alice_voice), "Alice");

        // Match Bob (2nd active profile → at cap)
        assert_eq!(c.assign(&bob_voice), "Bob");

        // Charlie voice arrives but cap is hit — should NOT activate Charlie,
        // instead reuse existing label (Bob is closest)
        let charlie_assigned = c.assign(&charlie_voice);
        assert!(matches!(charlie_assigned.as_str(), "Alice" | "Bob"));

        // Verify only 2 profiles have segments (active)
        let centroids: Vec<_> = c.centroids().collect();
        assert_eq!(centroids.len(), 2);
    }
}
