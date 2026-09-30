// Whisper prompt token counting without loading a model.
//
// whisper.cpp conditions decoding on at most `n_text_ctx / 2` prompt tokens
// (224 for every Whisper model) and keeps the LAST ones when a prompt is
// longer. Characters are the wrong unit: Norwegian words and names cost 2–3
// tokens each, so a 958-character domain file was 368 tokens and its first
// 144 tokens had no effect at all.
//
// At transcription time the loaded model's own tokenizer (`whisper_tokenize`)
// is authoritative, see `WhisperEngine::domain_prompt_tokens`. The domain
// editor usually runs while no model is loaded (models are unloaded after a
// recording), so this module reproduces whisper.cpp's tokenizer from the
// vocabulary stored in the ggml file header, without reading the weights:
//
// 1. Split the text into words with the GPT-2 pattern
//    `'s|'t|'re|'ve|'m|'ll|'d| ?[[:alpha:]]+| ?[[:digit:]]+| ?[^\s[:alpha:][:digit:]]+|\s+(?!\S)|\s+`.
//    whisper.cpp runs it through `std::regex` in the C locale, so
//    `[[:alpha:]]` only matches ASCII letters and "æøå" land in the
//    "other" class.
// 2. For each word, repeatedly take the longest prefix that is a vocabulary
//    entry. Bytes that match nothing are skipped (whisper.cpp logs them).

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const GGML_MAGIC: u32 = 0x6767_6d6c;

/// Most prompt tokens whisper.cpp will use for a model with `n_text_ctx`.
pub fn prompt_token_limit(n_text_ctx: i32) -> usize {
    (n_text_ctx.max(0) / 2) as usize
}

/// Which prompt tokens survive whisper.cpp's limit. Over the limit the START
/// is dropped (`keep_from > 0`), matching whisper.cpp, which keeps the last
/// tokens. Domain files therefore put the most important terms last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptFit {
    pub total: usize,
    pub limit: usize,
    /// Index of the first token that is kept.
    pub keep_from: usize,
}

impl PromptFit {
    pub fn new(total: usize, limit: usize) -> Self {
        Self {
            total,
            limit,
            keep_from: total.saturating_sub(limit),
        }
    }

    pub fn is_over(&self) -> bool {
        self.keep_from > 0
    }

    pub fn dropped(&self) -> usize {
        self.keep_from
    }

    pub fn kept(&self) -> usize {
        self.total - self.keep_from
    }
}

/// Token strings of a Whisper model, read from the ggml file header.
pub struct GgmlVocab {
    tokens: HashSet<Box<[u8]>>,
    max_token_len: usize,
    pub n_text_ctx: i32,
}

impl GgmlVocab {
    /// Read the vocabulary from a ggml model file. Only the header is read
    /// (hparams, mel filters, vocabulary), never the weights.
    pub fn read(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("opening model {}", path.display()))?;
        let mut r = BufReader::new(file);

        if read_u32(&mut r)? != GGML_MAGIC {
            bail!("{} is not a ggml model", path.display());
        }
        // n_vocab, n_audio_ctx, n_audio_state, n_audio_head, n_audio_layer,
        // n_text_ctx, n_text_state, n_text_head, n_text_layer, n_mels, ftype
        let mut hparams = [0i32; 11];
        for hp in hparams.iter_mut() {
            *hp = read_i32(&mut r)?;
        }
        let n_text_ctx = hparams[5];

        let n_mel = read_i32(&mut r)?;
        let n_fft = read_i32(&mut r)?;
        if n_mel < 0 || n_fft < 0 {
            bail!("{}: invalid mel filter header", path.display());
        }
        r.seek(SeekFrom::Current(4 * n_mel as i64 * n_fft as i64))?;

        let n_vocab = read_i32(&mut r)?;
        if !(0..=1_000_000).contains(&n_vocab) {
            bail!("{}: implausible vocabulary size {}", path.display(), n_vocab);
        }
        let mut tokens = HashSet::with_capacity(n_vocab as usize);
        let mut max_token_len = 0;
        for _ in 0..n_vocab {
            let len = read_u32(&mut r)? as usize;
            if len > 4096 {
                bail!("{}: implausible token length {}", path.display(), len);
            }
            let mut buf = vec![0u8; len];
            r.read_exact(&mut buf)?;
            max_token_len = max_token_len.max(len);
            tokens.insert(buf.into_boxed_slice());
        }

