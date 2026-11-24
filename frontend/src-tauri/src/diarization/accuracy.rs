// diarization/accuracy.rs
//
// How often the automatic speaker labels agree with the names Dor gave.
// Each meeting's automatic label (Speaker 2) counts as the name most of its
// named lines carry; a line is right when its own name is that name. A name
// belongs to one label per meeting (the one with most lines of it): one
// person split across two labels is a mistake, not two right answers. Lines
// shorter than MIN_SCORED_SECONDS are left out: one-word replies are
// deliberately left unlabelled and would only add noise.
//
// A voice the app named from a profile makes a claim of its own: its lines
// are right only when the name given is that name, and that person cannot
// also be credited to an anonymous label in the same meeting.

use serde::Serialize;
use std::collections::HashMap;

pub const MIN_SCORED_SECONDS: f64 = 2.5;

/// One transcript line as the score sees it.
pub struct ScoredLine {
    pub meeting_id: String,
    pub speaker_label: Option<String>,
    pub speaker_name: Option<String>,
    pub duration: Option<f64>,
    /// The name the app guessed for this line's voice, if any.
    pub speaker_guess: Option<String>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct AccuracyCounts {
    /// Named lines long enough to score.
    pub scored: usize,
    /// Of those, lines whose automatic label maps to the same name.
    pub correct: usize,
    /// Of those, lines the app left without a label.
    pub unlabelled: usize,
    /// Of those, lines whose voice the app named from a profile (right or
    /// wrong).
    pub guessed: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct SpeakerAccuracy {
    pub overall: AccuracyCounts,
    pub meetings: HashMap<String, AccuracyCounts>,
}

pub fn speaker_accuracy(lines: &[ScoredLine]) -> SpeakerAccuracy {
    let scored: Vec<(&str, Option<&str>, &str, Option<&str>)> = lines
        .iter()
        .filter(|l| l.duration.unwrap_or(0.0) >= MIN_SCORED_SECONDS)
        .filter_map(|l| {
            let name = l.speaker_name.as_deref()?.trim();
            let guess = l.speaker_guess.as_deref().map(str::trim).filter(|g| !g.is_empty());
            (!name.is_empty()).then(|| (l.meeting_id.as_str(), l.speaker_label.as_deref(), name, guess))
        })
        .collect();
    let same_person = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    // People already named by a guess, per meeting.
    let claimed: Vec<(&str, &str)> = scored
        .iter()
        .filter_map(|&(meeting, label, _, guess)| Some((meeting, guess.filter(|_| label.is_some())?)))
        .collect();

    // The name each anonymous (meeting, label) mostly carries.
    let mut votes: HashMap<(&str, &str), HashMap<&str, usize>> = HashMap::new();
    for &(meeting, label, name, guess) in &scored {
        if let (Some(label), None) = (label, guess) {
            *votes.entry((meeting, label)).or_default().entry(name).or_default() += 1;
        }
    }
    // Each label's top name and how many lines carry it. Ties go to the
    // alphabetically first name, so the score is stable.
    let top: Vec<((&str, &str), &str, usize)> = votes
        .into_iter()
        .filter_map(|(key, names)| {
            let (name, count) = names.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))?;
            Some((key, name, count))
        })
        .collect();
    // A name goes to the label with most lines of it (ties: first label).
    let mut owner: HashMap<(&str, &str), ((&str, &str), usize)> = HashMap::new();
    for &(key, name, count) in &top {
        let claim = owner.entry((key.0, name)).or_insert((key, count));
        if count > claim.1 || (count == claim.1 && key.1 < claim.0 .1) {
            *claim = (key, count);
        }
    }
    let majority: HashMap<(&str, &str), &str> = top
        .into_iter()
        .filter(|&(key, name, _)| owner[&(key.0, name)].0 == key)
        .filter(|&(key, name, _)| !claimed.iter().any(|&(m, g)| m == key.0 && same_person(g, name)))
        .map(|(key, name, _)| (key, name))
        .collect();

