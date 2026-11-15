//! Speaker assignment from voiceprints.
//!
//! * [`OnlineSpeakerClusterer`] labels live transcript chunks one at a time.
//!   Clusters are kept per audio source (microphone vs. system) when system
//!   audio is captured, and the first microphone voice is labelled `me`.
//! * [`cluster_offline_with_threshold`] labels a whole recording at once; batch
//!   import and re-transcription use it on short windows (see `segmentation`).
//!
//! Speaker ids are stable strings stored in `transcripts.speaker`:
//! `me`, `others` (source-only fallback) or `speaker_<n>`. Display names are
//! resolved later from `meeting_speakers`, so renames never rewrite rows.

use super::embedding::cosine;

pub const SPEAKER_ME: &str = "me";
pub const SPEAKER_OTHERS: &str = "others";

// Thresholds for the 3D-Speaker CAM++ model. Calibrated on synthetic speech
// (same voice >= 0.87, distinct voices <= 0.63); erring high because splitting
// one person into two speakers can be fixed by merging, but a merge cannot.
/// Same-speaker cosine threshold for joining an existing cluster.
const MATCH_THRESHOLD: f32 = 0.62;
/// Below `MATCH_THRESHOLD` a short chunk may still join its nearest cluster above this.
const WEAK_MATCH_THRESHOLD: f32 = 0.4;
/// A new speaker is only created from at least this much speech.
const MIN_NEW_SPEAKER_SECONDS: f32 = 1.5;
/// Cap on distinct speakers per meeting; beyond this chunks join the nearest one.
const MAX_SPEAKERS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerSource {
    Microphone,
    System,
}

pub fn speaker_number_id(n: usize) -> String {
    format!("speaker_{n}")
}

struct Cluster {
    id: String,
    source: SpeakerSource,
    sum: Vec<f32>,
    centroid: Vec<f32>,
}

impl Cluster {
    fn add(&mut self, embedding: &[f32], weight: f32) {
        for (s, e) in self.sum.iter_mut().zip(embedding) {
            *s += e * weight;
        }
        self.centroid = self.sum.clone();
        super::embedding::l2_normalize(&mut self.centroid);
    }
}

pub struct OnlineSpeakerClusterer {
    /// True when system audio is captured, so the mic/system split is meaningful.
    source_aware: bool,
    clusters: Vec<Cluster>,
    next_number: usize,
    last_mic: Option<String>,
    last_system: Option<String>,
}

impl OnlineSpeakerClusterer {
    pub fn new(source_aware: bool) -> Self {
        Self {
            source_aware,
            clusters: Vec::new(),
            next_number: 1,
            last_mic: None,
            last_system: None,
        }
    }

    fn remember(&mut self, source: SpeakerSource, id: &str) {
        let slot = match source {
            SpeakerSource::Microphone => &mut self.last_mic,
            SpeakerSource::System => &mut self.last_system,
        };
        *slot = Some(id.to_string());
    }

    fn last_for(&self, source: SpeakerSource) -> Option<String> {
        match source {
            SpeakerSource::Microphone => self.last_mic.clone(),
            SpeakerSource::System => self.last_system.clone(),
        }
    }

    fn new_cluster_id(&mut self, source: SpeakerSource) -> String {
        let me_taken = self.clusters.iter().any(|c| c.id == SPEAKER_ME);
        if self.source_aware && source == SpeakerSource::Microphone && !me_taken {
            return SPEAKER_ME.to_string();
        }
        let id = speaker_number_id(self.next_number);
        self.next_number += 1;
        id
    }

