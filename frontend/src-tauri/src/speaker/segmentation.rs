//! Speaker-change segmentation for batch audio (import / re-transcription).
//!
//! Batch VAD segments can run for many seconds and span several speakers, so a
//! label per segment is not enough. Each segment is covered with short
//! overlapping windows, every window is embedded, all windows of the recording
//! are clustered together, and segments are cut where the window label changes.
//! Each resulting span is transcribed separately and keeps its speaker id.

use super::clustering::cluster_offline_with_threshold;

const SAMPLE_RATE: usize = 16_000;
/// Window length for change detection; long enough for a usable voiceprint.
pub const WINDOW_SECONDS: f32 = 1.5;
/// Hop between windows; sets the resolution of speaker-change cut points.
pub const STEP_SECONDS: f32 = 0.5;
/// Spans shorter than this are folded into a neighbour (too short to transcribe well).
pub const MIN_SPAN_SECONDS: f32 = 1.0;
/// Cosine threshold for clustering short windows (noisier than whole segments).
pub const WINDOW_CLUSTER_THRESHOLD: f32 = 0.55;

#[derive(Debug, Clone, PartialEq)]
pub struct LabeledSpan {
    /// Sample offsets within the segment, `[start, end)`.
    pub start: usize,
    pub end: usize,
    pub speaker: Option<String>,
}

