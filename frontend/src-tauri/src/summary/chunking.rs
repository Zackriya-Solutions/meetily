/// Window and overlap are measured in Unicode scalar values, not UTF-8 bytes.
pub(crate) fn split_windows(text: &str, window: usize, overlap: usize) -> Vec<String> {
    if window == 0 || text.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let sentence_end = |i: usize| match chars[i - 1] {
        '\n' | '؟' => true,
        '.' | '!' | '?' => chars.get(i).map_or(true, |c| c.is_whitespace()),
        _ => false,
    };
    let word_end = |i: usize| {
        chars[i - 1].is_whitespace() || matches!(chars[i - 1], '،' | '؛') || sentence_end(i)
    };

    let mut chunks = Vec::new();
    let mut start: usize = 0;
    loop {
        let limit = start.saturating_add(window).min(chars.len());
        if limit == chars.len() {
            chunks.push(chars[start..].iter().collect());
            break;
        }
        // A preferred end must advance past the previous overlap.
        let candidates = start.saturating_add(overlap).saturating_add(1)..=limit;
        let end = candidates
            .clone()
            .rev()
            .find(|&i| sentence_end(i))
            .or_else(|| candidates.rev().find(|&i| word_end(i)))
            .unwrap_or(limit);
        chunks.push(chars[start..end].iter().collect());

        // Round forward, never skipping unconsumed text. For uninterrupted
        // strings, keep the bounded character overlap and guarantee progress.
        let earliest = end.saturating_sub(overlap).max(start + 1);
        start = (earliest..=end).find(|&i| word_end(i)).unwrap_or(earliest);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::split_windows;

    #[test]
    fn arabic_punctuation_without_spaces() {
        for mark in ['؟', '،', '؛'] {
            let first = format!("مرحبا{mark}");
            let text = format!("{first}بالجميع");
            assert_eq!(split_windows(&text, 8, 0), [first, "بالجميع".into()]);
        }
    }

    #[test]
    fn prefers_line_break_to_later_spaces() {
        assert_eq!(split_windows("one\ntwo more words", 10, 0)[0], "one\n");
    }

    #[test]
    fn overlap_restarts_at_a_word_boundary() {
        assert_eq!(
            split_windows("one three four seven", 15, 7),
            ["one three four ", "four seven"]
        );
    }

    #[test]
    fn does_not_treat_decimal_point_as_a_sentence_end() {
        assert_eq!(split_windows("Pi 3.14 radians", 8, 0)[0], "Pi 3.14 ");
    }

    #[test]
    fn zero_overlap_round_trips_every_window_size() {
        let text = "أهلا؟كيف الحال، اليوم؛\nمتابعة 🚀. Done!";
        for size in 1..=text.chars().count() {
            let chunks = split_windows(text, size, 0);
            assert_eq!(chunks.concat(), text);
            assert!(chunks
                .iter()
                .all(|c| (1..=size).contains(&c.chars().count())));
        }
    }

    #[test]
    fn unbroken_unicode_progresses_even_with_excessive_overlap() {
        assert_eq!(split_windows("أبجد", 3, usize::MAX), ["أبج", "بجد"]);
    }
}
