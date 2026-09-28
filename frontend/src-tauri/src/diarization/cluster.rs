//! Vector math and agglomerative (average-linkage) clustering on cosine similarity.
use std::cmp::Ordering;
use std::collections::HashMap;

/// Above this many embeddings, a time-uniform subset is clustered and the rest join the nearest
/// resulting centroid. Keeps the similarity matrix near 64 MB (roughly 2.5-4 h of speech).
pub const MAX_CLUSTER_POINTS: usize = 4000;

pub fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Cosine similarity; 0 when either vector has zero length or the lengths differ
/// (for example a centroid stored by a different embedding model).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let na = dot(a, a).sqrt();
    let nb = dot(b, b).sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot(a, b) / (na * nb)
    }
}

/// Weighted mean of L2-normalised vectors, re-normalised. Vectors that are not finite after
/// normalisation are skipped, so one NaN embedding cannot poison a centroid.
pub fn weighted_centroid(vectors: &[&[f32]], weights: &[f64]) -> Vec<f32> {
    let dim = vectors.first().map(|v| v.len()).unwrap_or(0);
    let mut acc = vec![0f64; dim];
    for (v, &w) in vectors.iter().zip(weights) {
        let mut n = v.to_vec();
        l2_normalize(&mut n);
        if !n.iter().all(|x| x.is_finite()) {
            continue;
        }
        for (a, x) in acc.iter_mut().zip(&n) {
            *a += w * *x as f64;
        }
    }
    let mut out: Vec<f32> = acc.into_iter().map(|x| x as f32).collect();
    l2_normalize(&mut out);
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClusterStop {
    /// Merge while the closest clusters' average cosine similarity is at least this value.
    Threshold(f32),
    /// Merge until exactly this many clusters remain (clamped to the number of points).
    Count(usize),
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }
    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.0[root] != root {
            root = self.0[root];
        }
        let mut cur = x;
        while self.0[cur] != root {
            let next = self.0[cur];
            self.0[cur] = root;
            cur = next;
        }
        root
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[rb] = ra;
        }
    }
}

/// Average-linkage agglomerative clustering. Returns one label per input, numbered 0.. in order
/// of first occurrence. More than MAX_CLUSTER_POINTS inputs (which arrive in window order) are
/// clustered through a time-uniform subset; the other inputs join the nearest centroid.
pub fn agglomerative(embeddings: &[Vec<f32>], stop: ClusterStop) -> Vec<usize> {
    let n = embeddings.len();
    if n <= MAX_CLUSTER_POINTS {
        return agglomerative_dense(embeddings, stop);
    }
    let idx: Vec<usize> = (0..MAX_CLUSTER_POINTS).map(|i| i * n / MAX_CLUSTER_POINTS).collect();
    let subset: Vec<Vec<f32>> = idx.iter().map(|&i| embeddings[i].clone()).collect();
    let sub_labels = agglomerative_dense(&subset, stop);
    let k = sub_labels.iter().max().map_or(0, |m| m + 1);
    let centroids: Vec<Vec<f32>> = (0..k)
        .map(|c| {
            let members: Vec<&[f32]> = idx
                .iter()
                .zip(&sub_labels)
                .filter(|(_, l)| **l == c)
                .map(|(&i, _)| embeddings[i].as_slice())
                .collect();
            weighted_centroid(&members, &vec![1.0; members.len()])
        })
        .collect();
    let mut in_subset = vec![false; n];
    let mut raw = vec![0usize; n];
    for (&i, &l) in idx.iter().zip(&sub_labels) {
        in_subset[i] = true;
        raw[i] = l;
    }
    for i in 0..n {
        if !in_subset[i] {
            raw[i] = (0..k)
                .max_by(|&a, &b| {
                    cosine(&embeddings[i], &centroids[a])
                        .partial_cmp(&cosine(&embeddings[i], &centroids[b]))
                        .unwrap_or(Ordering::Equal)
                })
                .unwrap_or(0);
        }
    }
    let mut relabel: HashMap<usize, usize> = HashMap::new();
    raw.into_iter()
        .map(|c| {
            let next = relabel.len();
            *relabel.entry(c).or_insert(next)
        })
        .collect()
}