/// Window `[start, end)` offsets covering `len` samples. The last window is
/// aligned to the end so the tail is covered.
fn windows(len: usize) -> Vec<(usize, usize)> {
    let win = (WINDOW_SECONDS * SAMPLE_RATE as f32) as usize;
    let step = (STEP_SECONDS * SAMPLE_RATE as f32) as usize;
    if len <= win {
        return vec![(0, len)];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start + win < len {
        out.push((start, start + win));
        start += step;
    }
    out.push((len - win, len));
    out
}

/// Splits each segment into speaker-homogeneous spans.
/// `embed` returns a voiceprint for a window (or `None` if it cannot).
pub fn segment_by_speaker<F>(segments: &[&[f32]], mut embed: F) -> Vec<Vec<LabeledSpan>>
where
    F: FnMut(&[f32]) -> Option<Vec<f32>>,
{
    // Embed every window of every segment.
    let mut window_ranges: Vec<Vec<(usize, usize)>> = Vec::with_capacity(segments.len());
    let mut embeddings: Vec<Option<Vec<f32>>> = Vec::new();
    for segment in segments {
        let ranges = windows(segment.len());
        for &(a, b) in &ranges {
            embeddings.push(embed(&segment[a..b]));
        }
        window_ranges.push(ranges);
    }

    // Cluster all windows of the recording together so ids are global.
    let labels = cluster_offline_with_threshold(&embeddings, WINDOW_CLUSTER_THRESHOLD);

    let min_span = (MIN_SPAN_SECONDS * SAMPLE_RATE as f32) as usize;
    let mut offset = 0;
    let mut result = Vec::with_capacity(segments.len());
    for (segment, ranges) in segments.iter().zip(&window_ranges) {
        let mut window_labels: Vec<Option<String>> = labels[offset..offset + ranges.len()].to_vec();
        offset += ranges.len();

        // Smooth single-window blips (A B A -> A A A).
        for i in 1..window_labels.len().saturating_sub(1) {
            if window_labels[i - 1] == window_labels[i + 1] && window_labels[i] != window_labels[i - 1] {
                window_labels[i] = window_labels[i - 1].clone();
            }
        }

        // Runs of equal labels become spans; cut between the centres of the
        // last window of one run and the first window of the next.
        let mut spans: Vec<LabeledSpan> = Vec::new();
        let mut run_start_sample = 0;
        for i in 0..window_labels.len() {
            let is_last = i + 1 == window_labels.len();
            if is_last || window_labels[i + 1] != window_labels[i] {
                let end = if is_last {
                    segment.len()
                } else {
                    let centre = |(a, b): (usize, usize)| (a + b) / 2;
                    (centre(ranges[i]) + centre(ranges[i + 1])) / 2
                };
                spans.push(LabeledSpan { start: run_start_sample, end, speaker: window_labels[i].clone() });
                run_start_sample = end;
            }
        }

        // Fold spans that are too short to transcribe into a neighbour,
        // and join neighbours that end up with the same speaker.
        let mut merged: Vec<LabeledSpan> = Vec::new();
        for span in spans {
            match merged.last_mut() {
                Some(prev) if prev.speaker == span.speaker || span.end - span.start < min_span => {
                    prev.end = span.end;
                }
                _ => merged.push(span),
            }
        }
        if merged.len() > 1 && merged[0].end - merged[0].start < min_span {
            let first = merged.remove(0);
            merged[0].start = first.start;
        }
        result.push(merged);
    }
    renumber_by_first_appearance(&mut result);
    result
}

/// Clusters that only existed in smoothed-away windows (e.g. silence) leave gaps
/// in the numbering; relabel so speakers read Speaker 1, 2, 3... in order.
fn renumber_by_first_appearance(spans: &mut [Vec<LabeledSpan>]) {
    let mut mapping: std::collections::HashMap<String, String> = Default::default();
    for span in spans.iter_mut().flatten() {
        if let Some(old) = span.speaker.take() {
            let next = super::clustering::speaker_number_id(mapping.len() + 1);
            span.speaker = Some(mapping.entry(old).or_insert(next).clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake "audio" where the sample value encodes the speaker (1.0 or 2.0).
    fn fake_embed(window: &[f32]) -> Option<Vec<f32>> {
        let mean = window.iter().sum::<f32>() / window.len() as f32;
        let mut v = vec![0.0f32; 4];
        if mean < 1.5 {
            v[0] = 1.0;
        } else {
            v[2] = 1.0;
        }
        Some(v)
    }

    fn secs(s: f32) -> usize {
        (s * SAMPLE_RATE as f32) as usize
    }

    #[test]
    fn windows_cover_the_whole_segment() {
        assert_eq!(windows(secs(1.0)), vec![(0, secs(1.0))]);
        let w = windows(secs(3.2));
        assert_eq!(w.first().unwrap().0, 0);
        assert_eq!(w.last().unwrap().1, secs(3.2));
    }

    #[test]
    fn cuts_a_two_speaker_segment_near_the_change() {
        let mut segment = vec![1.0f32; secs(6.0)];
        segment.extend(vec![2.0f32; secs(5.0)]);
        let spans = segment_by_speaker(&[&segment], fake_embed);
        assert_eq!(spans[0].len(), 2, "{:?}", spans[0]);
        let cut = spans[0][0].end as f32 / SAMPLE_RATE as f32;
        assert!((cut - 6.0).abs() <= 0.75, "cut at {cut}s");
        assert_eq!(spans[0][0].speaker.as_deref(), Some("speaker_1"));
        assert_eq!(spans[0][1].speaker.as_deref(), Some("speaker_2"));
        assert_eq!(spans[0][1].end, segment.len());
    }

    #[test]
    fn ids_are_consistent_across_segments_and_short_blips_fold() {
        let a = vec![1.0f32; secs(3.0)];
        let mut b = vec![2.0f32; secs(4.0)];
        b.extend(vec![1.0f32; secs(0.4)]); // too short to stand alone
        let spans = segment_by_speaker(&[&a, &b], fake_embed);
        assert_eq!(spans[0], vec![LabeledSpan { start: 0, end: a.len(), speaker: Some("speaker_1".into()) }]);
        assert_eq!(spans[1].len(), 1);
        assert_eq!(spans[1][0].speaker.as_deref(), Some("speaker_2"));
        assert_eq!(spans[1][0].end, b.len());
    }
}
