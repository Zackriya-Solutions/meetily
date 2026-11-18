//! User-editable glossary of domain terms that speech-to-text tends to mishear.
//!
//! The glossary is a plain-text file in the config directory. Its non-comment
//! lines are added to the chunk and final summary prompts for every template,
//! framed as spelling corrections for garbled terms, never as content.

use std::path::{Path, PathBuf};
use log::{info, warn};

const FILE_NAME: &str = "glossary.txt";
const DEFAULT_GLOSSARY: &str = include_str!("default_glossary.txt");

/// The glossary rides along with every transcript chunk, so an oversized file
/// would eat the context of small local models. Lines past this are dropped.
const MAX_GLOSSARY_CHARS: usize = 4000;

pub fn glossary_path() -> PathBuf {
    crate::storage::config_dir().join(FILE_NAME)
}

/// Reads the glossary at summary time, so edits apply to the next summary
/// without a restart. Returns `None` when the file has no terms.
pub fn load() -> Option<String> {
    load_from(&glossary_path())
}

/// A missing file is seeded with the default glossary. An existing file is
/// never overwritten, so a user who empties it keeps the glossary off.
fn load_from(path: &Path) -> Option<String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::write(path, DEFAULT_GLOSSARY) {
                Ok(()) => info!("Created default summary glossary at {}", path.display()),
                Err(e) => warn!("Failed to create summary glossary at {}: {}", path.display(), e),
            }
            DEFAULT_GLOSSARY.to_string()
        }
        Err(e) => {
            warn!("Failed to read summary glossary at {}: {}", path.display(), e);
            return None;
        }
    };
    terms_from(&raw)
}

/// Drops comment and blank lines and caps the size at a line boundary.
fn terms_from(raw: &str) -> Option<String> {
    // Windows editors may save UTF-8 with a BOM, which would hide the first
    // line's comment marker.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut terms = String::new();
    for line in raw.lines().map(str::trim_end) {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if terms.len() + line.len() + 1 > MAX_GLOSSARY_CHARS {
            warn!("Summary glossary is over {MAX_GLOSSARY_CHARS} characters; ignoring the remaining lines");
            break;
        }
        terms.push_str(line);
        terms.push('\n');
    }
    let terms = terms.trim_end();
    (!terms.is_empty()).then(|| terms.to_string())
}

/// The prompt section that carries the glossary. The wording limits it to
/// fixing how terms are written, so the model cannot treat a listed term as
/// evidence that the meeting discussed it.
pub(crate) fn prompt_block(glossary: &str) -> String {
    format!(
        "**TERM GLOSSARY:**\nSpeech-to-text often mishears the domain terms below. When the source text contains a garbled form of a glossary term (it sounds alike and fits the context), write the glossary term instead. Use the glossary only to correct how terms are written: never add a term, fact, or topic that the source text does not contain.\n\n<glossary>\n{glossary}\n</glossary>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_glossary_has_terms_and_strips_comments() {
        let terms = terms_from(DEFAULT_GLOSSARY).expect("default glossary has terms");
        assert!(terms.contains("man-days"));
        assert!(terms.contains("Delta 48"));
        assert!(!terms.contains('#'));
        assert!(!terms.starts_with('\n'));
    }

    #[test]
    fn comment_only_glossary_is_off() {
        assert_eq!(terms_from("# just a note\n\n   \n  # indented note\n"), None);
    }

    #[test]
    fn byte_order_mark_does_not_turn_a_comment_into_a_term() {
        assert_eq!(terms_from("\u{feff}# header\r\nUAT2: \"U82\"\r\n").as_deref(), Some("UAT2: \"U82\""));
    }

    #[test]
    fn oversized_glossary_keeps_whole_lines_within_the_cap() {
        let line = "x".repeat(99);
        let raw = std::iter::repeat(line.as_str()).take(100).collect::<Vec<_>>().join("\n");
        let terms = terms_from(&raw).unwrap();
        assert!(terms.len() <= MAX_GLOSSARY_CHARS);
        assert!(terms.lines().all(|l| l == line));
    }

    #[test]
    fn missing_file_is_seeded_once_and_user_edits_win() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);

        let seeded = load_from(&path).unwrap();
        assert!(seeded.contains("Murex"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_GLOSSARY);

        std::fs::write(&path, "# off\n").unwrap();
        assert_eq!(load_from(&path), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# off\n");

        std::fs::write(&path, "UAT2: \"U82\"\n").unwrap();
        assert_eq!(load_from(&path).as_deref(), Some("UAT2: \"U82\""));
    }

    #[test]
    fn prompt_block_frames_terms_as_corrections_only() {
        let block = prompt_block("UAT2: \"U82\"");
        assert!(block.contains("<glossary>\nUAT2: \"U82\"\n</glossary>"));
        assert!(block.contains("never add a term, fact, or topic"));
    }
}
