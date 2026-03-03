// Analytics module - no-op stubs (PostHog telemetry removed for privacy)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsConfig {
    pub api_key: String,
    pub host: Option<String>,
    pub enabled: bool,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            host: None,
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    pub session_id: String,
    pub user_id: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub is_active: bool,
}

impl UserSession {
    pub fn new(user_id: String) -> Self {
        Self {
            session_id: format!("session_{}", uuid::Uuid::new_v4()),
            user_id,
            start_time: chrono::Utc::now(),
            is_active: false,
        }
    }

    pub fn duration_seconds(&self) -> i64 {
        (chrono::Utc::now() - self.start_time).num_seconds()
    }
}

pub struct AnalyticsClient;

impl AnalyticsClient {
    pub async fn new(_config: AnalyticsConfig) -> Self {
        Self
    }

    pub async fn identify(&self, _user_id: String, _properties: Option<HashMap<String, String>>) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_event(&self, _event_name: &str, _properties: Option<HashMap<String, String>>) -> Result<(), String> {
        Ok(())
    }

    pub async fn start_session(&self, _user_id: String) -> Result<String, String> {
        Ok(String::new())
    }

    pub async fn end_session(&self) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_daily_active_user(&self) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_user_first_launch(&self) -> Result<(), String> {
        Ok(())
    }

    pub async fn get_current_session(&self) -> Option<UserSession> {
        None
    }

    pub async fn is_session_active(&self) -> bool {
        false
    }

    pub async fn track_meeting_started(&self, _meeting_id: &str, _meeting_title: &str) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_recording_started(&self, _meeting_id: &str) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_recording_stopped(&self, _meeting_id: &str, _duration_seconds: Option<u64>) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_meeting_deleted(&self, _meeting_id: &str) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_settings_changed(&self, _setting_type: &str, _new_value: &str) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_app_started(&self, _version: &str) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_feature_used(&self, _feature_name: &str) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_summary_generation_started(&self, _model_provider: &str, _model_name: &str, _transcript_length: usize) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_summary_generation_completed(&self, _model_provider: &str, _model_name: &str, _success: bool, _duration_seconds: Option<u64>, _error_message: Option<&str>) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_summary_regenerated(&self, _model_provider: &str, _model_name: &str) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_model_changed(&self, _old_provider: &str, _old_model: &str, _new_provider: &str, _new_model: &str) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_custom_prompt_used(&self, _prompt_length: usize) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_meeting_ended(
        &self,
        _transcription_provider: &str,
        _transcription_model: &str,
        _summary_provider: &str,
        _summary_model: &str,
        _total_duration_seconds: Option<f64>,
        _active_duration_seconds: f64,
        _pause_duration_seconds: f64,
        _microphone_device_type: &str,
        _system_audio_device_type: &str,
        _chunks_processed: u64,
        _transcript_segments_count: u64,
        _had_fatal_error: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_analytics_enabled(&self) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_analytics_disabled(&self) -> Result<(), String> {
        Ok(())
    }

    pub async fn track_analytics_transparency_viewed(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn is_enabled(&self) -> bool {
        false
    }

    pub async fn set_user_properties(&self, _properties: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }
}

// Helper function to create analytics client from config
pub async fn create_analytics_client(config: AnalyticsConfig) -> AnalyticsClient {
    AnalyticsClient::new(config).await
}
