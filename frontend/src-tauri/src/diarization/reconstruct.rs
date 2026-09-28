//! Turn per-window segmentation output plus cluster assignments into speaker turns.

/// pyannote segmentation-3.0 predicts up to 3 local speakers per window.
pub const NUM_LOCAL: usize = 3;

/// Powerset classes of segmentation-3.0 (max 2 simultaneous speakers).
pub const POWERSET: [&[usize]; 7] = [&[], &[0], &[1], &[2], &[0, 1], &[0, 2], &[1, 2]];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameGeometry {
    /// Samples between consecutive output frames.
    pub frame_shift: usize,
    /// Receptive field of one output frame, in samples.
    pub frame_size: usize,
    pub sample_rate: usize,
}

/// Hard multi-label activity (0/1 per local speaker) from powerset scores.
pub fn powerset_to_multilabel(scores: &[[f32; 7]]) -> Vec<[f32; NUM_LOCAL]> {
    scores
        .iter()
        .map(|frame| {
            let best = frame
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let mut out = [0f32; NUM_LOCAL];
            for &speaker in POWERSET[best] {
                out[speaker] = 1.0;
            }
            out
        })
        .collect()
}

pub struct WindowActivity {
    pub start_sample: usize,
    pub activity: Vec<[f32; NUM_LOCAL]>,
    /// Global cluster for each local speaker; None when it was not embedded.
    pub local_to_global: [Option<usize>; NUM_LOCAL],
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawTurn {
    pub start_s: f64,
    pub end_s: f64,
    pub cluster: usize,
}

/// Aggregate overlapping windows on a global frame grid and label each frame with its
/// dominant cluster, or nothing when the windows agree nobody is speaking.
pub fn reconstruct(
    windows: &[WindowActivity],
    total_samples: usize,
    geo: FrameGeometry,
    num_clusters: usize,
) -> Vec<RawTurn> {
    if total_samples == 0 || num_clusters == 0 {
        return Vec::new();
    }
    let num_frames = total_samples / geo.frame_shift + 1;
    // Sparse per-frame scores: each window touches at most NUM_LOCAL clusters per frame,
    // so memory does not grow with the number of clusters.
    let mut score: Vec<Vec<(usize, f32)>> = vec![Vec::new(); num_frames];
    let mut active_sum = vec![0f32; num_frames];
    let mut coverage = vec![0u32; num_frames];

    for w in windows {
        for (i, frame) in w.activity.iter().enumerate() {
            let center = w.start_sample + i * geo.frame_shift + geo.frame_size / 2;
            if center >= total_samples {
                break;
            }
            let g = center / geo.frame_shift;
            coverage[g] += 1;
            active_sum[g] += frame.iter().sum::<f32>();
            // Max over this window's local speakers that map to the same cluster.
            let mut local: [(Option<usize>, f32); NUM_LOCAL] = [(None, 0.0); NUM_LOCAL];
            for l in 0..NUM_LOCAL {
                if let Some(c) = w.local_to_global[l] {
                    if let Some(e) = local.iter_mut().find(|e| e.0 == Some(c)) {
                        e.1 = e.1.max(frame[l]);
                    } else if let Some(e) = local.iter_mut().find(|e| e.0.is_none()) {
                        *e = (Some(c), frame[l]);
                    }
                }
            }
            for (c, v) in local.iter().filter_map(|&(c, v)| c.map(|c| (c, v))) {
                match score[g].iter_mut().find(|e| e.0 == c) {
                    Some(e) => e.1 += v,
                    None => score[g].push((c, v)),
                }
            }
        }
    }

    let label = |g: usize| -> Option<usize> {
        if coverage[g] == 0 || (active_sum[g] / coverage[g] as f32).round() < 1.0 {
            return None;
        }
        // Largest score wins; ties go to the larger cluster index.
        let (best, value) = score[g]
            .iter()
            .copied()
            .reduce(|a, b| if b.1 > a.1 || (b.1 == a.1 && b.0 > a.0) { b } else { a })?;
        (value > 0.0).then_some(best)
    };

    let frame_s = geo.frame_shift as f64 / geo.sample_rate as f64;
    let total_s = total_samples as f64 / geo.sample_rate as f64;
    let mut turns: Vec<RawTurn> = Vec::new();
    for g in 0..num_frames {
        let Some(cluster) = label(g) else { continue };
        let start = g as f64 * frame_s;
        let end = ((g + 1) as f64 * frame_s).min(total_s);
        match turns.last_mut() {
            Some(last) if last.cluster == cluster && (last.end_s - start).abs() < 1e-9 => last.end_s = end,
            _ => turns.push(RawTurn { start_s: start, end_s: end, cluster }),
        }
    }
    turns
}

fn bridge_same_speaker(turns: Vec<RawTurn>, max_gap_s: f64) -> Vec<RawTurn> {
    let mut out: Vec<RawTurn> = Vec::with_capacity(turns.len());
    for turn in turns {
        match out.last_mut() {
            Some(last) if last.cluster == turn.cluster && turn.start_s - last.end_s <= max_gap_s => {
                last.end_s = last.end_s.max(turn.end_s);
            }
            _ => out.push(turn),
        }
    }
    out
}

/// Bridge short same-speaker gaps, fold turns shorter than `min_turn_s` into an adjacent
/// turn (previous first, within `max_gap_s`), drop isolated blips, then bridge again.
pub fn smooth_turns(mut turns: Vec<RawTurn>, min_turn_s: f64, max_gap_s: f64) -> Vec<RawTurn> {
    turns.sort_by(|a, b| a.start_s.partial_cmp(&b.start_s).unwrap_or(std::cmp::Ordering::Equal));
    let turns = bridge_same_speaker(turns, max_gap_s);

    let mut out: Vec<RawTurn> = Vec::with_capacity(turns.len());
    let mut pending_start: Option<f64> = None;
    for (i, turn) in turns.iter().enumerate() {
        let short = turn.end_s - turn.start_s < min_turn_s;
        if !short {
            let mut turn = turn.clone();
            if let Some(start) = pending_start.take() {
                turn.start_s = start;
            }
            out.push(turn);
            continue;
        }
        if let Some(prev) = out.last_mut() {
            if turn.start_s - prev.end_s <= max_gap_s {
                prev.end_s = turn.end_s;
                continue;
            }
        }
        if let Some(next) = turns.get(i + 1) {
            if next.start_s - turn.end_s <= max_gap_s {
                pending_start = Some(pending_start.unwrap_or(turn.start_s));
                continue;
            }
        }
        // Isolated blip: drop it.
    }
    bridge_same_speaker(out, max_gap_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 100 Hz "audio", 10-sample frames: one frame = 0.1 s.
    const GEO: FrameGeometry = FrameGeometry { frame_shift: 10, frame_size: 10, sample_rate: 100 };

    fn t(start: f64, end: f64, cluster: usize) -> RawTurn {
        RawTurn { start_s: start, end_s: end, cluster }
    }

    #[test]
    fn powerset_argmax_decodes_single_and_overlap_classes() {
        let mut silent = [-9.0f32; 7];
        silent[0] = 0.0;
        let mut second = [-9.0f32; 7];
        second[2] = 0.0;
        let mut overlap = [-9.0f32; 7];
        overlap[5] = 0.0; // {0, 2}
        let out = powerset_to_multilabel(&[silent, second, overlap]);
        assert_eq!(out, vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 1.0]]);
    }

