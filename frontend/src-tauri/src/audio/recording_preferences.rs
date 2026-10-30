use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use anyhow::Result;
#[cfg(target_os = "macos")]
use log::error;

#[cfg(target_os = "macos")]
use crate::audio::capture::AudioCaptureBackend;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecordingPreferences {
    pub save_folder: PathBuf,
    pub auto_save: bool,
    pub file_format: String,
    /// Microphone, then system audio: 0 is legacy, 1 is an exact ID.
    #[serde(default)]
    pub device_selection_version: [u8; 2],
    #[serde(skip)]
    unresolved_devices: [bool; 2],
    #[serde(skip)]
    migrated_from: [Option<String>; 2],
    #[serde(default)]
    pub preferred_mic_device: Option<String>,
    #[serde(default)]
    pub preferred_system_device: Option<String>,
    #[cfg(target_os = "macos")]
    #[serde(default)]
    pub system_audio_backend: Option<String>,
}

impl Default for RecordingPreferences {
    fn default() -> Self {
        Self {
            save_folder: get_default_recordings_folder(),
            auto_save: true,
            file_format: "mp4".to_string(),
            device_selection_version: [1; 2],
            unresolved_devices: [false; 2],
            migrated_from: [None, None],
            preferred_mic_device: None,
            preferred_system_device: None,
            #[cfg(target_os = "macos")]
            system_audio_backend: Some("coreaudio".to_string()),
        }
    }
}

