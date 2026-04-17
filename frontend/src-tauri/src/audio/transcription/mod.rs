// audio/transcription/mod.rs
//
// Transcription module: Provider abstraction, engine routing, and worker pool.
// The Cohere ONNX path is the single local provider in this build.

pub mod provider;
pub mod cohere_provider;
pub mod engine;
pub mod worker;

// Re-export commonly used types
pub use provider::{TranscriptionError, TranscriptionProvider, TranscriptResult};
pub use cohere_provider::CohereProvider;
pub use engine::{
    TranscriptionEngine,
    validate_transcription_model_ready,
    get_or_init_transcription_engine,
};
pub use worker::{
    start_transcription_task,
    reset_speech_detected_flag,
    TranscriptUpdate
};
