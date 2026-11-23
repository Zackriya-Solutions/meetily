// diarization/session.rs
//
// Per-recording diarization state: embedding extractor + online clusterer.
// Created when a recording starts and dropped when it ends.

use super::clustering::{change_windows, SpeakerClusterer};
use super::embedding::{EmbeddingError, EmbeddingExtractor};
use std::path::Path;

/// Shortest segment whose embedding is trusted: below about a second the
/// CAM++ embedding of one voice scatters too much to cluster.
const MIN_SAMPLES_FOR_EMBEDDING: usize = 16_000;
/// Shortest segment allowed to start a new speaker (2.5 s). An "Uh" or
/// "Mike." under that scatters enough to look like a stranger: in a test
/// call with 3 remote people, lines of 1 to 2 s minted 3 phantom speakers.
/// A short line may only join a voice already heard. The cost is that a
/// new person's first line, if short, carries the previous label.
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
    /// short to embed, or one whose embedding fails, carries the previous
    /// label (None before the first one): diarization never fails
    /// transcription. `split_piece` marks a piece cut at a voice change: the
    /// cut already says the voice differs, so a short piece may still start
    /// a new speaker.
    pub fn label_segment(&mut self, samples_16k: &[f32], split_piece: bool) -> Option<String> {
        if samples_16k.len() < MIN_SAMPLES_FOR_EMBEDDING {
            return self.clusterer.last_label();
        }
        match self.extractor.compute(samples_16k) {
            Ok(embedding) if !split_piece && samples_16k.len() < MIN_SAMPLES_FOR_NEW_SPEAKER => self
                .clusterer
                .match_existing(&embedding)
                .or_else(|| self.clusterer.last_label()),
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
        let mut embeddings = Vec::new();
        let mut start = 0;
        while start + CHANGE_WINDOW_SAMPLES <= samples_16k.len() {
            match self.extractor.compute(&samples_16k[start..start + CHANGE_WINDOW_SAMPLES]) {
                Ok(embedding) => embeddings.push(embedding),
                Err(e) => {
                    log::warn!("Diarization window embedding failed, not splitting: {}", e);
                    return Vec::new();
                }
            }
            start += CHANGE_HOP_SAMPLES;
        }
        // The window that starts a change is mostly the new voice and the one
        // before it mostly the old, so the turn sits in the first hop of the
        // new window. Cut at the quietest moment there, the likeliest gap
        // between words, so neither line gets half a word.
        change_windows(&embeddings, CHANGE_SIMILARITY_THRESHOLD)
            .into_iter()
            .map(|i| quietest_frame_center(samples_16k, i * CHANGE_HOP_SAMPLES, CHANGE_HOP_SAMPLES))
            .collect()
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
    /// Starts loading the model. `MEETILY_DIARIZATION=off` turns it off.
    pub fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Self {
        let live = Self::default();
        if std::env::var("MEETILY_DIARIZATION").as_deref() == Ok("off") {
            log::info!("Speaker diarization off: MEETILY_DIARIZATION=off");
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
    pub async fn label(&self, samples_16k: Vec<f32>, split_piece: bool) -> Option<String> {
        let slot = self.session.clone();
        tokio::task::spawn_blocking(move || {
            slot.lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
                .and_then(|session| session.label_segment(&samples_16k, split_piece))
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