// Legacy selections appended a role suffix; current IDs must remain literal.
fn normalize_device_preferences(
    prefs: &mut RecordingPreferences,
    devices: &[super::devices::AudioDevice],
) {
    use super::devices::DeviceType;
    for (index, (selection, role, suffix)) in [
        (
            &mut prefs.preferred_mic_device,
            DeviceType::Input,
            " (input)",
        ),
        (
            &mut prefs.preferred_system_device,
            DeviceType::Output,
            " (output)",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        prefs.unresolved_devices[index] = false;
        if prefs.device_selection_version[index] != 0 {
            continue;
        }
        if let Some(id) = selection {
            let exists = |name: &str| {
                devices
                    .iter()
                    .any(|d| d.device_type == role && d.name == name)
            };
            if exists(id) {
                prefs.device_selection_version[index] = 1;
                continue;
            }
            if let Some(raw) = id.strip_suffix(suffix).filter(|raw| exists(raw)) {
                prefs.migrated_from[index] = Some(id.clone());
                *id = raw.to_string();
            } else {
                prefs.unresolved_devices[index] = true;
            }
        }
        if !prefs.unresolved_devices[index] {
            prefs.device_selection_version[index] = 1;
        }
    }
}

impl RecordingPreferences {
    pub(super) fn validate_device_selection(
        &self,
        mic: Option<&str>,
        system: Option<&str>,
    ) -> Result<(), String> {
        for (index, (requested, saved, role)) in [
            (mic, self.preferred_mic_device.as_deref(), "microphone"),
            (
                system,
                self.preferred_system_device.as_deref(),
                "system-audio",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if self.unresolved_devices[index] && requested.is_some() && requested == saved {
                return Err(format!("Your saved {} selection needs updating. Choose a device in Recording Settings.", role));
            }
        }
        Ok(())
    }
}

pub(super) async fn resolve_start_devices(
    prefs: &RecordingPreferences,
    mic: Option<String>,
    system: Option<String>,
) -> Result<(Option<String>, Option<String>), String> {
    use super::devices::DeviceType;
    let mut requested = [mic, system];
    for (index, role) in [DeviceType::Input, DeviceType::Output]
        .into_iter()
        .enumerate()
    {
        if requested[index].is_some() && requested[index] == prefs.migrated_from[index] {
            requested[index] = if index == 0 {
                prefs.preferred_mic_device.clone()
            } else {
                prefs.preferred_system_device.clone()
            };
        }
        let suffix = if index == 0 { " (input)" } else { " (output)" };
        // A cached legacy argument may outlive migration by another preferences reader.
        if let Some(id) = requested[index]
            .as_deref()
            .filter(|id| id.ends_with(suffix))
        {
            let devices = super::devices::list_audio_devices()
                .await
                .map_err(|e| e.to_string())?;
            if !devices
                .iter()
                .any(|d| d.device_type == role && d.name == id)
            {
                return Err("Your saved audio selection needs updating. Choose a device in Recording Settings.".into());
            }
        }
    }
    prefs.validate_device_selection(requested[0].as_deref(), requested[1].as_deref())?;
    let [mic, system] = requested;
    Ok((mic, system))
}

async fn normalize_legacy_preferences(prefs: &mut RecordingPreferences) {
    if prefs.device_selection_version.contains(&0) {
        match super::devices::list_audio_devices().await {
            Ok(devices) => normalize_device_preferences(prefs, &devices),
            Err(e) => {
                warn!("Cannot resolve legacy device preferences: {}", e);
                prefs.unresolved_devices = [
                    prefs.device_selection_version[0] == 0 && prefs.preferred_mic_device.is_some(),
                    prefs.device_selection_version[1] == 0
                        && prefs.preferred_system_device.is_some(),
                ];
            }
        }
    }
}

/// Get the default recordings folder based on platform
pub fn get_default_recordings_folder() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        // Windows: %USERPROFILE%\Music\meetily-recordings
        if let Some(music_dir) = dirs::audio_dir() {
            music_dir.join("meetily-recordings")
        } else {
            // Fallback to Documents if Music folder is not available
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("meetily-recordings")
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: ~/Movies/meetily-recordings
        if let Some(movies_dir) = dirs::video_dir() {
            movies_dir.join("meetily-recordings")
        } else {
            // Fallback to Documents if Movies folder is not available
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("meetily-recordings")
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux/Others: ~/Documents/meetily-recordings
        dirs::document_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("meetily-recordings")
    }
}

/// Ensure the recordings directory exists
pub fn ensure_recordings_directory(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
        info!("Created recordings directory: {:?}", path);
    }
    Ok(())
}

/// Generate a unique filename for a recording
pub fn generate_recording_filename(format: &str) -> String {
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y%m%d_%H%M%S");
    format!("recording_{}.{}", timestamp, format)
}

/// Load recording preferences from store
pub async fn load_recording_preferences<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<RecordingPreferences> {
    // Try to load from Tauri store
    let store = match app.store("recording_preferences.json") {
        Ok(store) => store,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to access recording preferences: {}",
                e
            ));
        }
    };

    // Try to get the preferences from store
    let mut prefs = if let Some(value) = store.get("preferences") {
        match serde_json::from_value::<RecordingPreferences>(value.clone()) {
            Ok(mut p) => {
                info!("Loaded recording preferences from store");
                // Update macOS backend to current value if needed
                #[cfg(target_os = "macos")]
                {
                    let backend = crate::audio::capture::get_current_backend();
                    p.system_audio_backend = Some(backend.to_string());
                }
                p
            }
            Err(e) => {
                warn!("Failed to deserialize preferences: {}, using defaults", e);
                RecordingPreferences::default()
            }
        }
    } else {
        info!("No stored preferences found, using defaults");
        RecordingPreferences::default()
    };

    if prefs.device_selection_version.contains(&0) {
        let previous = serde_json::to_value(&prefs)?;
        normalize_legacy_preferences(&mut prefs).await;
        let migrated = serde_json::to_value(&prefs)?;
        if previous != migrated {
            store.set("preferences", migrated);
            if let Err(e) = store.save() {
                store.set("preferences", previous);
                return Err(e.into());
            }
        }
    }

    info!("Loaded recording preferences: save_folder={:?}, auto_save={}, format={}, mic={:?}, system={:?}",
          prefs.save_folder, prefs.auto_save, prefs.file_format,
          prefs.preferred_mic_device, prefs.preferred_system_device);
    Ok(prefs)
}

