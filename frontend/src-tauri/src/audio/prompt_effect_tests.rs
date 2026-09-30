// Opt-in end-to-end check that the meeting-domain prompt reaches every part of
// a long clip, through the retranscription path and through the live path
// (one long VAD chunk, split by the worker). Needs a real clip and Whisper
// model, so the tests are ignored by default:
//
//   MEETILY_EFFECT_AUDIO=clip.wav          # 16 kHz mono is fine, any decodable file works
//   MEETILY_EFFECT_OFFSET=2220             # clip start in the recording, for printed times
//   MEETILY_EFFECT_PROMPT=domains/X.txt    # omit to transcribe without a prompt
//   MEETILY_EFFECT_EXPECT=word             # optional: assert the transcript contains it
//   MEETILY_MODELS_DIR=…/models MEETILY_EFFECT_MODEL=nb-whisper-large MEETILY_EFFECT_LANGUAGE=no
//   RUST_LOG=app_lib=debug cargo test --release --lib prompt_effect -- --ignored --nocapture --test-threads=1

use crate::audio::common::{split_segment_at_silence, MAX_WHISPER_SEGMENT_SAMPLES};
use crate::audio::recording_state::DeviceType;
use crate::audio::retranscription::VAD_REDEMPTION_TIME_MS;
use crate::audio::transcription::worker::split_long_chunk;
use crate::audio::AudioChunk;
use crate::whisper_engine::WhisperEngine;
use std::path::{Path, PathBuf};

const SAMPLE_RATE: f64 = 16000.0;

struct Setup {
    engine: WhisperEngine,
    clip: Vec<f32>,
    offset: f64,
    prompt: Option<String>,
    language: String,
    expect: Option<String>,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn required(name: &str) -> String {
    var(name).unwrap_or_else(|| panic!("{name} is not set"))
}

async fn setup() -> Setup {
    let _ = env_logger::builder().is_test(true).try_init();

    let decoded = crate::audio::decoder::decode_audio_file(Path::new(&required("MEETILY_EFFECT_AUDIO")))
        .expect("decoding clip");
    let clip = decoded.to_whisper_format();
    let prompt = var("MEETILY_EFFECT_PROMPT")
        .map(|p| std::fs::read_to_string(p).expect("reading prompt").trim().to_string());

    let engine = WhisperEngine::new_with_models_dir(Some(PathBuf::from(required("MEETILY_MODELS_DIR"))))
        .expect("creating engine");
    engine.discover_models().await.expect("discovering models");
    let model = var("MEETILY_EFFECT_MODEL").unwrap_or_else(|| crate::config::DEFAULT_WHISPER_MODEL.to_string());
    engine.load_model(&model).await.expect("loading model");

    println!(
        "clip {:.1}s, model {}, prompt {}",
        clip.len() as f64 / SAMPLE_RATE,
        model,
        prompt.as_ref().map_or("none".to_string(), |p| format!("{} chars", p.chars().count()))
    );
    Setup {
        engine,
        clip,
        offset: var("MEETILY_EFFECT_OFFSET").map_or(0.0, |o| o.parse().expect("MEETILY_EFFECT_OFFSET")),
        prompt,
        language: var("MEETILY_EFFECT_LANGUAGE").unwrap_or_else(|| "no".to_string()),
        expect: var("MEETILY_EFFECT_EXPECT"),
    }
}

fn mmss(seconds: f64) -> String {
    format!("{:02}:{:04.1}", (seconds / 60.0).floor() as u64, seconds % 60.0)
}

/// Transcribe each (start seconds, samples) part with the prompt, print it,
/// and check the expected word.
async fn transcribe_parts(setup: &Setup, parts: Vec<(f64, Vec<f32>)>) {
    let mut transcript = String::new();
    let n = parts.len();
    for (i, (start, samples)) in parts.into_iter().enumerate() {
        let seconds = samples.len() as f64 / SAMPLE_RATE;
        let (text, _, _) = setup
            .engine
            .transcribe_audio_with_confidence(samples, Some(setup.language.clone()), setup.prompt.clone())
            .await
            .expect("transcribing");
        println!(
            "part {:>2}/{} [{}–{}] {:4.1}s prompt={}: {}",
            i + 1,
            n,
            mmss(start),
            mmss(start + seconds),
            seconds,
            setup.prompt.is_some(),
            text.trim()
        );
        transcript.push_str(&text);
        transcript.push('\n');
    }
    if let Some(expect) = &setup.expect {
        assert!(transcript.contains(expect.as_str()), "transcript does not contain {expect:?}");
    }
}

/// Same steps as `retranscription.rs`: VAD, split at 25 s, prompt per segment.
#[tokio::test]
#[ignore]
async fn prompt_effect_retranscription_path() {
    let setup = setup().await;
    let segments = crate::audio::vad::get_speech_chunks(&setup.clip, VAD_REDEMPTION_TIME_MS).expect("VAD");
    let parts = segments
        .iter()
        .flat_map(|s| split_segment_at_silence(s, MAX_WHISPER_SEGMENT_SAMPLES))
        .filter(|s| s.samples.len() >= 1600)
        .map(|s| (setup.offset + s.start_timestamp_ms / 1000.0, s.samples))
        .collect();
    transcribe_parts(&setup, parts).await;
}

/// Worst case for live transcription: the whole clip arrives as ONE VAD chunk
/// (live chunks of 165 s have been seen) and the worker splits it.
#[tokio::test]
#[ignore]
async fn prompt_effect_live_path() {
    let setup = setup().await;
    let chunk = AudioChunk {
        data: setup.clip.clone(),
        sample_rate: 16000,
        timestamp: setup.offset,
        chunk_id: 0,
        device_type: DeviceType::Microphone,
    };
    let parts = split_long_chunk(chunk)
        .into_iter()
        .map(|c| (c.timestamp, c.data))
        .collect();
    transcribe_parts(&setup, parts).await;
}