        Ok(Self {
            tokens,
            max_token_len,
            n_text_ctx,
        })
    }

    #[cfg(test)]
    fn from_tokens(tokens: &[&str], n_text_ctx: i32) -> Self {
        Self {
            tokens: tokens.iter().map(|t| t.as_bytes().into()).collect(),
            max_token_len: tokens.iter().map(|t| t.len()).max().unwrap_or(0),
            n_text_ctx,
        }
    }

    /// Tokenize like whisper.cpp. Returns each token's byte range in `text`.
    pub fn tokenize(&self, text: &str) -> Vec<Range<usize>> {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        for word in split_words(bytes) {
            let mut i = word.start;
            while i < word.end {
                let longest = (i + 1..=word.end.min(i + self.max_token_len))
                    .rev()
                    .find(|&j| self.tokens.contains(&bytes[i..j]));
                match longest {
                    Some(j) => {
                        out.push(i..j);
                        i = j;
                    }
                    None => i += 1, // unknown byte, whisper.cpp skips it too
                }
            }
        }
        out
    }
}

fn read_u32(r: &mut impl Read) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_i32(r: &mut impl Read) -> Result<i32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(i32::from_le_bytes(b))
}

/// `\s` in the C locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

#[derive(PartialEq)]
enum Class {
    Alpha,
    Digit,
    Other,
    Space,
}

fn class(c: u8) -> Class {
    if c.is_ascii_alphabetic() {
        Class::Alpha
    } else if c.is_ascii_digit() {
        Class::Digit
    } else if is_space(c) {
        Class::Space
    } else {
        Class::Other
    }
}

/// Split into words with whisper.cpp's GPT-2 pattern (see module docs).
/// Hand-written because the `regex` crate has no lookahead for `\s+(?!\S)`.
fn split_words(b: &[u8]) -> Vec<Range<usize>> {
    const CONTRACTIONS: [&[u8]; 7] = [b"'s", b"'t", b"'re", b"'ve", b"'m", b"'ll", b"'d"];

    let run = |from: usize, cls: Class| -> usize {
        b[from..].iter().take_while(|&&c| class(c) == cls).count()
    };

    let mut words = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let rest = &b[i..];

        // 's|'t|'re|'ve|'m|'ll|'d
        if let Some(c) = CONTRACTIONS.iter().find(|c| rest.starts_with(c)) {
            words.push(i..i + c.len());
            i += c.len();
            continue;
        }

        // ` ?[[:alpha:]]+`, ` ?[[:digit:]]+`, ` ?[^\s[:alpha:][:digit:]]+`
        let lead = usize::from(rest[0] == b' ' && rest.len() > 1);
        let cls = class(rest[lead]);
        if cls != Class::Space {
            let len = lead + run(i + lead, cls);
            words.push(i..i + len);
            i += len;
            continue;
        }

        // `\s+(?!\S)` takes a whitespace run but leaves its last character
        // when a word follows; `\s+` then catches a lone whitespace character.
        let spaces = run(i, Class::Space);
        let len = if i + spaces == b.len() || spaces == 1 {
            spaces
        } else {
            spaces - 1
        };
        words.push(i..i + len);
        i += len;
    }
    words
}

/// Result of counting a domain prompt, shown in the domain editor.
#[derive(Debug, Clone, Serialize)]
pub struct PromptTokenCount {
    pub tokens: usize,
    pub limit: usize,
    /// Model whose vocabulary was used.
    pub model: String,
    /// Start of the prompt that whisper.cpp drops, when over the limit.
    pub dropped_text: Option<String>,
}

static VOCAB_CACHE: Mutex<Option<(PathBuf, Option<SystemTime>, Arc<GgmlVocab>)>> = Mutex::new(None);

