// diarization/session.rs
//
// Per-recording diarization state: embedding extractor + online clusterer.
// Created when a recording starts and dropped when it ends.

use super::clustering::{change_windows, cosine_similarity, SpeakerClusterer, CLUSTER_SIMILARITY_THRESHOLD};
use super::embedding::{EmbeddingError, EmbeddingExtractor};
use std::path::Path;

/// Shortest segment whose embedding is trusted: below about a second the
/// CAM++ embedding of one voice scatters too much to cluster.
const MIN_SAMPLES_FOR_EMBEDDING: usize = 16_000;
/// Shortest segment allowed to start a new speaker (2.5 s). An "Uh" or
/// "Mike." under that scatters enough to look like a stranger: in a test
/// call with 3 remote people, lines of 1 to 2 s minted 3 phantom speakers.
/// A short line may only join a voice already heard; one that matches no
/// one gets no label rather than the previous speaker's.
const MIN_SAMPLES_FOR_NEW_SPEAKER: usize = 40_000;

/// Voice-change search inside one segment: 1.5 s windows every 0.75 s. A
/// window this short still separates two voices, though not well enough to
/// name them; each piece is labelled whole afterwards.
const CHANGE_WINDOW_SAMPLES: usize = 24_000;
const CHANGE_HOP_SAMPLES: usize = 12_000;
/// Lower than the clustering threshold: a 1.5 s window of the same voice
/// scatters more than a whole segment does.
const CHANGE_SIMILARITY_THRESHOLD: f32 = 0.4;
/// `change_windows` needs four windows to find a change.
const MIN_SAMPLES_FOR_CHANGES: usize = CHANGE_WINDOW_SAMPLES + 3 * CHANGE_HOP_SAMPLES;

pub struct DiarizationSession {
    extractor: EmbeddingExtractor,
    clusterer: SpeakerClusterer,
}

impl DiarizationSession {
    pub fn new(embedding_model_path: &Path) -> Result<Self, EmbeddingError> {
        Ok(Self {
            extractor: EmbeddingExtractor::new(embedding_model_path)?,
            clusterer: SpeakerClusterer::new(),
        })
    }

    /// Assign a speaker label to a 16kHz mono speech segment. A segment too
    /// short to embed (under a second, mostly "So" or "Um" from whoever is
    /// talking), or one whose embedding fails, carries the previous label
    /// (None before the first one): diarization never fails transcription.
    /// Short pieces cut at a voice change follow the same rule: allowing
    /// them to start a speaker minted a phantom from a 2 s piece in testing.
    pub fn label_segment(&mut self, samples_16k: &[f32]) -> Option<String> {
        if samples_16k.len() < MIN_SAMPLES_FOR_EMBEDDING {
            return self.clusterer.last_label();
        }
        match self.extractor.compute(samples_16k) {
            Ok(embedding) if samples_16k.len() < MIN_SAMPLES_FOR_NEW_SPEAKER => {
                self.clusterer.match_existing(&embedding)
            }
            Ok(embedding) => Some(self.clusterer.assign(&embedding)),
            Err(e) => {
                log::warn!("Diarization embedding failed, carrying previous label: {}", e);
                self.clusterer.last_label()
            }
        }
    }

    /// Sample offsets inside a 16kHz segment where a different voice takes
    /// over, for speech with no pause between speakers. Leaves the speaker
    /// clusters alone.
    pub fn speaker_changes(&mut self, samples_16k: &[f32]) -> Vec<usize> {
        if samples_16k.len() < MIN_SAMPLES_FOR_CHANGES {
            return Vec::new();
        }
        let window_count = (samples_16k.len() - CHANGE_WINDOW_SAMPLES) / CHANGE_HOP_SAMPLES + 1;
        let mut embeddings: Vec<Option<Vec<f32>>> = vec![None; window_count];

        // Every other window first: they do not overlap, and when each is
        // like the next the segment is one voice, which is most segments.
        // That skips half the model runs; the in-between windows are only
        // computed when a change is likely.
        for i in (0..window_count).step_by(2) {
            match self.embed_or_none(samples_16k, i) {
                Some(embedding) => embeddings[i] = Some(embedding),
                None => return Vec::new(),
            }
        }
        // Also compare windows two apart: a turn in the middle of a window
        // makes it half of each voice, close enough to both neighbours.
        let coarse: Vec<&Vec<f32>> = embeddings.iter().step_by(2).flatten().collect();
        let departs = |a: usize, b: usize| {
            b < coarse.len() && cosine_similarity(coarse[a], coarse[b]) < CHANGE_SIMILARITY_THRESHOLD
        };
        let any_departure = (0..coarse.len()).any(|k| departs(k, k + 1) || departs(k, k + 2));
        if !any_departure {
            return Vec::new();
        }
        for i in (1..window_count).step_by(2) {
            match self.embed_or_none(samples_16k, i) {
                Some(embedding) => embeddings[i] = Some(embedding),
                None => return Vec::new(),
            }
        }
        let embeddings: Vec<Vec<f32>> = embeddings.into_iter().flatten().collect();

        // The window that starts a change is mostly the new voice and the one
        // before it mostly the old, so the turn sits in the first hop of the
        // new window. Cut at the quietest moment there, the likeliest gap
        // between words, so neither line gets half a word.
        let cuts: Vec<usize> = change_windows(&embeddings, CHANGE_SIMILARITY_THRESHOLD)
            .into_iter()
            .map(|i| quietest_frame_center(samples_16k, i * CHANGE_HOP_SAMPLES, CHANGE_HOP_SAMPLES))
            .collect();
        self.confirm_cuts(samples_16k, cuts)
    }

