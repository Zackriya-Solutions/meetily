// diarization/mod.rs
//
// Speaker identification (diarization) for the live transcription pipeline.
// Rust-native and fully local: WeSpeaker ONNX embeddings (via ort, the same
// runtime parakeet uses) + online cosine clustering. See docs in each module.

pub mod clustering;
pub mod clustering_offline;
pub mod commands;
pub mod embedding;
pub mod fbank;
pub mod models;
pub mod offline;
pub mod overlap_detector;
pub mod segmentation;
pub mod session;
pub mod timeline;

pub use offline::{diarize_offline, SpeakerSegment};
pub use session::DiarizationSession;