    /// Assigns a speaker id to one transcript chunk.
    ///
    /// `embedding` is `None` when no voiceprint model is installed or the
    /// chunk is too short to embed; then only the source (and continuity with
    /// the previous chunk from that source) decides.
    pub fn assign(
        &mut self,
        embedding: Option<&[f32]>,
        source: SpeakerSource,
        duration_secs: f32,
    ) -> Option<String> {
        // Without system audio every chunk is "microphone"; ignore the split.
        let source = if self.source_aware { source } else { SpeakerSource::Microphone };

        let Some(embedding) = embedding else {
            let id = self.last_for(source).or_else(|| {
                if !self.source_aware {
                    return None;
                }
                Some(match source {
                    SpeakerSource::Microphone => SPEAKER_ME.to_string(),
                    SpeakerSource::System => SPEAKER_OTHERS.to_string(),
                })
            })?;
            self.remember(source, &id);
            return Some(id);
        };

        let best = self
            .clusters
            .iter()
            .enumerate()
            .filter(|(_, c)| c.source == source)
            .map(|(i, c)| (i, cosine(&c.centroid, embedding)))
            .max_by(|a, b| a.1.total_cmp(&b.1));

        let can_create = duration_secs >= MIN_NEW_SPEAKER_SECONDS && self.clusters.len() < MAX_SPEAKERS;
        let index = match best {
            Some((i, sim)) if sim >= MATCH_THRESHOLD => {
                self.clusters[i].add(embedding, duration_secs.min(10.0));
                Some(i)
            }
            _ if can_create => {
                let id = self.new_cluster_id(source);
                let mut cluster = Cluster {
                    id,
                    source,
                    sum: vec![0.0; embedding.len()],
                    centroid: Vec::new(),
                };
                cluster.add(embedding, duration_secs.min(10.0));
                self.clusters.push(cluster);
                Some(self.clusters.len() - 1)
            }
            Some((i, sim)) if sim >= WEAK_MATCH_THRESHOLD || self.clusters.len() >= MAX_SPEAKERS => Some(i),
            _ => None,
        };

        let id = match index {
            Some(i) => self.clusters[i].id.clone(),
            // Too short to found a speaker and unlike everyone: keep continuity.
            None => self.last_for(source).or_else(|| {
                self.source_aware.then(|| match source {
                    SpeakerSource::Microphone => SPEAKER_ME.to_string(),
                    SpeakerSource::System => SPEAKER_OTHERS.to_string(),
                })
            })?,
        };
        self.remember(source, &id);
        Some(id)
    }
}

/// Labels a full recording. Each item is an optional voiceprint (in time
/// order). Items without a voiceprint inherit the nearest labelled neighbour.
/// Speakers are numbered by first appearance.
pub fn cluster_offline_with_threshold(embeddings: &[Option<Vec<f32>>], threshold: f32) -> Vec<Option<String>> {
    let indexed: Vec<(usize, &[f32])> = embeddings
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.as_deref().map(|e| (i, e)))
        .collect();
    if indexed.is_empty() {
        return vec![None; embeddings.len()];
    }
    let vectors: Vec<&[f32]> = indexed.iter().map(|(_, e)| *e).collect();
    let cluster_of = cluster_embeddings(&vectors, threshold);

    // Number clusters by first appearance in time.
    let mut labels: Vec<Option<String>> = vec![None; embeddings.len()];
    let mut numbering: std::collections::HashMap<usize, usize> = Default::default();
    for (k, &(segment, _)) in indexed.iter().enumerate() {
        let next = numbering.len() + 1;
        let number = *numbering.entry(cluster_of[k]).or_insert(next);
        labels[segment] = Some(speaker_number_id(number));
    }

    // Fill unlabelled segments from the nearest labelled neighbour (prefer previous).
    for i in 0..labels.len() {
        if labels[i].is_some() {
            continue;
        }
        let prev = (0..i).rev().find_map(|j| labels[j].clone().map(|l| (i - j, l)));
        let next = ((i + 1)..labels.len()).find_map(|j| labels[j].clone().map(|l| (j - i, l)));
        labels[i] = match (prev, next) {
            (Some((dp, lp)), Some((dn, ln))) => Some(if dn < dp { ln } else { lp }),
            (Some((_, l)), None) | (None, Some((_, l))) => Some(l),
            (None, None) => None,
        };
    }
    labels
}

struct Centroid {
    sum: Vec<f32>,
    unit: Vec<f32>,
    count: usize,
}

