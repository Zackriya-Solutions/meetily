/**
 * Default model names for transcription engines.
 * IMPORTANT: Keep in sync with Rust constants in src-tauri/src/config.rs
 */

/**
 * Default Cohere ONNX model for transcription when no preference is configured.
 * This is the Cohere Transcribe 03-2026 local ONNX model.
 */
export const DEFAULT_COHERE_MODEL = 'cohere-transcribe-03-2026';

/**
 * Model defaults by provider type.
 * Only Cohere is supported after the 2026 migration.
 */
export const MODEL_DEFAULTS = {
  cohere: DEFAULT_COHERE_MODEL,
} as const;