/// Nearest-neighbour chain (O(n²) time and memory); used for up to MAX_CLUSTER_POINTS inputs.
fn agglomerative_dense(embeddings: &[Vec<f32>], stop: ClusterStop) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 {
        return Vec::new();
    }
    let normed: Vec<Vec<f32>> = embeddings
        .iter()
        .map(|e| {
            let mut v = e.clone();
            l2_normalize(&mut v);
            v
        })
        .collect();
    let mut sim = vec![0f32; n * n];
    for i in 0..n {
        for j in (i + 1)..n {
            let s = dot(&normed[i], &normed[j]);
            // A NaN embedding must not break the chain: treat it as maximally dissimilar.
            let s = if s.is_finite() { s } else { -1.0 };
            sim[i * n + j] = s;
            sim[j * n + i] = s;
        }
    }

    let mut size = vec![1usize; n];
    let mut active = vec![true; n];
    let mut merges: Vec<(usize, usize, f32)> = Vec::with_capacity(n - 1);
    let mut chain: Vec<usize> = Vec::new();
    let mut remaining = n;

    while remaining > 1 {
        if chain.is_empty() {
            chain.push((0..n).find(|&i| active[i]).expect("an active cluster"));
        }
        loop {
            let c = *chain.last().expect("non-empty chain");
            let prev = if chain.len() >= 2 { Some(chain[chain.len() - 2]) } else { None };
            let mut best = prev;
            let mut best_sim = prev.map(|p| sim[c * n + p]).unwrap_or(f32::NEG_INFINITY);
            for j in 0..n {
                if j != c && active[j] && sim[c * n + j] > best_sim {
                    best = Some(j);
                    best_sim = sim[c * n + j];
                }
            }
            let best = best.expect("at least two active clusters");
            if Some(best) == prev {
                chain.pop();
                chain.pop();
                let (a, b) = (c, best);
                let (sa, sb) = (size[a] as f32, size[b] as f32);
                for j in 0..n {
                    if active[j] && j != a && j != b {
                        let s = (sa * sim[a * n + j] + sb * sim[b * n + j]) / (sa + sb);
                        sim[a * n + j] = s;
                        sim[j * n + a] = s;
                    }
                }
                size[a] += size[b];
                active[b] = false;
                merges.push((a, b, best_sim));
                remaining -= 1;
                break;
            }
            chain.push(best);
        }
    }

    // Average linkage is monotone, so applying merges from most to least similar
    // reproduces the dendrogram; the merges form a spanning tree over the points,
    // so applying m of them leaves exactly n - m clusters.
    merges.sort_by(|x, y| y.2.partial_cmp(&x.2).unwrap_or(Ordering::Equal));
    let to_apply = match stop {
        ClusterStop::Threshold(t) => merges.iter().take_while(|m| m.2 >= t).count(),
        ClusterStop::Count(k) => n.saturating_sub(k.max(1)),
    };
    let mut uf = UnionFind::new(n);
    for &(a, b, _) in merges.iter().take(to_apply) {
        uf.union(a, b);
    }

    let mut relabel: HashMap<usize, usize> = HashMap::new();
    (0..n)
        .map(|i| {
            let root = uf.find(i);
            let next = relabel.len();
            *relabel.entry(root).or_insert(next)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn around(base: &[f32], jitter: f32, i: usize) -> Vec<f32> {
        base.iter()
            .enumerate()
            .map(|(d, x)| x + jitter * (((i * 7 + d * 3) % 5) as f32 - 2.0))
            .collect()
    }

    fn three_groups() -> Vec<Vec<f32>> {
        let a = [1.0, 0.0, 0.0, 0.0];
        let b = [0.0, 1.0, 0.0, 0.0];
        let c = [0.0, 0.0, 1.0, 0.0];
        let mut v = Vec::new();
        for i in 0..4 {
            v.push(around(&a, 0.05, i));
            v.push(around(&b, 0.05, i));
            v.push(around(&c, 0.05, i));
        }
        v
    }

    #[test]
    fn cosine_of_parallel_and_orthogonal_vectors() {
        assert!((cosine(&[1.0, 2.0], &[2.0, 4.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 3.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn cosine_of_different_lengths_is_zero() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn weighted_centroid_leans_to_heavier_vector_and_is_normalised() {
        let c = weighted_centroid(&[&[1.0, 0.0], &[0.0, 1.0]], &[3.0, 1.0]);
        assert!(c[0] > c[1]);
        let norm = (c[0] * c[0] + c[1] * c[1]).sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn threshold_separates_three_groups() {
        let labels = agglomerative(&three_groups(), ClusterStop::Threshold(0.5));
        assert_eq!(labels.len(), 12);
        assert_eq!(labels.iter().max(), Some(&2));
        for i in 0..4 {
            assert_eq!(labels[i * 3], labels[0]);
            assert_eq!(labels[i * 3 + 1], labels[1]);
            assert_eq!(labels[i * 3 + 2], labels[2]);
        }
        assert_eq!(&labels[0..3], &[0, 1, 2]);
    }

    #[test]
    fn count_forces_exact_number_of_clusters() {
        let labels = agglomerative(&three_groups(), ClusterStop::Count(2));
        let distinct: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(distinct.len(), 2);
        let one = agglomerative(&three_groups(), ClusterStop::Count(1));
        assert!(one.iter().all(|&l| l == 0));
    }

    #[test]
    fn count_larger_than_points_keeps_every_point_separate() {
        let labels = agglomerative(&three_groups()[..3].to_vec(), ClusterStop::Count(10));
        assert_eq!(labels, vec![0, 1, 2]);
    }

    #[test]
    fn strict_threshold_keeps_points_apart_and_empty_input_is_empty() {
        let labels = agglomerative(&three_groups(), ClusterStop::Threshold(0.9999));
        assert_eq!(labels.iter().max(), Some(&11));
        assert!(agglomerative(&[], ClusterStop::Threshold(0.5)).is_empty());
    }

    #[test]
    fn non_finite_embeddings_do_not_panic() {
        let labels = agglomerative(&[vec![f32::NAN, 0.0], vec![1.0, 0.0], vec![0.9, 0.1]], ClusterStop::Threshold(0.5));
        assert_eq!(labels, vec![0, 1, 1], "the NaN point stays in its own cluster");
        let c = weighted_centroid(&[&[f32::NAN, 0.0], &[1.0, 0.0]], &[1.0, 1.0]);
        assert!(c.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn large_inputs_are_clustered_through_a_subset() {
        let bases = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let n = MAX_CLUSTER_POINTS + 500;
        let points: Vec<Vec<f32>> = (0..n).map(|i| around(&bases[i % 3], 0.05, i)).collect();
        for stop in [ClusterStop::Threshold(0.5), ClusterStop::Count(3)] {
            let labels = agglomerative(&points, stop);
            assert_eq!(labels.len(), n);
            assert_eq!(&labels[0..3], &[0, 1, 2], "{stop:?}");
            for (i, &l) in labels.iter().enumerate() {
                assert_eq!(l, labels[i % 3], "{stop:?}: point {i} left its group");
            }
        }
    }
}
