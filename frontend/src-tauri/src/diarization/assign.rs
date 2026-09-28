//! Map diarization turns onto transcript rows and VAD segments.
use super::cluster::cosine;
use super::Turn;
use crate::api::TranscriptSegment;
use crate::audio::vad::SpeechSegment;
use std::collections::HashMap;

pub const MIXED_MIN_SECONDS: f64 = 1.5;
pub const MIXED_MIN_FRACTION: f64 = 0.3;
pub const NEAREST_TURN_MAX_GAP_S: f64 = 1.0;
pub const MIN_PIECE_S: f64 = 0.3;
pub const CARRY_OVER_MIN_SIMILARITY: f32 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowSpan {
    pub start_s: f64,
    pub end_s: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowLabel {
    Unlabeled,
    Single(String),
    /// A clear mid-row speaker change; `pieces` cover the row contiguously.
    Mixed { majority: String, pieces: Vec<Turn> },
}

fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// Seconds spoken by each speaker inside the span, largest first.
fn speaker_totals(span: RowSpan, turns: &[Turn]) -> Vec<(String, f64)> {
    let mut totals: HashMap<&str, f64> = HashMap::new();
    for t in turns {
        let o = overlap(span.start_s, span.end_s, t.start_s, t.end_s);
        if o > 0.0 {
            *totals.entry(t.key.as_str()).or_default() += o;
        }
    }
    let mut v: Vec<(String, f64)> = totals.into_iter().map(|(k, s)| (k.to_string(), s)).collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    v
}

fn nearest_turn(span: RowSpan, turns: &[Turn]) -> Option<&Turn> {
    turns
        .iter()
        .map(|t| {
            let gap = if t.end_s <= span.start_s { span.start_s - t.end_s } else { t.start_s - span.end_s };
            (gap.max(0.0), t)
        })
        .filter(|(gap, _)| *gap <= NEAREST_TURN_MAX_GAP_S)
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, t)| t)
}

/// Contiguous single-speaker pieces covering `span`. Empty when no turn overlaps it.
pub fn pieces_for_span(span: RowSpan, turns: &[Turn]) -> Vec<Turn> {
    let mut pieces: Vec<Turn> = Vec::new();
    for t in turns {
        let start = t.start_s.max(span.start_s);
        let end = t.end_s.min(span.end_s);
        if end <= start {
            continue;
        }
        match pieces.last_mut() {
            Some(last) if last.key == t.key => last.end_s = end,
            _ => pieces.push(Turn { start_s: start, end_s: end, key: t.key.clone() }),
        }
    }
    // Fold tiny pieces into their predecessor (or successor for the first one).
    let mut merged: Vec<Turn> = Vec::with_capacity(pieces.len());
    for p in pieces {
        if p.duration() < MIN_PIECE_S {
            if let Some(last) = merged.last_mut() {
                last.end_s = p.end_s;
                continue;
            }
        }
        match merged.last_mut() {
            Some(last) if last.key == p.key => last.end_s = p.end_s,
            Some(last) if last.duration() < MIN_PIECE_S => {
                *last = Turn { start_s: last.start_s, end_s: p.end_s, key: p.key.clone() };
            }
            _ => merged.push(p),
        }
    }
    // Close gaps so no audio falls between pieces, and stretch to the span edges.
    for i in 1..merged.len() {
        let prev_end = merged[i - 1].end_s;
        merged[i].start_s = prev_end;
    }
    if let Some(first) = merged.first_mut() {
        first.start_s = span.start_s;
    }
    if let Some(last) = merged.last_mut() {
        last.end_s = span.end_s;
    }
    merged
}

/// Label each row (None = row has no audio timing) by majority overlap.
pub fn label_rows(rows: &[Option<RowSpan>], turns: &[Turn]) -> Vec<RowLabel> {
    rows.iter()
        .map(|row| {
            let Some(span) = *row else { return RowLabel::Unlabeled };
            let totals = speaker_totals(span, turns);
            let Some((majority, _)) = totals.first() else {
                return nearest_turn(span, turns)
                    .map(|t| RowLabel::Single(t.key.clone()))
                    .unwrap_or(RowLabel::Unlabeled);
            };
            let row_len = (span.end_s - span.start_s).max(f64::EPSILON);
            let mixed = totals
                .get(1)
                .map(|(_, s)| *s >= MIXED_MIN_SECONDS && *s / row_len >= MIXED_MIN_FRACTION)
                .unwrap_or(false);
            if mixed {
                let pieces = pieces_for_span(span, turns);
                if pieces.len() > 1 {
                    return RowLabel::Mixed { majority: majority.clone(), pieces };
                }
            }
            RowLabel::Single(majority.clone())
        })
        .collect()
}

