//! Decode saved audio continuously and give each overlapping window its own output region.
use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
};

use anyhow::{ensure, Context, Result};

use super::ffmpeg::find_ffmpeg_path;
use crate::parakeet_engine::TimestampedResult;

const RATE: usize = 16_000;
const CORE: usize = 24 * RATE;

type Transcript = Vec<(String, f64, f64)>;

struct Decoder(Child);

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Return text with start and end times in milliseconds. PCM memory stays bounded
/// regardless of recording length. Output is discarded if any window fails.
pub fn transcribe_saved_audio(
    path: &Path,
    decode: impl FnMut(Vec<f32>) -> Result<TimestampedResult>,
) -> Result<Transcript> {
    transcribe_saved_audio_with_context(path, 1, decode)
}

pub(crate) fn transcribe_saved_audio_with_context(
    path: &Path,
    edge_seconds: usize,
    mut decode: impl FnMut(Vec<f32>) -> Result<TimestampedResult>,
) -> Result<Transcript> {
    let edge = edge_seconds * RATE;
    let ffmpeg = find_ffmpeg_path().context("Audio decoder is unavailable")?;
    let mut decoder = Decoder(
        Command::new(ffmpeg)
            .args(["-nostdin", "-v", "error", "-xerror", "-i"])
            .arg(path)
            .args(["-map", "0:a:0", "-ac", "1", "-ar", "16000", "-f", "f32le", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Could not start audio decoder")?,
    );
    let mut pcm = decoder.0.stdout.take().context("No decoded audio stream")?;
    let mut output = Vec::new();
    let mut samples = Vec::with_capacity(CORE + edge * 2);
    let mut core_start = 0usize;
    loop {
        let origin = core_start.saturating_sub(edge);
        let limit = core_start + CORE + edge - origin;
        let mut bytes = Vec::with_capacity((limit - samples.len()) * 4);
        (&mut pcm).take(((limit - samples.len()) * 4) as u64).read_to_end(&mut bytes)?;
        ensure!(bytes.len() % 4 == 0, "Incomplete PCM sample");
        samples.extend(bytes.chunks_exact(4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]])));
        let core_end = (core_start + CORE).min(origin + samples.len());
        if core_end <= core_start {
            break;
        }

        let result = decode(samples.clone())?;
        if let Some((text, first)) = owned_text(result, origin, core_start, core_end)? {
            output.push((text, milliseconds(first), milliseconds(core_end)));
        }
        if samples.len() < limit {
            break;
        }
        samples.drain(..samples.len() - edge * 2);
        core_start += CORE;
    }
    // A short read alone cannot distinguish a clean EOF from corrupt input.
    drop(pcm);
    ensure!(decoder.0.wait()?.success(), "Could not decode the full recording");
    Ok(output)
}

fn milliseconds(sample: usize) -> f64 {
    sample as f64 * 1000.0 / RATE as f64
}

pub(super) fn owned_text(
    result: TimestampedResult,
    origin: usize,
    start: usize,
    end: usize,
) -> Result<Option<(String, usize)>> {
    ensure!(result.tokens.len() == result.timestamps.len(), "Unmatched Parakeet token times");
    let mut first = None;
    let mut word_is_owned = false;
    let mut text = String::new();
    for (index, (token, seconds)) in result.tokens.iter().zip(result.timestamps).enumerate() {
        ensure!(seconds.is_finite() && seconds >= 0.0, "Invalid Parakeet token time");
        if index == 0 || token.starts_with(' ') {
            let position = origin + (seconds as f64 * RATE as f64).round() as usize;
            // Assign by the start of a word, keeping its punctuation and subword pieces together.
            word_is_owned = (start..end).contains(&position);
            if word_is_owned {
                first.get_or_insert(position);
            }
        }
        if word_is_owned {
            text.push_str(token);
        }
    }
    let text = text.trim().to_owned();
    Ok(first.filter(|_| !text.is_empty()).map(|sample| (text, sample)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result() -> TimestampedResult {
        TimestampedResult {
            text: String::new(),
            timestamps: vec![0.5, 0.75, 1.0, 1.25, 1.5],
            tokens: [" catch", "word", " next", "ly", "."].iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn overlap_owns_each_word_once_without_splitting_subwords() {
        assert_eq!(
            owned_text(result(), RATE, RATE, RATE * 2).unwrap(),
            Some(("catchword".into(), RATE * 3 / 2))
        );
        assert_eq!(
            owned_text(result(), RATE, RATE * 2, RATE * 3).unwrap(),
            Some(("nextly.".into(), RATE * 2))
        );
    }
}
