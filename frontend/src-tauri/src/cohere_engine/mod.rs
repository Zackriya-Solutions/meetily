//! Cohere Transcribe 03-2026 local ONNX Runtime engine.
//!
//! This module provides speech-to-text transcription using the
//! [CohereLabs/cohere-transcribe-03-2026](https://huggingface.co/CohereLabs/cohere-transcribe-03-2026)
//! model served entirely on-device via ONNX Runtime (see
//! [onnx-community/cohere-transcribe-03-2026-ONNX](https://huggingface.co/onnx-community/cohere-transcribe-03-2026-ONNX)).
//!
//! # Architecture
//!
//! * [`preprocess`] converts raw f32 PCM samples at any sample rate into the
//!   80-bin log-mel spectrogram the model expects (Whisper-style, 16 kHz,
//!   400-sample window, 160-sample hop, 3000 frames = 30 seconds).
//! * [`tokenizer`] loads the HuggingFace `tokenizer.json` and exposes the
//!   special-token IDs (start-of-transcript, language, task, timestamps, EOT)
//!   required to drive the decoder.
//! * [`decode`] runs the encoder + autoregressive greedy decoder on ONNX
//!   Runtime sessions and returns the decoded text.
//! * [`downloader`] fetches the ONNX weights + tokenizer from the public HF
//!   mirror into the per-user models directory with progress events.
//! * [`engine`] orchestrates session lifetime, model selection, and the
//!   end-to-end `transcribe_audio` entrypoint.
//! * [`commands`] exposes the Tauri command surface consumed by the frontend.

pub mod commands;
pub mod decode;
pub mod downloader;
pub mod engine;
pub mod preprocess;
pub mod tokenizer;

pub use engine::{CohereEngine, CohereEngineState, ModelInfo, ModelStatus};