    /// Embedding of change window `index`, or None (no split) when the model
    /// fails.
    fn embed_or_none(&mut self, samples_16k: &[f32], index: usize) -> Option<Vec<f32>> {
        let start = index * CHANGE_HOP_SAMPLES;
        self.extractor
            .compute(&samples_16k[start..start + CHANGE_WINDOW_SAMPLES])
            .map_err(|e| log::warn!("Diarization window embedding failed, not splitting: {}", e))
            .ok()
    }

    /// Keeps a cut only when the whole pieces on either side sound like
    /// different people. Short windows of one voice over music or a noisy
    /// screen share can dip below the window threshold; the pieces, several
    /// seconds each, do not.
    fn confirm_cuts(&mut self, samples_16k: &[f32], cuts: Vec<usize>) -> Vec<usize> {
        let mut kept = Vec::new();
        let mut start = 0;
        // The right piece of a kept cut is the next cut's left piece.
        let mut left = None;
        for (i, &cut) in cuts.iter().enumerate() {
            let next = cuts.get(i + 1).copied().unwrap_or(samples_16k.len());
            let left_embedding = match left.take() {
                Some(embedding) => Ok(embedding),
                None => self.extractor.compute(&samples_16k[start..cut]),
            };
            let right = self.extractor.compute(&samples_16k[cut..next]);
            let same_voice = match (&left_embedding, &right) {
                (Ok(l), Ok(r)) => cosine_similarity(l, r) >= CLUSTER_SIMILARITY_THRESHOLD,
                _ => false,
            };
            if !same_voice {
                kept.push(cut);
                start = cut;
                left = right.ok();
            }
        }
        kept
    }
}

/// 20 ms at 16 kHz: short enough to land between two words.
const QUIET_FRAME_SAMPLES: usize = 320;

/// Center of the lowest-energy 20 ms frame in `samples[start..start + len]`.
fn quietest_frame_center(samples: &[f32], start: usize, len: usize) -> usize {
    let end = (start + len).min(samples.len());
    (start..end.saturating_sub(QUIET_FRAME_SAMPLES).max(start + 1))
        .step_by(QUIET_FRAME_SAMPLES / 2)
        .min_by(|&a, &b| {
            let energy = |s: usize| {
                samples[s..(s + QUIET_FRAME_SAMPLES).min(end)].iter().map(|x| x * x).sum::<f32>()
            };
            energy(a).partial_cmp(&energy(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or(start + len / 2, |s| s + QUIET_FRAME_SAMPLES / 2)
}

/// The diarization session of one recording, shared with the transcription
/// worker. The model downloads and loads in the background; segments that
/// arrive before it is ready get no label.
#[derive(Clone, Default)]
pub struct LiveDiarization {
    session: std::sync::Arc<std::sync::Mutex<Option<DiarizationSession>>>,
}

impl LiveDiarization {
    /// Starts loading the model. `OMN_DIARIZATION=off` turns it off.
    pub fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Self {
        let live = Self::default();
        if std::env::var("OMN_DIARIZATION").as_deref() == Ok("off") {
            log::info!("Speaker diarization off: OMN_DIARIZATION=off");
            return live;
        }
        let app = app.clone();
        let slot = live.session.clone();
        tauri::async_runtime::spawn(async move {
            let path = match super::models::ensure_embedding_model(&app).await {
                Ok(path) => path,
                Err(e) => {
                    log::warn!("Speaker diarization unavailable: {}", e);
                    return;
                }
            };
            match tokio::task::spawn_blocking(move || DiarizationSession::new(&path)).await {
                Ok(Ok(session)) => {
                    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(session);
                    log::info!("Speaker diarization ready");
                }
                Ok(Err(e)) => log::warn!("Speaker diarization model failed to load: {}", e),
                Err(e) => log::warn!("Speaker diarization loader panicked: {}", e),
            }
        });
        live
    }

    /// Speaker label for a 16kHz segment, or None while the model is not
    /// ready. Runs the model on a blocking thread.
    pub async fn label(&self, samples_16k: Vec<f32>) -> Option<String> {
        let slot = self.session.clone();
        tokio::task::spawn_blocking(move || {
            slot.lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
                .and_then(|session| session.label_segment(&samples_16k))
        })
        .await
        .ok()
        .flatten()
    }

    /// Where the voice changes inside a 16kHz segment (see
    /// `DiarizationSession::speaker_changes`); empty while the model is not
    /// ready.
    pub async fn speaker_changes(&self, samples_16k: Vec<f32>) -> Vec<usize> {
        let slot = self.session.clone();
        tokio::task::spawn_blocking(move || {
            slot.lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
                .map(|session| session.speaker_changes(&samples_16k))
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_in_the_gap_between_words() {
        let mut samples = vec![0.5f32; 12_000];
        samples[7_000..7_400].iter_mut().for_each(|x| *x = 0.0);
        let cut = quietest_frame_center(&samples, 0, 12_000);
        assert!((7_000..7_400).contains(&cut), "cut at {}", cut);
    }
}
