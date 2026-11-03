//! NVIDIA Nemotron-3 Speaker Diarization engine module.
//!
//! Provides on-device streaming and offline speaker diarization using
//! NVIDIA's Nemotron-3 (Sortformer v3) architecture via ONNX Runtime.
//!
//! # Features
//! - Up to 8 speakers identified concurrently
//! - Low latency streaming or high-accuracy offline diarization
//! - Runs 100% locally on-device via ONNX Runtime (ort)
//! - Integrated with transcription pipeline for automatic speaker attribution

pub mod commands;
pub mod diarization_engine;
pub mod model;

pub use commands::*;
pub use diarization_engine::{
    DiarizationConfig, DiarizationEngine, DiarizationEngineError, DiarizationModelInfo,
    DiarizationModelStatus, DownloadProgress, StreamingPreset,
};
pub use model::{
    align_words_to_speaker_turns, DiarizationResult, NemotronDiarizationModel, SpeakerSegment,
    SpeakerTurn, TimedWord,
};