    let mut result = SpeakerAccuracy::default();
    for (meeting, label, name, guess) in scored {
        let counts = result.meetings.entry(meeting.to_string()).or_default();
        for c in [&mut result.overall, counts] {
            c.scored += 1;
            match (label, guess) {
                (None, _) => c.unlabelled += 1,
                (Some(_), Some(guess)) => {
                    c.guessed += 1;
                    if same_person(guess, name) {
                        c.correct += 1;
                    }
                }
                (Some(label), None) if majority.get(&(meeting, label)) == Some(&name) => c.correct += 1,
                (Some(_), None) => {}
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(meeting: &str, label: Option<&str>, name: Option<&str>, duration: f64) -> ScoredLine {
        ScoredLine {
            meeting_id: meeting.into(),
            speaker_label: label.map(Into::into),
            speaker_name: name.map(Into::into),
            duration: Some(duration),
            speaker_guess: None,
        }
    }

    fn guessed(meeting: &str, label: &str, name: &str, guess: &str) -> ScoredLine {
        ScoredLine { speaker_guess: Some(guess.into()), ..line(meeting, Some(label), Some(name), 3.0) }
    }

    #[test]
    fn a_guess_is_right_only_when_it_is_the_name_given() {
        let lines = vec![
            guessed("m1", "Speaker 1", "Alex", "alex"),
            guessed("m1", "Speaker 1", "Bea", "Alex"),
            guessed("m1", "Speaker 2", "Bea", "Alex"),
            // Alex is already claimed by a guess, so this voice cannot be
            // credited with Alex as well.
            line("m1", Some("Speaker 3"), Some("Alex"), 3.0),
            line("m1", Some("Speaker 4"), Some("Dana"), 3.0),
        ];
        let acc = speaker_accuracy(&lines);
        assert_eq!(acc.overall, AccuracyCounts { scored: 5, correct: 2, unlabelled: 0, guessed: 3 });
    }

    #[test]
    fn a_label_counts_as_the_name_most_of_its_lines_carry() {
        let lines = vec![
            line("m1", Some("Speaker 1"), Some("Alex"), 4.0),
            line("m1", Some("Speaker 1"), Some("Alex"), 3.0),
            line("m1", Some("Speaker 1"), Some("Bea"), 5.0),
            line("m1", Some("Speaker 2"), Some("Bea"), 3.0),
        ];
        let acc = speaker_accuracy(&lines);
        assert_eq!(acc.overall, AccuracyCounts { scored: 4, correct: 3, unlabelled: 0, guessed: 0 });
    }

    #[test]
    fn short_unnamed_and_unlabelled_lines() {
        let lines = vec![
            line("m1", Some("Speaker 1"), Some("Alex"), 1.0),
            line("m1", Some("Speaker 1"), None, 9.0),
            line("m1", None, Some("Alex"), 3.0),
            line("m1", Some("Speaker 1"), Some("  "), 3.0),
        ];
        let acc = speaker_accuracy(&lines);
        assert_eq!(acc.overall, AccuracyCounts { scored: 1, correct: 0, unlabelled: 1, guessed: 0 });
    }

    #[test]
    fn one_person_split_across_two_labels_is_not_all_right() {
        let lines = vec![
            line("m1", Some("Speaker 1"), Some("Alex"), 3.0),
            line("m1", Some("Speaker 1"), Some("Alex"), 3.0),
            line("m1", Some("Speaker 1"), Some("Alex"), 3.0),
            line("m1", Some("Speaker 2"), Some("Alex"), 3.0),
        ];
        let acc = speaker_accuracy(&lines);
        assert_eq!(acc.overall, AccuracyCounts { scored: 4, correct: 3, unlabelled: 0, guessed: 0 });
    }

    #[test]
    fn labels_are_per_meeting() {
        let lines = vec![
            line("m1", Some("Speaker 1"), Some("Alex"), 3.0),
            line("m2", Some("Speaker 1"), Some("Bea"), 3.0),
        ];
        let acc = speaker_accuracy(&lines);
        assert_eq!(acc.overall.correct, 2);
        assert_eq!(acc.meetings["m2"], AccuracyCounts { scored: 1, correct: 1, unlabelled: 0, guessed: 0 });
    }
}