/// Save recording preferences to store
pub async fn save_recording_preferences<R: Runtime>(
    app: &AppHandle<R>,
    preferences: &RecordingPreferences,
) -> Result<()> {
    info!("Saving recording preferences: save_folder={:?}, auto_save={}, format={}, mic={:?}, system={:?}",
          preferences.save_folder, preferences.auto_save, preferences.file_format,
          preferences.preferred_mic_device, preferences.preferred_system_device);

    // Get or create store
    let store = app
        .store("recording_preferences.json")
        .map_err(|e| anyhow::anyhow!("Failed to access store: {}", e))?;

    let mut normalized = preferences.clone();
    if let Some(value) = store.get("preferences") {
        if let Ok(saved) = serde_json::from_value::<RecordingPreferences>(value) {
            for (index, unchanged) in [
                saved.preferred_mic_device == normalized.preferred_mic_device,
                saved.preferred_system_device == normalized.preferred_system_device,
            ]
            .into_iter()
            .enumerate()
            {
                if unchanged && saved.device_selection_version[index] == 1 {
                    normalized.device_selection_version[index] = 1;
                }
            }
        }
    }
    normalize_legacy_preferences(&mut normalized).await;
    let preferences = &normalized;

    // Serialize preferences to JSON value
    let prefs_value = serde_json::to_value(preferences)
        .map_err(|e| anyhow::anyhow!("Failed to serialize preferences: {}", e))?;

    // Save to store
    let previous = store.get("preferences");
    store.set("preferences", prefs_value);

    // Persist to disk
    if let Err(e) = store.save() {
        if let Some(previous) = previous {
            store.set("preferences", previous);
        } else {
            store.delete("preferences");
        }
        return Err(anyhow::anyhow!("Failed to save store to disk: {}", e));
    }

    info!("Successfully persisted recording preferences to disk");

    // Save backend preference to global config
    #[cfg(target_os = "macos")]
    if let Some(backend_str) = &preferences.system_audio_backend {
        if let Some(backend) = AudioCaptureBackend::from_string(backend_str) {
            info!("Setting audio capture backend to: {:?}", backend);
            crate::audio::capture::set_current_backend(backend);
        }
    }

    // Ensure the directory exists
    ensure_recordings_directory(&preferences.save_folder)?;

    Ok(())
}

/// Tauri commands for recording preferences
#[tauri::command]
pub async fn get_recording_preferences<R: Runtime>(
    app: AppHandle<R>,
) -> Result<RecordingPreferences, String> {
    load_recording_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load recording preferences: {}", e))
}

#[tauri::command]
pub async fn set_recording_preferences<R: Runtime>(
    app: AppHandle<R>,
    preferences: RecordingPreferences,
) -> Result<(), String> {
    save_recording_preferences(&app, &preferences)
        .await
        .map_err(|e| format!("Failed to save recording preferences: {}", e))
}

#[tauri::command]
pub async fn get_default_recordings_folder_path() -> Result<String, String> {
    let path = get_default_recordings_folder();
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_recordings_folder<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let preferences = load_recording_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load preferences: {}", e))?;

    // Ensure directory exists before trying to open it
    ensure_recordings_directory(&preferences.save_folder)
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    let folder_path = preferences.save_folder.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    info!("Opened recordings folder: {}", folder_path);
    Ok(())
}

#[tauri::command]
pub async fn select_recording_folder<R: Runtime>(
    _app: AppHandle<R>,
) -> Result<Option<String>, String> {
    // Use Tauri's dialog to select folder
    // For now, return None - this would need to be implemented with tauri-plugin-dialog
    // when it's available in the Cargo.toml
    warn!("Folder selection not yet implemented - using dialog plugin");
    Ok(None)
}

// Backend selection commands

/// Get available audio capture backends for the current platform
#[tauri::command]
pub async fn get_available_audio_backends() -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let backends = crate::audio::capture::get_available_backends();
        Ok(backends.iter().map(|b| b.to_string()).collect())
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Only ScreenCaptureKit available on non-macOS
        Ok(vec!["screencapturekit".to_string()])
    }
}

/// Get current audio capture backend
#[tauri::command]
pub async fn get_current_audio_backend() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let backend = crate::audio::capture::get_current_backend();
        Ok(backend.to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok("screencapturekit".to_string())
    }
}

