// audio/transcription/cohere_provider.rs
//
// Cohere ONNX transcription provider. Wraps `CohereEngine` so it can be
// driven through the same `TranscriptionProvider` trait as the other
// engines. The provider receives 16 kHz mono audio from the VAD stage.

use super::provider::{TranscriptionError, TranscriptionProvider, TranscriptResult};
use async_trait::async_trait;
use std::sync::Arc;

pub struct CohereProvider {
    engine: Arc<crate::cohere_engine::CohereEngine>,
}

impl CohereProvider {
    pub fn new(engine: Arc<crate::cohere_engine::CohereEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl TranscriptionProvider for CohereProvider {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> std::result::Result<TranscriptResult, TranscriptionError> {
        // Audio from the VAD stage is already 16 kHz mono f32.
        match self.engine.transcribe_audio(audio, 16_000, language).await {
            Ok(text) => Ok(TranscriptResult {
                text: text.trim().to_string(),
                confidence: None,
                is_partial: false,
            }),
            Err(e) => Err(TranscriptionError::EngineFailed(e.to_string())),
        }
    }

    async fn is_model_loaded(&self) -> bool {
        self.engine.is_model_loaded().await
    }

    async fn get_current_model(&self) -> Option<String> {
        self.engine.get_current_model().await
    }

    fn provider_name(&self) -> &'static str {
        "Cohere"
    }
}
