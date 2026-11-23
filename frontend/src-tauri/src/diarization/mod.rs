// diarization/mod.rs
//
// Speaker identification for the other side of a call. Fully local:
// WeSpeaker ONNX embeddings (via ort, the same runtime parakeet uses) and
// online cosine clustering into "Speaker 1", "Speaker 2", ...
//
// Ported from upstream Meetily PR #511 (Krtin Shet).

pub mod embedding;
pub mod fbank;
