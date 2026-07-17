// diarization/clustering_offline.rs
//
// Agglomerative hierarchical clustering (AHC) with average-linkage and cosine distance.
// Merges clusters until max_clusters is reached OR min inter-cluster distance < threshold.
// Input: L2-normalized embeddings. Output: cluster ID per embedding.

/// Cosine distance threshold for merging (distance = 1 - cosine_similarity).
/// Similarity >= 0.7 = distance <= 0.3. Reasonable default for speaker embeddings.
pub const AHC_DISTANCE_THRESHOLD: f32 = 0.3;

/// Compute cosine similarity between two L2-normalized vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Compute cosine distance (1 - similarity).
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    1.0 - cosine_similarity(a, b)
}

/// Cluster representative (centroid of all members).
#[derive(Debug, Clone)]
struct ClusterRep {
    centroid: Vec<f32>,
    members: Vec<usize>, // indices of embeddings in this cluster
}

impl ClusterRep {
    fn new(embedding: Vec<f32>, member_idx: usize) -> Self {
        Self {
            centroid: embedding,
            members: vec![member_idx],
        }
    }

    /// Merge another cluster into this one (average-linkage: mean of all members).
    fn merge(&mut self, other: ClusterRep) {
        let n1 = self.members.len() as f32;
        let n2 = other.members.len() as f32;
        let n_total = n1 + n2;

        for (c, e) in self.centroid.iter_mut().zip(other.centroid.iter()) {
            *c = (*c * n1 + e * n2) / n_total;
        }

        let norm: f32 = self.centroid.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for c in &mut self.centroid {
                *c /= norm;
            }
        }

        self.members.extend(other.members);
    }
}

/// Agglomerative hierarchical clustering with dual stopping criteria.
///
/// Merges the two closest clusters while:
/// - num_clusters > max_clusters, OR
/// - num_clusters > 1 AND min_distance < distance_threshold
///
/// Returns cluster assignment per embedding (0-indexed cluster ID).
pub fn cluster(
    embeddings: &[Vec<f32>],
    max_clusters: usize,
    distance_threshold: f32,
) -> Vec<usize> {
    if embeddings.is_empty() {
        return Vec::new();
    }

    if embeddings.len() == 1 {
        return vec![0];
    }

    // Initialize: each embedding is its own cluster
    let mut clusters: Vec<ClusterRep> = embeddings
        .iter()
        .enumerate()
        .map(|(i, emb)| ClusterRep::new(emb.clone(), i))
        .collect();

    // Merge until stopping criteria met
    loop {
        let num_clusters = clusters.len();

        // Stopping criteria: num_clusters at target OR threshold reached (and num_clusters > 1)
        if num_clusters <= max_clusters {
            // Compute min distance to see if we should stop
            if num_clusters <= 1 {
                break; // No more merges possible
            }
            let min_dist = find_min_distance(&clusters);
            if min_dist >= distance_threshold {
                break; // No pair is close enough
            }
        }

        // Find the closest pair of clusters
        let (i, j, _dist) = find_closest_pair(&clusters)
            .expect("Clusters should have >= 2 members for merging");

        // Merge cluster j into cluster i
        let merged = clusters.remove(j);
        clusters[i].merge(merged);
    }

    // Map original embedding indices back to final cluster IDs
    let mut assignment = vec![0usize; embeddings.len()];
    for (cluster_id, cluster) in clusters.iter().enumerate() {
        for &member_idx in &cluster.members {
            assignment[member_idx] = cluster_id;
        }
    }

    assignment
}

/// Find the closest pair of clusters by average-linkage (mean pairwise cosine distance).
fn find_closest_pair(clusters: &[ClusterRep]) -> Option<(usize, usize, f32)> {
    let mut best_i = 0;
    let mut best_j = 1;
    let mut best_distance = f32::INFINITY;

    for i in 0..clusters.len() {
        for j in (i + 1)..clusters.len() {
            let distance = cosine_distance(&clusters[i].centroid, &clusters[j].centroid);
            if distance < best_distance {
                best_distance = distance;
                best_i = i;
                best_j = j;
            }
        }
    }

    if best_distance == f32::INFINITY {
        None
    } else {
        Some((best_i, best_j, best_distance))
    }
}

/// Find minimum inter-cluster distance (for stopping criteria).
fn find_min_distance(clusters: &[ClusterRep]) -> f32 {
    let mut min_dist = f32::INFINITY;
    for i in 0..clusters.len() {
        for j in (i + 1)..clusters.len() {
            let distance = cosine_distance(&clusters[i].centroid, &clusters[j].centroid);
            min_dist = min_dist.min(distance);
        }
    }
    min_dist
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_vector(v: Vec<f32>) -> Vec<f32> {
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            v.into_iter().map(|x| x / norm).collect()
        } else {
            v
        }
    }

    #[test]
    fn cluster_two_distinct_voices() {
        // Two clearly separate clusters
        let embeddings = vec![
            unit_vector(vec![1.0, 0.0, 0.0]),
            unit_vector(vec![0.9, 0.1, 0.0]),
            unit_vector(vec![0.0, 1.0, 0.0]),
            unit_vector(vec![0.0, 0.9, 0.1]),
        ];

        let assignment = cluster(&embeddings, 2, AHC_DISTANCE_THRESHOLD);
        let mut clusters_found = std::collections::HashSet::new();
        for &cluster_id in &assignment {
            clusters_found.insert(cluster_id);
        }
        assert_eq!(clusters_found.len(), 2, "Should form 2 clusters");
    }

    #[test]
    fn cluster_max_limit_respected() {
        // Three similar embeddings, max_clusters=2 should merge one pair
        let embeddings = vec![
            unit_vector(vec![1.0, 0.0]),
            unit_vector(vec![0.95, 0.05]),
            unit_vector(vec![0.94, 0.06]),
        ];

        let assignment = cluster(&embeddings, 2, 0.1); // threshold high (hard to merge)
        let mut clusters_found = std::collections::HashSet::new();
        for &cluster_id in &assignment {
            clusters_found.insert(cluster_id);
        }
        assert!(
            clusters_found.len() <= 2,
            "Should not exceed max_clusters=2; got {}",
            clusters_found.len()
        );
    }

    #[test]
    fn single_embedding_returns_single_cluster() {
        let embeddings = vec![unit_vector(vec![1.0, 0.0, 0.0])];
        let assignment = cluster(&embeddings, 5, 0.3);
        assert_eq!(assignment, vec![0]);
    }

    #[test]
    fn empty_input_returns_empty() {
        let embeddings: Vec<Vec<f32>> = vec![];
        let assignment = cluster(&embeddings, 2, 0.3);
        assert!(assignment.is_empty());
    }

    #[test]
    fn cosine_similarity_normalized_vectors() {
        let a = unit_vector(vec![1.0, 0.0]);
        let b = unit_vector(vec![0.0, 1.0]);
        assert!(cosine_similarity(&a, &b).abs() < 0.01, "Orthogonal vectors");

        let c = unit_vector(vec![1.0, 0.0]);
        assert!((cosine_similarity(&a, &c) - 1.0).abs() < 0.01, "Identical vectors");
    }
}
