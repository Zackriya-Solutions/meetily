use regex::{Regex, RegexBuilder};

use crate::preferences::TranscriptCleanupSettings;

pub fn clean_for_storage(text: &str, settings: &TranscriptCleanupSettings) -> String {
    let original = text.trim();
    if original.is_empty() {
        return String::new();
    }

    if !settings.enabled {
        return original.to_string();
    }

    let mut cleaned = clean_repetitions(original);
    if settings.remove_fillers {
        cleaned = remove_fillers(&cleaned);
    }
    cleaned = normalize_text(&cleaned);
    cleaned = apply_contextual_improvements(&cleaned);
    cleaned.trim().to_string()
}

fn clean_repetitions(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 3 {
        return text.to_string();
    }

    let mut cleaned = Vec::new();
    let mut i = 0usize;
    while i < words.len() {
        let current = words[i];
        cleaned.push(current);

        let mut repeat_count = 1usize;
        while i + repeat_count < words.len()
            && words[i + repeat_count].eq_ignore_ascii_case(current)
        {
            repeat_count += 1;
        }

        if repeat_count > 1 {
            i += repeat_count;
        } else {
            i += 1;
        }
    }

    cleaned.join(" ")
}

fn remove_fillers(text: &str) -> String {
    const FILLERS: [&str; 12] = [
        "uh", "um", "er", "ah", "oh", "hm", "hmm", "uhh", "umm", "err", "ahh", "ohh",
    ];

    let mut output = text.to_string();
    for filler in FILLERS {
        let pattern = format!(r"\b{}\b[, ]*", regex::escape(filler));
        let Ok(regex) = RegexBuilder::new(&pattern).case_insensitive(true).build() else {
            continue;
        };
        output = regex.replace_all(&output, " ").to_string();
    }
    output
}

fn normalize_text(text: &str) -> String {
    let mut output = text.trim().to_string();
    let double_ws = Regex::new(r"\s+").expect("valid regex");
    output = double_ws.replace_all(&output, " ").to_string();
    output = output.replace(" .", ".");
    output = output.replace(" ,", ",");
    output = output.replace(" ?", "?");
    output = output.replace(" !", "!");

    if let Some(first) = output.chars().next() {
        if first.is_lowercase() {
            output = first.to_uppercase().collect::<String>() + &output[first.len_utf8()..];
        }
    }
    output
}

fn apply_contextual_improvements(text: &str) -> String {
    let corrections = [
        ("cant", "can't"),
        ("wont", "won't"),
        ("dont", "don't"),
        ("doesnt", "doesn't"),
        ("didnt", "didn't"),
        ("wouldnt", "wouldn't"),
        ("couldnt", "couldn't"),
        ("shouldnt", "shouldn't"),
        ("isnt", "isn't"),
        ("arent", "aren't"),
    ];

    let mut output = text.to_string();
    for (incorrect, correct) in corrections {
        let pattern = format!(r"\b{}\b", regex::escape(incorrect));
        let Ok(regex) = RegexBuilder::new(&pattern).case_insensitive(true).build() else {
            continue;
        };
        output = regex.replace_all(&output, correct).to_string();
    }
    output
}

#[cfg(test)]
mod tests {
    use crate::preferences::TranscriptCleanupSettings;

    use super::clean_for_storage;

    #[test]
    fn cleanup_removes_fillers_when_enabled() {
        let settings = TranscriptCleanupSettings {
            enabled: true,
            remove_fillers: true,
        };

        let cleaned = clean_for_storage("um this is uh a test", &settings);
        assert_eq!(cleaned, "This is a test");
    }

    #[test]
    fn cleanup_keeps_fillers_when_toggle_off() {
        let settings = TranscriptCleanupSettings {
            enabled: true,
            remove_fillers: false,
        };

        let cleaned = clean_for_storage("um this is uh a test", &settings);
        assert!(cleaned.to_lowercase().contains("um"));
        assert!(cleaned.to_lowercase().contains("uh"));
    }

    #[test]
    fn cleanup_is_idempotent() {
        let settings = TranscriptCleanupSettings {
            enabled: true,
            remove_fillers: true,
        };

        let once = clean_for_storage("um   this  is  dont  test", &settings);
        let twice = clean_for_storage(&once, &settings);
        assert_eq!(once, twice);
    }

    #[test]
    fn cleanup_disabled_returns_trimmed_original() {
        let settings = TranscriptCleanupSettings {
            enabled: false,
            remove_fillers: true,
        };

        let cleaned = clean_for_storage("  um this is a test  ", &settings);
        assert_eq!(cleaned, "um this is a test");
    }
}