/// Set audio capture backend
#[tauri::command]
pub async fn set_audio_backend(backend: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use crate::audio::capture::AudioCaptureBackend;
        use crate::audio::permissions::{
            check_screen_recording_permission, request_screen_recording_permission,
        };

        let backend_enum = AudioCaptureBackend::from_string(&backend)
            .ok_or_else(|| format!("Invalid backend: {}", backend))?;

        // If switching to Core Audio, log information about Audio Capture permission
        if backend_enum == AudioCaptureBackend::CoreAudio {
            info!("🔐 Core Audio backend requires Audio Capture permission (macOS 14.4+)");
            info!("📍 Permission dialog will appear automatically when recording starts");

            // Check if permission is already granted (this is informational only)
            if !check_screen_recording_permission() {
                warn!("⚠️  Audio Capture permission may not be granted");

                // Attempt to open System Settings (opens System Settings)
                if let Err(e) = request_screen_recording_permission() {
                    error!("Failed to open System Settings: {}", e);
                }

                return Err(
                    "Core Audio requires Audio Capture permission. \
                    The permission dialog will appear when you start recording. \
                    If already denied, enable it in System Settings → Privacy & Security → Audio Capture, \
                    then restart the app.".to_string()
                );
            }

            info!(
                "✅ Core Audio backend selected - permission check will occur at recording start"
            );
        }

        info!("Setting audio backend to: {:?}", backend_enum);
        crate::audio::capture::set_current_backend(backend_enum);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        if backend != "screencapturekit" {
            return Err(format!(
                "Backend {} not available on this platform",
                backend
            ));
        }
        Ok(())
    }
}

/// Get backend information (name and description)
#[derive(Serialize)]
pub struct BackendInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[tauri::command]
pub async fn get_audio_backend_info() -> Result<Vec<BackendInfo>, String> {
    #[cfg(target_os = "macos")]
    {
        use crate::audio::capture::AudioCaptureBackend;

        let backends = vec![
            BackendInfo {
                id: AudioCaptureBackend::ScreenCaptureKit.to_string(),
                name: AudioCaptureBackend::ScreenCaptureKit.name().to_string(),
                description: AudioCaptureBackend::ScreenCaptureKit
                    .description()
                    .to_string(),
            },
            BackendInfo {
                id: AudioCaptureBackend::CoreAudio.to_string(),
                name: AudioCaptureBackend::CoreAudio.name().to_string(),
                description: AudioCaptureBackend::CoreAudio.description().to_string(),
            },
        ];
        Ok(backends)
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(vec![BackendInfo {
            id: "screencapturekit".to_string(),
            name: "ScreenCaptureKit".to_string(),
            description: "Default system audio capture".to_string(),
        }])
    }
}

#[cfg(test)]
mod device_preference_tests {
    use super::*;
    use crate::audio::devices::{AudioDevice, DeviceType};

    fn mic(id: &str) -> AudioDevice {
        AudioDevice::new(id.into(), DeviceType::Input)
    }

    #[tokio::test]
    async fn just_migrated_explicit_argument_uses_selected_device_and_default_stays_default() {
        let mut prefs = RecordingPreferences::default();
        prefs.device_selection_version = [0; 2];
        prefs.preferred_mic_device = Some("USB Mic (input)".into());
        normalize_device_preferences(&mut prefs, &[mic("USB Mic")]);
        let (selected, _) = resolve_start_devices(&prefs, Some("USB Mic (input)".into()), None)
            .await
            .unwrap();
        assert_eq!(selected.as_deref(), Some("USB Mic"));
        assert_eq!(
            resolve_start_devices(&prefs, None, None).await.unwrap(),
            (None, None)
        );
    }

    #[test]
    fn resolved_role_is_not_reinterpreted_when_other_role_is_still_legacy() {
        let mut prefs = RecordingPreferences::default();
        prefs.device_selection_version = [0; 2];
        prefs.preferred_mic_device = Some("USB (input) (input)".into());
        prefs.preferred_system_device = Some("missing (output)".into());
        normalize_device_preferences(&mut prefs, &[mic("USB (input)")]);
        assert_eq!(prefs.device_selection_version, [1, 0]);
        let mut reloaded: RecordingPreferences =
            serde_json::from_value(serde_json::to_value(prefs).unwrap()).unwrap();
        normalize_device_preferences(&mut reloaded, &[mic("USB")]);
        assert_eq!(
            reloaded.preferred_mic_device.as_deref(),
            Some("USB (input)")
        );
    }