fn cached_vocab(path: &Path) -> Result<Arc<GgmlVocab>> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut cache = VOCAB_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, t, vocab)) = cache.as_ref() {
        if p == path && *t == mtime {
            return Ok(vocab.clone());
        }
    }
    let vocab = Arc::new(GgmlVocab::read(path)?);
    *cache = Some((path.to_path_buf(), mtime, vocab.clone()));
    Ok(vocab)
}

/// Count `text` in tokens of the model at `model_path` and report what
/// whisper.cpp would drop.
pub fn count_prompt_tokens(model: &str, model_path: &Path, text: &str) -> Result<PromptTokenCount> {
    let vocab = cached_vocab(model_path)?;
    let tokens = vocab.tokenize(text);
    let fit = PromptFit::new(tokens.len(), prompt_token_limit(vocab.n_text_ctx));
    let dropped_text = fit.is_over().then(|| {
        let mut end = tokens[fit.keep_from].start;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text[..end].trim().to_string()
    });
    Ok(PromptTokenCount {
        tokens: fit.total,
        limit: fit.limit,
        model: model.to_string(),
        dropped_text,
    })
}

/// Model file to count with: `preferred` if present on disk, else the default
/// Whisper model, else any catalog model present. All multilingual Whisper
/// models share the same token strings, so any of them gives the same count.
pub fn resolve_tokenizer_model(models_dir: &Path, preferred: Option<&str>) -> Option<(String, PathBuf)> {
    use crate::config::{DEFAULT_WHISPER_MODEL, WHISPER_MODEL_CATALOG};

    let file_for = |name: &str| {
        WHISPER_MODEL_CATALOG
            .iter()
            .find(|(n, ..)| *n == name)
            .map(|&(_, filename, ..)| models_dir.join(filename))
    };
    preferred
        .into_iter()
        .chain(std::iter::once(DEFAULT_WHISPER_MODEL))
        .chain(WHISPER_MODEL_CATALOG.iter().map(|&(name, ..)| name))
        .find_map(|name| file_for(name).filter(|p| p.is_file()).map(|p| (name.to_string(), p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<&str> {
        split_words(text.as_bytes())
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn split_words_follows_gpt2_pattern() {
        assert_eq!(words("Hello world"), ["Hello", " world"]);
        assert_eq!(words("it's 42, ok"), ["it", "'s", " 42", ",", " ok"]);
        // Non-ASCII letters are not [[:alpha:]] in the C locale.
        assert_eq!(words(" Kirkerådet"), [" Kirker", "å", "det"]);
        assert_eq!(words("Ås"), ["Å", "s"]);
        // A whitespace run before a word leaves its last space to the word.
        assert_eq!(words("a   b"), ["a", "  ", " b"]);
        // A lone newline before a word is its own token (`\s+`).
        assert_eq!(words("a\nb"), ["a", "\n", "b"]);
        assert_eq!(words("a\n\nb"), ["a", "\n", "\n", "b"]);
        // Trailing whitespace is taken whole.
        assert_eq!(words("a  "), ["a", "  "]);
        assert_eq!(words(" ,."), [" ,."]);
    }

    #[test]
    fn tokenize_takes_longest_vocab_match() {
        let vocab = GgmlVocab::from_tokens(&["Kir", "Kirk", "e", " Kirk", "er", "å", "det", "d", "et"], 448);
        let text = " Kirkerådet";
        let toks: Vec<&str> = vocab.tokenize(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(toks, [" Kirk", "er", "å", "det"]);
    }

    #[test]
    fn tokenize_skips_unknown_bytes() {
        let vocab = GgmlVocab::from_tokens(&["a", "b"], 448);
        assert_eq!(vocab.tokenize("axb").len(), 2);
    }

    #[test]
    fn prompt_fit_over_limit_keeps_the_tail() {
        // Old Kirken.txt: 368 tokens against whisper.cpp's 224.
        let fit = PromptFit::new(368, prompt_token_limit(448));
        assert!(fit.is_over());
        assert_eq!(fit.limit, 224);
        assert_eq!(fit.keep_from, 144);
        assert_eq!(fit.dropped(), 144);
        assert_eq!(fit.kept(), 224);
    }

    #[test]
    fn prompt_fit_under_limit_is_unchanged() {
        let fit = PromptFit::new(208, prompt_token_limit(448));
        assert!(!fit.is_over());
        assert_eq!(fit.keep_from, 0);
        assert_eq!(fit.kept(), 208);
    }

    #[test]
    fn count_prompt_tokens_reports_dropped_start() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("ggml-test.bin");
        write_test_model(&path, &["a", " b", " c", ","], 8); // limit 4
        let count = count_prompt_tokens("test", &path, "a, b, c, b, c").unwrap();
        // a , ␣b , ␣c , ␣b , ␣c → 9 tokens, the first 5 dropped.
        assert_eq!(count.tokens, 9);
        assert_eq!(count.limit, 4);
        assert_eq!(count.dropped_text.as_deref(), Some("a, b, c"));

        let count = count_prompt_tokens("test", &path, "a, b").unwrap();
        assert_eq!(count.tokens, 3);
        assert_eq!(count.dropped_text, None);
    }

    fn write_test_model(path: &Path, tokens: &[&str], n_text_ctx: i32) {
        let mut b = Vec::new();
        b.extend(GGML_MAGIC.to_le_bytes());
        for hp in [tokens.len() as i32, 1500, 384, 6, 4, n_text_ctx, 384, 6, 4, 80, 1] {
            b.extend(hp.to_le_bytes());
        }
        b.extend(2i32.to_le_bytes()); // n_mel
        b.extend(3i32.to_le_bytes()); // n_fft
        b.extend([0u8; 4 * 2 * 3]);
        b.extend((tokens.len() as i32).to_le_bytes());
        for t in tokens {
            b.extend((t.len() as u32).to_le_bytes());
            b.extend(t.as_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    /// Compare this port with the model's own `whisper_tokenize` on real
    /// prompt files. Loads the full model, so it is opt-in:
    ///
    /// MEETILY_MODEL_PATH=…/ggml-nb-whisper-large.bin \
    /// MEETILY_PROMPT_FILES=a.txt:b.txt \
    /// cargo test --lib tokens_match_whisper_tokenize -- --ignored --nocapture
    #[test]
    #[ignore]
    fn tokens_match_whisper_tokenize() {
        use whisper_rs::{WhisperContext, WhisperContextParameters};

        let model = std::env::var("MEETILY_MODEL_PATH").expect("MEETILY_MODEL_PATH");
        let files = std::env::var("MEETILY_PROMPT_FILES").expect("MEETILY_PROMPT_FILES");
        let vocab = GgmlVocab::read(Path::new(&model)).unwrap();
        let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default()).unwrap();
        assert_eq!(vocab.n_text_ctx, ctx.n_text_ctx());

        for file in files.split(':') {
            let raw = std::fs::read_to_string(file).unwrap();
            let text = raw.trim(); // as load_prompt sends it
            let ours = vocab.tokenize(text).len();
            let theirs = ctx.tokenize(text, text.len() + 1).unwrap();
            let limit = prompt_token_limit(ctx.n_text_ctx());
            println!(
                "{}: {} chars, {} tokens (whisper_tokenize {}), limit {}",
                file,
                text.chars().count(),
                ours,
                theirs.len(),
                limit
            );
            assert_eq!(ours, theirs.len(), "{}", file);

            // The editor's "dropped" text must match what the engine drops.
            let fit = PromptFit::new(theirs.len(), limit);
            let engine_dropped: Vec<u8> = theirs[..fit.keep_from]
                .iter()
                .flat_map(|&t| ctx.token_to_cstr(t).unwrap().to_bytes().to_vec())
                .collect();
            let count = count_prompt_tokens("test", Path::new(&model), text).unwrap();
            let engine_dropped = String::from_utf8_lossy(&engine_dropped).trim().to_string();
            assert_eq!(count.dropped_text.unwrap_or_default(), engine_dropped, "{}", file);
            if fit.is_over() {
                println!("  dropped {} tokens: \"{}\"", fit.dropped(), engine_dropped);
            }
        }
    }
}