/// Cut VAD segments at speaker changes so each piece carries one speaker.
pub fn split_segments_at_turns(segments: Vec<SpeechSegment>, turns: &[Turn], sample_rate: usize) -> Vec<SpeechSegment> {
    let mut out = Vec::with_capacity(segments.len());
    for seg in segments {
        let span = RowSpan { start_s: seg.start_timestamp_ms / 1000.0, end_s: seg.end_timestamp_ms / 1000.0 };
        let pieces = pieces_for_span(span, turns);
        if pieces.len() <= 1 {
            out.push(seg);
            continue;
        }
        let to_index = |t_s: f64| -> usize {
            (((t_s - span.start_s) * sample_rate as f64).round().max(0.0) as usize).min(seg.samples.len())
        };
        for p in &pieces {
            let (a, b) = (to_index(p.start_s), to_index(p.end_s));
            if b <= a {
                continue;
            }
            out.push(SpeechSegment {
                samples: seg.samples[a..b].to_vec(),
                start_timestamp_ms: p.start_s * 1000.0,
                end_timestamp_ms: p.end_s * 1000.0,
                confidence: seg.confidence,
            });
        }
    }
    out
}

/// Set each segment's speaker by majority overlap (segments already split per speaker).
pub fn label_segments(segments: &mut [TranscriptSegment], turns: &[Turn]) {
    let spans: Vec<Option<RowSpan>> = segments
        .iter()
        .map(|s| match (s.audio_start_time, s.audio_end_time) {
            (Some(a), Some(b)) => Some(RowSpan { start_s: a, end_s: b }),
            _ => None,
        })
        .collect();
    for (seg, label) in segments.iter_mut().zip(label_rows(&spans, turns)) {
        seg.speaker = match label {
            RowLabel::Unlabeled => None,
            RowLabel::Single(k) | RowLabel::Mixed { majority: k, .. } => Some(k),
        };
    }
}