impl Centroid {
    fn new(dim: usize) -> Self {
        Self { sum: vec![0.0; dim], unit: vec![0.0; dim], count: 0 }
    }
    fn add(&mut self, v: &[f32]) {
        self.sum.iter_mut().zip(v).for_each(|(s, x)| *s += x);
        self.count += 1;
    }
    fn finish(&mut self) {
        self.unit = self.sum.clone();
        super::embedding::l2_normalize(&mut self.unit);
    }
}

fn nearest(centroids: &[Centroid], v: &[f32]) -> Option<(usize, f32)> {
    centroids
        .iter()
        .enumerate()
        .map(|(i, c)| (i, cosine(&c.unit, v)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Scalable clustering: leader pass, centroid merging, then k-means refinement.
/// O(n·k) instead of O(n³), so an hour of short windows stays fast.
/// Returns a cluster index per vector.
fn cluster_embeddings(vectors: &[&[f32]], threshold: f32) -> Vec<usize> {
    let dim = vectors[0].len();

    // 1. Leader clustering against running centroids. Capped so noisy windows in
    // a long recording cannot blow up the (cubic in k) merge step below.
    const MAX_LEADER_CLUSTERS: usize = 64;
    let mut centroids: Vec<Centroid> = Vec::new();
    for v in vectors {
        match nearest(&centroids, v) {
            Some((i, sim)) if sim >= threshold || centroids.len() >= MAX_LEADER_CLUSTERS => {
                centroids[i].add(v);
                centroids[i].finish();
            }
            _ => {
                let mut c = Centroid::new(dim);
                c.add(v);
                c.finish();
                centroids.push(c);
            }
        }
    }

    // 2. Merge centroids that describe the same voice (k is small here).
    loop {
        let mut best: Option<(usize, usize, f32)> = None;
        for a in 0..centroids.len() {
            for b in (a + 1)..centroids.len() {
                let sim = cosine(&centroids[a].unit, &centroids[b].unit);
                if best.map_or(true, |(_, _, s)| sim > s) {
                    best = Some((a, b, sim));
                }
            }
        }
        match best {
            Some((a, b, sim)) if sim >= threshold => {
                let merged = centroids.swap_remove(b);
                centroids[a].sum.iter_mut().zip(&merged.sum).for_each(|(s, x)| *s += x);
                centroids[a].count += merged.count;
                centroids[a].finish();
            }
            _ => break,
        }
    }

    // 3. Refine: reassign every vector to its nearest centroid, recompute, repeat.
    let mut assignment = vec![0usize; vectors.len()];
    for _ in 0..5 {
        for (i, v) in vectors.iter().enumerate() {
            assignment[i] = nearest(&centroids, v).map(|(c, _)| c).unwrap_or(0);
        }
        let mut next: Vec<Centroid> = (0..centroids.len()).map(|_| Centroid::new(dim)).collect();
        for (i, v) in vectors.iter().enumerate() {
            next[assignment[i]].add(v);
        }
        // Drop empty clusters, and fold single-vector clusters into a neighbour when
        // one is reasonably close (isolated outliers are usually noise, not a speaker).
        let mut keep: Vec<Centroid> = next.into_iter().filter(|c| c.count > 0).collect();
        keep.iter_mut().for_each(Centroid::finish);
        if keep.len() > 1 {
            let singletons: Vec<usize> = (0..keep.len()).filter(|&i| keep[i].count == 1).collect();
            let mut removed = vec![false; keep.len()];
            for i in singletons {
                let best_other = (0..keep.len())
                    .filter(|&j| j != i && !removed[j])
                    .map(|j| (j, cosine(&keep[i].unit, &keep[j].unit)))
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                if matches!(best_other, Some((_, sim)) if sim >= WEAK_MATCH_THRESHOLD) {
                    removed[i] = true;
                }
            }
            keep = keep
                .into_iter()
                .zip(removed)
                .filter_map(|(c, r)| (!r).then_some(c))
                .collect();
        }
        centroids = keep;
    }
    for (i, v) in vectors.iter().enumerate() {
        assignment[i] = nearest(&centroids, v).map(|(c, _)| c).unwrap_or(0);
    }
    assignment
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(axis: usize, jitter: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; 8];
        v[axis] = 1.0;
        v[(axis + 1) % 8] = jitter;
        super::super::embedding::l2_normalize(&mut v);
        v
    }

    #[test]
    fn online_separates_voices_and_labels_first_mic_voice_me() {
        let mut c = OnlineSpeakerClusterer::new(true);
        let mic = SpeakerSource::Microphone;
        let sys = SpeakerSource::System;
        assert_eq!(c.assign(Some(&voice(0, 0.1)), mic, 3.0).as_deref(), Some("me"));
        assert_eq!(c.assign(Some(&voice(2, 0.1)), sys, 3.0).as_deref(), Some("speaker_1"));
        assert_eq!(c.assign(Some(&voice(4, 0.1)), sys, 3.0).as_deref(), Some("speaker_2"));
        assert_eq!(c.assign(Some(&voice(2, 0.2)), sys, 2.0).as_deref(), Some("speaker_1"));
        assert_eq!(c.assign(Some(&voice(0, 0.2)), mic, 2.0).as_deref(), Some("me"));
        // A second in-room voice on the microphone becomes a numbered speaker.
        assert_eq!(c.assign(Some(&voice(6, 0.1)), mic, 3.0).as_deref(), Some("speaker_3"));
    }

    #[test]
    fn online_short_unknown_chunk_keeps_continuity() {
        let mut c = OnlineSpeakerClusterer::new(true);
        let sys = SpeakerSource::System;
        assert_eq!(c.assign(Some(&voice(2, 0.1)), sys, 3.0).as_deref(), Some("speaker_1"));
        // Too short to create a new speaker and dissimilar: stays with last system speaker.
        assert_eq!(c.assign(Some(&voice(5, 0.0)), sys, 0.8).as_deref(), Some("speaker_1"));
    }

    #[test]
    fn online_without_model_falls_back_to_source() {
        let mut c = OnlineSpeakerClusterer::new(true);
        assert_eq!(c.assign(None, SpeakerSource::Microphone, 2.0).as_deref(), Some("me"));
        assert_eq!(c.assign(None, SpeakerSource::System, 2.0).as_deref(), Some("others"));

        let mut room = OnlineSpeakerClusterer::new(false);
        assert_eq!(room.assign(None, SpeakerSource::Microphone, 2.0), None);
    }

    #[test]
    fn online_without_system_audio_never_uses_me() {
        let mut c = OnlineSpeakerClusterer::new(false);
        assert_eq!(c.assign(Some(&voice(0, 0.1)), SpeakerSource::Microphone, 3.0).as_deref(), Some("speaker_1"));
        assert_eq!(c.assign(Some(&voice(3, 0.1)), SpeakerSource::Microphone, 3.0).as_deref(), Some("speaker_2"));
    }

    #[test]
    fn offline_clustering_scales_to_an_hour_of_windows() {
        // ~1 hour of 0.5 s-hop windows from 4 voices with deterministic jitter.
        let n = 7200;
        let embeddings: Vec<Option<Vec<f32>>> = (0..n)
            .map(|i| {
                let jitter = ((i * 7919) % 100) as f32 / 400.0;
                Some(voice((i / 300) % 4 * 2, jitter))
            })
            .collect();
        let started = std::time::Instant::now();
        let labels = cluster_offline_with_threshold(&embeddings, 0.55);
        let distinct: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(distinct.len(), 4);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "took {:?}", started.elapsed());
    }

    #[test]
    fn offline_groups_by_voice_and_fills_gaps() {
        let embeddings = vec![
            Some(voice(1, 0.1)),
            Some(voice(5, 0.1)),
            None,
            Some(voice(1, 0.2)),
            Some(voice(5, 0.15)),
        ];
        let labels = cluster_offline_with_threshold(&embeddings, 0.62);
        assert_eq!(
            labels,
            vec![
                Some("speaker_1".into()),
                Some("speaker_2".into()),
                Some("speaker_2".into()),
                Some("speaker_1".into()),
                Some("speaker_2".into()),
            ]
        );
        assert_eq!(cluster_offline_with_threshold(&[None, None], 0.62), vec![None, None]);
    }
}
