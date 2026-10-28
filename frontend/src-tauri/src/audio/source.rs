//! Recording intent is separate from hardware devices. An application selection must
//! never pass through the device resolver's fallback-to-default behavior.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationIdentity {
    pub bundle_id: String,
    pub bundle_path: String,
    pub name: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AudioSource {
    #[default]
    System,
    Application {
        application: ApplicationIdentity,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RecordingSourceOptions {
    pub source: AudioSource,
    pub microphone_enabled: bool,
}

impl Default for RecordingSourceOptions {
    fn default() -> Self {
        Self {
            source: AudioSource::System,
            microphone_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub session_id: String,
    pub options: RecordingSourceOptions,
    pub state: SourceState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceState {
    Capturing,
    Waiting,
    Error,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureCapabilities {
    pub application_audio: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioApplication {
    #[serde(flatten)]
    pub identity: ApplicationIdentity,
    pub audio_active: bool,
}

#[tauri::command]
pub fn get_audio_capture_capabilities() -> CaptureCapabilities {
    #[cfg(target_os = "macos")]
    let supported = super::application_audio::is_supported();
    #[cfg(not(target_os = "macos"))]
    let supported = false;
    CaptureCapabilities {
        application_audio: supported,
        reason: (!supported)
            .then(|| "Application audio requires macOS 14.2 or later with Core Audio taps.".into()),
    }
}

#[tauri::command]
pub async fn list_audio_applications() -> Result<Vec<AudioApplication>, String> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(super::application_audio::list_applications)
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    Err("Application audio capture is available on macOS only.".into())
}

impl RecordingSourceOptions {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let AudioSource::Application { application } = &self.source {
            anyhow::ensure!(
                get_audio_capture_capabilities().application_audio,
                "Application audio requires macOS 14.2 or later with Core Audio taps."
            );
            anyhow::ensure!(
                !application.bundle_id.is_empty()
                    && std::path::Path::new(&application.bundle_path).is_absolute(),
                "Select a running application before recording."
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_options_enable_mic_and_system_audio() {
        let options: RecordingSourceOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(options, RecordingSourceOptions::default());
    }
    #[test]
    fn microphone_off_survives_round_trip() {
        let options = RecordingSourceOptions {
            microphone_enabled: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&options).unwrap();
        assert_eq!(
            serde_json::from_str::<RecordingSourceOptions>(&json).unwrap(),
            options
        );
        assert!(json.contains("\"microphoneEnabled\":false"));
    }
}