/// Greedy one-to-one match of new speakers to the meeting's previous speakers by voice
/// similarity, highest first. `new` holds (key, centroid); `old` holds (display name if any,
/// centroid). Unnamed previous voices take part, so a name cannot move onto their voice.
/// Returns new key → carried-over name.
pub fn carry_over_names(
    new: &[(String, Vec<f32>)],
    old: &[(Option<String>, Vec<f32>)],
    min_similarity: f32,
) -> HashMap<String, String> {
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for (i, (_, ne)) in new.iter().enumerate() {
        for (j, (_, oe)) in old.iter().enumerate() {
            let s = cosine(ne, oe);
            if s >= min_similarity {
                pairs.push((s, i, j));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut used_new = vec![false; new.len()];
    let mut used_old = vec![false; old.len()];
    let mut names = HashMap::new();
    for (_, i, j) in pairs {
        if used_new[i] || used_old[j] {
            continue;
        }
        used_new[i] = true;
        used_old[j] = true;
        if let Some(name) = &old[j].0 {
            names.insert(new[i].0.clone(), name.clone());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::vad::SpeechSegment;

    fn turn(s: f64, e: f64, k: &str) -> Turn {
        Turn { start_s: s, end_s: e, key: k.to_string() }
    }
    fn span(s: f64, e: f64) -> Option<RowSpan> {
        Some(RowSpan { start_s: s, end_s: e })
    }

    #[test]
    fn majority_speaker_labels_a_row() {
        let turns = vec![turn(0.0, 4.0, "spk_0"), turn(4.0, 5.0, "spk_1")];
        // 4 s vs 1 s: second speaker below 1.5 s, so not mixed.
        assert_eq!(label_rows(&[span(0.0, 5.0)], &turns), vec![RowLabel::Single("spk_0".into())]);
    }

    #[test]
    fn clear_mid_row_change_marks_row_mixed_with_covering_pieces() {
        let turns = vec![turn(0.0, 3.0, "spk_0"), turn(3.2, 6.0, "spk_1")];
        let labels = label_rows(&[span(0.5, 6.0)], &turns);
        match &labels[0] {
            RowLabel::Mixed { majority, pieces } => {
                assert_eq!(majority, "spk_1");
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[0].start_s, 0.5);
                assert_eq!(pieces[0].end_s, pieces[1].start_s, "pieces are contiguous");
                assert_eq!(pieces[1].end_s, 6.0);
                assert_eq!(pieces[0].key, "spk_0");
            }
            other => panic!("expected mixed, got {other:?}"),
        }
    }

    #[test]
    fn second_speaker_must_be_long_and_a_large_share() {
        // 2 s of spk_1 inside a 10 s row: ≥ 1.5 s but only 20 % → single.
        let turns = vec![turn(0.0, 8.0, "spk_0"), turn(8.0, 10.0, "spk_1")];
        assert_eq!(label_rows(&[span(0.0, 10.0)], &turns), vec![RowLabel::Single("spk_0".into())]);
    }

    #[test]
    fn row_without_overlap_takes_nearest_turn_within_one_second() {
        let turns = vec![turn(0.0, 1.0, "spk_0"), turn(10.0, 11.0, "spk_1")];
        let labels = label_rows(&[span(1.5, 2.0), span(5.0, 6.0)], &turns);
        assert_eq!(labels, vec![RowLabel::Single("spk_0".into()), RowLabel::Unlabeled]);
    }

    #[test]
    fn identify_rows_without_timing_stay_unlabelled() {
        let turns = vec![turn(0.0, 10.0, "spk_0")];
        assert_eq!(label_rows(&[None, span(1.0, 2.0)], &turns), vec![RowLabel::Unlabeled, RowLabel::Single("spk_0".into())]);
    }

    #[test]
    fn tiny_pieces_join_a_neighbour() {
        let turns = vec![turn(0.0, 3.0, "spk_0"), turn(3.0, 3.1, "spk_2"), turn(3.1, 6.0, "spk_1")];
        let pieces = pieces_for_span(RowSpan { start_s: 0.0, end_s: 6.0 }, &turns);
        assert_eq!(pieces.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), vec!["spk_0", "spk_1"]);
    }

    #[test]
    fn vad_segments_are_cut_at_speaker_changes() {
        let seg = SpeechSegment { samples: vec![0.1; 16000 * 4], start_timestamp_ms: 1000.0, end_timestamp_ms: 5000.0, confidence: 0.9 };
        let turns = vec![turn(0.0, 3.0, "spk_0"), turn(3.0, 9.0, "spk_1")];
        let out = split_segments_at_turns(vec![seg], &turns, 16000);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].samples.len(), 16000 * 2);
        assert_eq!(out[1].samples.len(), 16000 * 2);
        assert_eq!(out[0].end_timestamp_ms, 3000.0);
        assert_eq!(out[1].start_timestamp_ms, 3000.0);
    }

    #[test]
    fn single_speaker_segment_is_untouched() {
        let seg = SpeechSegment { samples: vec![0.1; 1600], start_timestamp_ms: 0.0, end_timestamp_ms: 100.0, confidence: 0.9 };
        let out = split_segments_at_turns(vec![seg.clone()], &[turn(0.0, 1.0, "spk_0")], 16000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].samples.len(), seg.samples.len());
    }

    #[test]
    fn names_carry_over_to_best_matching_new_speaker() {
        let new = vec![("spk_0".to_string(), vec![0.0, 1.0]), ("spk_1".to_string(), vec![1.0, 0.1])];
        let old = vec![(Some("Noah".to_string()), vec![1.0, 0.0]), (Some("Ana".to_string()), vec![-1.0, 0.0])];
        let names = carry_over_names(&new, &old, CARRY_OVER_MIN_SIMILARITY);
        assert_eq!(names.get("spk_1").map(String::as_str), Some("Noah"));
        assert_eq!(names.get("spk_0"), None, "Ana is below the similarity floor");
    }

    #[test]
    fn unnamed_old_voice_keeps_name_from_moving() {
        // spk_0 is the unnamed old voice (cosine 0.92) and only 0.66 like Noah; spk_1 is Noah (0.63).
        // Ignoring the unnamed voice would hand Noah's name to spk_0.
        let new = vec![("spk_0".to_string(), vec![0.92, 0.3919]), ("spk_1".to_string(), vec![-0.5405, 0.8413])];
        let old = vec![(None, vec![1.0, 0.0]), (Some("Noah".to_string()), vec![0.3129, 0.9498])];
        let names = carry_over_names(&new, &old, CARRY_OVER_MIN_SIMILARITY);
        assert_eq!(names.get("spk_0"), None);
        assert_eq!(names.get("spk_1").map(String::as_str), Some("Noah"));
    }
}
