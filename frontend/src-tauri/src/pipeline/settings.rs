//! Persisted configuration and run state for the automatic pipeline.
//!
//! Stored in `pipeline_settings.json` via `tauri-plugin-store`, the same
//! pattern `obsidian.rs` uses. The run state lives here too so a user pause
//! or a SharePoint auth-required stop survives an app restart.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "pipeline_settings.json";
const STORE_KEY: &str = "settings";

/// Why the pipeline is (or is not) processing work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PipelineRunState {
    /// Processing normally.
    Running,
    /// Stopped because the user asked; only a resume clears it.
    PausedByUser,
    /// SharePoint cookies expired. Scanning stops until the user signs in;
    /// local work (transcribe/summarize) keeps running.
    AuthRequired { host: String, since: String },
}

impl Default for PipelineRunState {
    fn default() -> Self {
        Self::Running
    }
}

impl PipelineRunState {
    /// A short identifier for the frontend and tray label.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::PausedByUser => "paused",
            Self::AuthRequired { .. } => "auth_required",
        }
    }

    /// Automatic local stages (transcribe, summarize, export) may run. Only
    /// an explicit user pause stops those — an expired SharePoint session
    /// says nothing about work already downloaded. Meetings the user queued
    /// with "Process now" bypass even the pause (see the orchestrator loop).
    pub fn allows_local_work(&self) -> bool {
        !matches!(self, Self::PausedByUser)
    }

    /// SharePoint scanning may run.
    pub fn allows_scanning(&self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSettings {
    /// Master switch for all automatic processing.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// How often to scan SharePoint for new recordings.
    #[serde(default = "default_scan_interval")]
    pub scan_interval_minutes: u64,

    /// How long the machine must be free of keyboard/mouse input before
    /// local (on-device Whisper) transcription starts.
    #[serde(default = "default_idle_minutes")]
    pub idle_minutes: u64,

    /// Meetings younger than this are left alone, so the pipeline never
    /// races a live recording that just stopped or an in-flight recovery.
    #[serde(default = "default_grace_minutes")]
    pub grace_minutes: i64,

    /// Summary template and chunking, previously hard-coded in the frontend
    /// pending-work panel.
    #[serde(default = "default_template_id")]
    pub summary_template_id: String,
    #[serde(default = "default_chunk_size")]
    pub summary_chunk_size: i32,
    #[serde(default = "default_overlap")]
    pub summary_overlap: i32,

    /// Identify speakers when the pipeline transcribes a meeting. On-device
    /// transcription needs the speaker model downloaded; without it the
    /// meeting is transcribed without speakers instead of failing.
    #[serde(default = "default_true")]
    pub diarization: bool,

    /// Consecutive hard failures before a meeting is left for manual retry.
    #[serde(default = "default_max_attempts")]
    pub max_attempts: i64,

    /// Persisted so pause / auth-required survive a restart.
    #[serde(default)]
    pub run_state: PipelineRunState,

    /// RFC3339 timestamp of the last completed SharePoint scan.
    #[serde(default)]
    pub last_scan_at: Option<String>,
}

impl Default for PipelineSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            scan_interval_minutes: default_scan_interval(),
            idle_minutes: default_idle_minutes(),
            grace_minutes: default_grace_minutes(),
            summary_template_id: default_template_id(),
            summary_chunk_size: default_chunk_size(),
            summary_overlap: default_overlap(),
            diarization: default_true(),
            max_attempts: default_max_attempts(),
            run_state: PipelineRunState::default(),
            last_scan_at: None,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_scan_interval() -> u64 {
    30
}
fn default_idle_minutes() -> u64 {
    5
}
fn default_grace_minutes() -> i64 {
    10
}
fn default_template_id() -> String {
    "standard_meeting".to_string()
}
fn default_chunk_size() -> i32 {
    40000
}
fn default_overlap() -> i32 {
    1000
}
fn default_max_attempts() -> i64 {
    3
}

pub async fn load_settings<R: Runtime>(app: &AppHandle<R>) -> Result<PipelineSettings> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| anyhow!("Failed to access pipeline settings store: {}", e))?;

    if let Some(value) = store.get(STORE_KEY) {
        // A malformed or partial file must not disable the pipeline; fall
        // back to defaults for anything that will not parse.
        return Ok(serde_json::from_value::<PipelineSettings>(value.clone())
            .unwrap_or_else(|e| {
                log::warn!("Pipeline settings unreadable ({}), using defaults", e);
                PipelineSettings::default()
            }));
    }

    Ok(PipelineSettings::default())
}

pub async fn save_settings<R: Runtime>(app: &AppHandle<R>, settings: &PipelineSettings) -> Result<()> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| anyhow!("Failed to access pipeline settings store: {}", e))?;
    let value = serde_json::to_value(settings)
        .map_err(|e| anyhow!("Failed to serialize pipeline settings: {}", e))?;
    store.set(STORE_KEY, value);
    store
        .save()
        .map_err(|e| anyhow!("Failed to save pipeline settings: {}", e))?;
    Ok(())
}

/// Persist just the run state, leaving the rest of the settings untouched.
pub async fn save_run_state<R: Runtime>(app: &AppHandle<R>, state: &PipelineRunState) {
    match load_settings(app).await {
        Ok(mut settings) => {
            settings.run_state = state.clone();
            if let Err(e) = save_settings(app, &settings).await {
                log::warn!("Failed to persist pipeline run state: {}", e);
            }
        }
        Err(e) => log::warn!("Failed to load pipeline settings to persist run state: {}", e),
    }
}

/// Persist the scan watermark after a completed scan.
pub async fn save_last_scan_at<R: Runtime>(app: &AppHandle<R>, when: &str) {
    match load_settings(app).await {
        Ok(mut settings) => {
            settings.last_scan_at = Some(when.to_string());
            if let Err(e) = save_settings(app, &settings).await {
                log::warn!("Failed to persist pipeline scan watermark: {}", e);
            }
        }
        Err(e) => log::warn!("Failed to load pipeline settings to persist scan time: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // A settings file written by an older build must still load.
        let settings: PipelineSettings = serde_json::from_str(r#"{"idle_minutes": 12}"#).unwrap();
        assert_eq!(settings.idle_minutes, 12);
        assert!(settings.enabled);
        assert!(settings.diarization);
        assert_eq!(settings.summary_template_id, "standard_meeting");
        assert_eq!(settings.run_state, PipelineRunState::Running);
    }

    #[test]
    fn auth_required_state_round_trips() {
        let state = PipelineRunState::AuthRequired {
            host: "contoso-my.sharepoint.com".to_string(),
            since: "2026-07-26T09:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<PipelineRunState>(&json).unwrap(), state);
    }

    #[test]
    fn auth_required_blocks_scanning_but_not_local_work() {
        let state = PipelineRunState::AuthRequired {
            host: "h".to_string(),
            since: "t".to_string(),
        };
        assert!(!state.allows_scanning());
        assert!(state.allows_local_work());

        assert!(!PipelineRunState::PausedByUser.allows_local_work());
        assert!(PipelineRunState::Running.allows_scanning());
    }
}