    #[test]
    fn two_speakers_in_one_window_become_two_turns() {
        // 20 frames: local 0 speaks frames 0..10, local 1 frames 10..20.
        let activity = (0..20)
            .map(|i| if i < 10 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] })
            .collect();
        let windows = vec![WindowActivity { start_sample: 0, activity, local_to_global: [Some(1), Some(0), None] }];
        let turns = reconstruct(&windows, 200, GEO, 2);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].cluster, 1);
        assert_eq!(turns[1].cluster, 0);
        assert!((turns[0].start_s - 0.0).abs() < 1e-9);
        assert!((turns[1].start_s - 1.0).abs() < 0.11);
        assert!((turns[1].end_s - 2.0).abs() < 0.11);
    }

    #[test]
    fn overlapping_windows_vote_and_padding_is_ignored() {
        // Window A says cluster 0 for frames 0..20; window B (starting at 1.0 s) says cluster 0
        // for its first 10 frames. Audio is only 1.5 s long, so B's tail is padding.
        let a = WindowActivity { start_sample: 0, activity: vec![[1.0, 0.0, 0.0]; 20], local_to_global: [Some(0), None, None] };
        let b = WindowActivity { start_sample: 100, activity: vec![[1.0, 0.0, 0.0]; 20], local_to_global: [Some(0), None, None] };
        let turns = reconstruct(&[a, b], 150, GEO, 1);
        assert_eq!(turns.len(), 1);
        assert!(turns[0].end_s <= 1.5 + 1e-9);
    }

    #[test]
    fn unmapped_local_speaker_frames_stay_unlabelled() {
        let activity = vec![[0.0, 0.0, 1.0]; 10];
        let windows = vec![WindowActivity { start_sample: 0, activity, local_to_global: [Some(0), None, None] }];
        assert!(reconstruct(&windows, 100, GEO, 1).is_empty());
    }

    #[test]
    fn smoothing_bridges_small_gaps_and_absorbs_blips() {
        let turns = vec![t(0.0, 2.0, 0), t(2.3, 4.0, 0), t(4.0, 4.1, 1), t(4.1, 6.0, 0), t(9.0, 9.1, 2)];
        let smoothed = smooth_turns(turns, 0.3, 0.5);
        assert_eq!(smoothed, vec![t(0.0, 6.0, 0)]);
    }

    #[test]
    fn smoothing_keeps_real_speaker_changes() {
        let turns = vec![t(0.0, 2.0, 0), t(2.0, 4.0, 1), t(5.0, 7.0, 0)];
        assert_eq!(smooth_turns(turns.clone(), 0.3, 0.5), turns);
    }

    #[test]
    fn huge_cluster_counts_give_the_same_turns() {
        let make = || {
            let activity = (0..20).map(|i| if i < 10 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }).collect();
            vec![WindowActivity { start_sample: 0, activity, local_to_global: [Some(1), Some(0), None] }]
        };
        assert_eq!(reconstruct(&make(), 200, GEO, 100_000), reconstruct(&make(), 200, GEO, 3));
    }
}
