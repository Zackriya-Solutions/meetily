use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const APP_PREFERENCES_STORE: &str = "app_preferences.json";
const APP_PREFERENCES_KEY: &str = "preferences";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptCleanupSettings {
    pub enabled: bool,
    pub remove_fillers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppPreferences {
    pub auto_export_markdown_on_finalize: bool,
    pub transcript_cleanup: TranscriptCleanupSettings,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            auto_export_markdown_on_finalize: false,
            transcript_cleanup: TranscriptCleanupSettings {
                enabled: true,
                remove_fillers: true,
            },
        }
    }
}

impl AppPreferences {
    pub fn sanitized(self) -> Self {
        self
    }
}

pub async fn load_app_preferences<R: Runtime>(app: &AppHandle<R>) -> Result<AppPreferences> {
    let store = match app.store(APP_PREFERENCES_STORE) {
        Ok(store) => store,
        Err(error) => {
            warn!(
                "Failed to access app preferences store: {}. Using defaults.",
                error
            );
            return Ok(AppPreferences::default());
        }
    };

    if let Some(value) = store.get(APP_PREFERENCES_KEY) {
        match serde_json::from_value::<AppPreferences>(value.clone()) {
            Ok(preferences) => Ok(preferences.sanitized()),
            Err(error) => {
                warn!(
                    "Failed to deserialize app preferences: {}. Using defaults.",
                    error
                );
                Ok(AppPreferences::default())
            }
        }
    } else {
        Ok(AppPreferences::default())
    }
}

pub async fn save_app_preferences<R: Runtime>(
    app: &AppHandle<R>,
    preferences: &AppPreferences,
) -> Result<AppPreferences> {
    let sanitized = preferences.clone().sanitized();
    let store = app
        .store(APP_PREFERENCES_STORE)
        .map_err(|e| anyhow::anyhow!("Failed to access app preferences store: {}", e))?;

    let value = serde_json::to_value(&sanitized)
        .map_err(|e| anyhow::anyhow!("Failed to serialize app preferences: {}", e))?;
    store.set(APP_PREFERENCES_KEY, value);
    store
        .save()
        .map_err(|e| anyhow::anyhow!("Failed to persist app preferences: {}", e))?;

    info!(
        "App preferences saved: auto_export_markdown_on_finalize={}, cleanup_enabled={}, remove_fillers={}",
        sanitized.auto_export_markdown_on_finalize,
        sanitized.transcript_cleanup.enabled,
        sanitized.transcript_cleanup.remove_fillers
    );

    Ok(sanitized)
}

#[tauri::command]
pub async fn get_app_preferences<R: Runtime>(app: AppHandle<R>) -> Result<AppPreferences, String> {
    load_app_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load app preferences: {}", e))
}

#[tauri::command]
pub async fn set_app_preferences<R: Runtime>(
    app: AppHandle<R>,
    preferences: AppPreferences,
) -> Result<AppPreferences, String> {
    save_app_preferences(&app, &preferences)
        .await
        .map_err(|e| format!("Failed to save app preferences: {}", e))
}
