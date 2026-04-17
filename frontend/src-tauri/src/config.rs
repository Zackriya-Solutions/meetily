/// Application configuration constants.
///
/// Cohere Transcribe 03-2026 is the sole on-device STT provider; Whisper and
/// Parakeet have been removed.

/// Default Cohere ONNX transcription model.
pub const DEFAULT_COHERE_MODEL: &str = "cohere-transcribe-03-2026";

/// Default transcription language (BCP-47 base code). Korean is the target.
pub const DEFAULT_LANGUAGE: &str = "ko";