    #[test]
    fn absent_version_is_legacy_and_partial_resolution_survives_roundtrip() {
        let mut value = serde_json::to_value(RecordingPreferences::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("device_selection_version");
        value["preferred_mic_device"] = serde_json::json!("USB Mic (input)");
        value["preferred_system_device"] = serde_json::json!("missing (output)");
        let mut prefs: RecordingPreferences = serde_json::from_value(value).unwrap();
        assert_eq!(prefs.device_selection_version[0], 0);
        normalize_device_preferences(&mut prefs, &[mic("USB Mic")]);
        assert_eq!(prefs.preferred_mic_device.as_deref(), Some("USB Mic"));
        assert_eq!(prefs.device_selection_version, [1, 0]);
        assert!(prefs
            .validate_device_selection(Some("USB Mic"), Some("missing (output)"))
            .is_err());
        prefs.auto_save = false;
        let mut reloaded: RecordingPreferences =
            serde_json::from_value(serde_json::to_value(prefs).unwrap()).unwrap();
        normalize_device_preferences(&mut reloaded, &[mic("USB Mic")]);
        assert_eq!(
            reloaded.preferred_system_device.as_deref(),
            Some("missing (output)")
        );
        assert!(reloaded
            .validate_device_selection(None, Some("missing (output)"))
            .is_err());
    }

    #[test]
    fn wrong_role_or_empty_inventory_cannot_resolve_legacy_choice() {
        let mut prefs = RecordingPreferences::default();
        prefs.device_selection_version = [0; 2];
        prefs.preferred_mic_device = Some("USB (input)".into());
        normalize_device_preferences(
            &mut prefs,
            &[AudioDevice::new("USB".into(), DeviceType::Output)],
        );
        assert_eq!(prefs.device_selection_version[0], 0);
        normalize_device_preferences(&mut prefs, &[]);
        assert!(prefs
            .validate_device_selection(Some("USB (input)"), None)
            .is_err());
    }

    #[test]
    fn legacy_selection_keeps_the_selected_mic_instead_of_default() {
        let mut prefs = RecordingPreferences::default();
        prefs.device_selection_version = [0; 2];
        prefs.preferred_mic_device = Some("USB Mic (input)".into());
        normalize_device_preferences(&mut prefs, &[mic("Laptop Mic"), mic("USB Mic")]);
        assert_eq!(prefs.preferred_mic_device.as_deref(), Some("USB Mic"));
        assert_eq!(prefs.device_selection_version, [1; 2]);
    }

    #[test]
    fn exact_ids_win_and_current_ids_are_never_suffix_parsed() {
        for version in [0, 1] {
            let mut prefs = RecordingPreferences::default();
            prefs.device_selection_version = [version; 2];
            prefs.preferred_mic_device = Some("USB (input)".into());
            normalize_device_preferences(&mut prefs, &[mic("USB"), mic("USB (input)")]);
            assert_eq!(prefs.preferred_mic_device.as_deref(), Some("USB (input)"));
        }
        let mut prefs = RecordingPreferences::default();
        prefs.preferred_mic_device = Some("USB (input)".into());
        normalize_device_preferences(&mut prefs, &[mic("USB")]);
        assert_eq!(prefs.preferred_mic_device.as_deref(), Some("USB (input)"));
    }

    #[test]
    fn unresolved_legacy_choice_survives_and_blocks_only_that_choice() {
        let mut prefs = RecordingPreferences::default();
        prefs.device_selection_version = [0; 2];
        prefs.preferred_mic_device = Some("Old ALSA (input)".into());
        let devices = vec![mic("pulse-source")];
        normalize_device_preferences(&mut prefs, &devices);
        assert_eq!(prefs.device_selection_version[0], 0);
        assert_eq!(
            prefs.preferred_mic_device.as_deref(),
            Some("Old ALSA (input)")
        );
        assert!(prefs
            .validate_device_selection(Some("Old ALSA (input)"), None)
            .is_err());
        assert!(prefs.validate_device_selection(None, None).is_ok());
        assert!(prefs
            .validate_device_selection(Some("pulse-source"), None)
            .is_ok());
        prefs.preferred_mic_device = None;
        normalize_device_preferences(&mut prefs, &devices);
        assert_eq!(prefs.device_selection_version, [1; 2]);
    }
}
