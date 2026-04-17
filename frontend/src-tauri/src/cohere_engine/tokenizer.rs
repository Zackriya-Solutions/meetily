//! Tokenizer helpers: loads `tokenizer.json` and locates Whisper-style special tokens.
//!
//! The Cohere Transcribe tokenizer reuses Whisper's special-token naming:
//! `<|startoftranscript|>`, `<|<lang>|>`, `<|transcribe|>`, `<|notimestamps|>`,
//! and `<|endoftext|>`.

use anyhow::{anyhow, Result};
use tokenizers::Tokenizer;

#[derive(Debug, Clone)]
pub struct SpecialTokens {
    pub sot: u32,
    pub eot: u32,
    pub transcribe: u32,
    pub no_timestamps: u32,
    pub lang: u32,
}

fn lookup(tokenizer: &Tokenizer, token: &str) -> Result<u32> {
    tokenizer
        .token_to_id(token)
        .ok_or_else(|| anyhow!("special token '{token}' not found in tokenizer"))
}

/// Resolve the special-token ids needed to drive the decoder for `language`.
///
/// `language` is a base BCP-47 code (e.g. `"ko"`, `"en"`). The lookup mirrors
/// Whisper's convention — `<|ko|>`, `<|en|>`, etc.
pub fn resolve_special_tokens(tokenizer: &Tokenizer, language: &str) -> Result<SpecialTokens> {
    let sot = lookup(tokenizer, "<|startoftranscript|>")?;
    let eot = lookup(tokenizer, "<|endoftext|>")?;
    let transcribe = lookup(tokenizer, "<|transcribe|>")?;
    let no_timestamps = lookup(tokenizer, "<|notimestamps|>")?;
    let lang_tok = format!("<|{language}|>");
    let lang = lookup(tokenizer, &lang_tok)?;
    Ok(SpecialTokens {
        sot,
        eot,
        transcribe,
        no_timestamps,
        lang,
    })
}

/// Decode a token stream back to plain text, skipping special tokens.
pub fn decode(tokenizer: &Tokenizer, ids: &[u32]) -> Result<String> {
    tokenizer
        .decode(ids, true)
        .map_err(|e| anyhow!("tokenizer decode: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt-in integration test: place a real `tokenizer.json` at
    /// `frontend/src-tauri/test_fixtures/cohere_tokenizer.json` to enable.
    #[test]
    #[ignore]
    fn test_tokenizer_loads_and_finds_korean_token() {
        let path = std::path::Path::new("test_fixtures/cohere_tokenizer.json");
        if !path.exists() {
            return;
        }
        let tokenizer = Tokenizer::from_file(path).expect("load tokenizer");
        let tokens = resolve_special_tokens(&tokenizer, "ko").expect("resolve ko");
        assert!(tokens.sot > 0);
        assert!(tokens.lang > 0);
    }
}
