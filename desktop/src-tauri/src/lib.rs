use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
// Removed unused import

// Performance optimization: Conditional logging macros for hot paths
#[cfg(debug_assertions)]
macro_rules! perf_debug {
    ($($arg:tt)*) => {
        log::debug!($($arg)*)
    };
}

#[cfg(not(debug_assertions))]
macro_rules! perf_debug {
    ($($arg:tt)*) => {};
}

#[cfg(debug_assertions)]
macro_rules! perf_trace {
    ($($arg:tt)*) => {
        log::trace!($($arg)*)
    };
}

#[cfg(not(debug_assertions))]
macro_rules! perf_trace {
    ($($arg:tt)*) => {};
}

// Make these macros available to other modules

// Re-export async logging macros for external use (removed due to macro conflicts)

// Declare audio module
#[macro_use]
pub mod command_registry;
pub mod anthropic;
pub mod api;
pub mod audio;
pub mod bootstrap;
pub mod brand;
pub mod config;
pub mod console_utils;
pub mod database;
pub mod groq;
pub mod markdown_export;
pub mod notifications;
pub mod ollama;
pub mod onboarding;
pub mod openai;
pub mod openrouter;
pub mod parakeet_engine;
pub mod preferences;
pub mod state;
pub mod summary;
pub mod transcript_processing;
pub mod tray;
pub mod utils;
pub mod vocabulary;
pub mod whisper_engine;

use audio::{list_audio_devices, trigger_audio_permission, AudioDevice};
use log::{error as log_error, info as log_info};
use notifications::commands::NotificationManagerState;
use tauri::{AppHandle, Manager, Runtime};

static RECORDING_FLAG: AtomicBool = AtomicBool::new(false);

// Global language preference storage (default to "auto-translate" for automatic translation to English)
static LANGUAGE_PREFERENCE: std::sync::LazyLock<StdMutex<String>> =
    std::sync::LazyLock::new(|| StdMutex::new("auto-translate".to_string()));

#[derive(Debug, Deserialize)]
struct RecordingArgs {
    save_path: String,
}

#[derive(Debug, Serialize, Clone)]
struct TranscriptionStatus {
    chunks_in_queue: usize,
    is_processing: bool,
    last_activity_ms: u64,
}

#[tauri::command]
async fn start_recording<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    log_info!("start_recording called with meeting: {:?}", meeting_name);
    log_info!(
        "Backend received parameters - mic: {:?}, system: {:?}, meeting: {:?}",
        mic_device_name,
        system_device_name,
        meeting_name
    );

    if is_recording().await {
        return Err("Recording already in progress".to_string());
    }

    // Call the actual audio recording system with meeting name
    match audio::recording_commands::start_recording_with_devices_and_meeting(
        app.clone(),
        mic_device_name,
        system_device_name,
        meeting_name.clone(),
    )
    .await
    {
        Ok(_) => {
            RECORDING_FLAG.store(true, Ordering::SeqCst);
            tray::update_tray_menu(&app);

            log_info!("Recording started successfully");

            // Show recording started notification through NotificationManager
            // This respects user's notification preferences
            let notification_manager_state = app.state::<NotificationManagerState<R>>();
            if let Err(e) = notifications::commands::show_recording_started_notification(
                &app,
                &notification_manager_state,
                meeting_name.clone(),
            )
            .await
            {
                log_error!("Failed to show recording started notification: {}", e);
            } else {
                log_info!("Successfully showed recording started notification");
            }

            Ok(())
        }
        Err(e) => {
            log_error!("Failed to start audio recording: {}", e);
            Err(format!("Failed to start recording: {}", e))
        }
    }
}

#[tauri::command]
async fn stop_and_finalize_recording<R: Runtime>(
    app: AppHandle<R>,
    args: RecordingArgs,
) -> Result<audio::recording_commands::MeetingFinalizationResult, String> {
    log_info!("Attempting to stop recording...");

    match audio::recording_commands::stop_and_finalize_recording(
        app.clone(),
        audio::recording_commands::RecordingArgs {
            save_path: args.save_path.clone(),
        },
    )
    .await
    {
        Ok(finalization_result) => {
            RECORDING_FLAG.store(false, Ordering::SeqCst);
            tray::update_tray_menu(&app);

            // Show recording stopped notification through NotificationManager
            // This respects user's notification preferences
            let notification_manager_state = app.state::<NotificationManagerState<R>>();
            if let Err(e) = notifications::commands::show_recording_stopped_notification(
                &app,
                &notification_manager_state,
            )
            .await
            {
                log_error!("Failed to show recording stopped notification: {}", e);
            } else {
                log_info!("Successfully showed recording stopped notification");
            }

            Ok(finalization_result)
        }
        Err(e) => {
            log_error!("Failed to stop audio recording: {}", e);
            // Still update the flag even if stopping failed
            RECORDING_FLAG.store(false, Ordering::SeqCst);
            tray::update_tray_menu(&app);
            Err(format!("Failed to stop recording: {}", e))
        }
    }
}

#[tauri::command]
async fn stop_recording<R: Runtime>(app: AppHandle<R>, args: RecordingArgs) -> Result<(), String> {
    // Deprecated compatibility wrapper. Prefer `stop_and_finalize_recording`.
    stop_and_finalize_recording(app, args).await.map(|_| ())
}

#[tauri::command]
async fn is_recording() -> bool {
    audio::recording_commands::is_recording().await
}

#[tauri::command]
fn get_transcription_status() -> TranscriptionStatus {
    TranscriptionStatus {
        chunks_in_queue: 0,
        is_processing: false,
        last_activity_ms: 0,
    }
}

#[tauri::command]
fn read_audio_file(file_path: String) -> Result<Vec<u8>, String> {
    match std::fs::read(&file_path) {
        Ok(data) => Ok(data),
        Err(e) => Err(format!("Failed to read audio file: {}", e)),
    }
}

#[tauri::command]
async fn save_transcript(file_path: String, content: String) -> Result<(), String> {
    log_info!("Saving transcript to: {}", file_path);

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }
    }

    // Write content to file
    std::fs::write(&file_path, content)
        .map_err(|e| format!("Failed to write transcript: {}", e))?;

    log_info!("Transcript saved successfully");
    Ok(())
}

// Audio level monitoring commands
#[tauri::command]
async fn start_audio_level_monitoring<R: Runtime>(
    app: AppHandle<R>,
    device_names: Vec<String>,
) -> Result<(), String> {
    log_info!(
        "Starting audio level monitoring for devices: {:?}",
        device_names
    );

    audio::simple_level_monitor::start_monitoring(app, device_names)
        .await
        .map_err(|e| format!("Failed to start audio level monitoring: {}", e))
}

#[tauri::command]
async fn stop_audio_level_monitoring() -> Result<(), String> {
    log_info!("Stopping audio level monitoring");

    audio::simple_level_monitor::stop_monitoring()
        .await
        .map_err(|e| format!("Failed to stop audio level monitoring: {}", e))
}

#[tauri::command]
async fn is_audio_level_monitoring() -> bool {
    audio::simple_level_monitor::is_monitoring()
}

// Whisper commands are now handled by whisper_engine::commands module

#[tauri::command]
async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    list_audio_devices()
        .await
        .map_err(|e| format!("Failed to list audio devices: {}", e))
}

#[tauri::command]
async fn trigger_microphone_permission() -> Result<bool, String> {
    trigger_audio_permission()
        .map_err(|e| format!("Failed to trigger microphone permission: {}", e))
}

#[tauri::command]
async fn start_recording_with_devices<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
) -> Result<(), String> {
    start_recording_with_devices_and_meeting(app, mic_device_name, system_device_name, None).await
}

#[tauri::command]
async fn start_recording_with_devices_and_meeting<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    log_info!(
        "start_recording_with_devices_and_meeting called - Mic: {:?}, System: {:?}, Meeting: {:?}",
        mic_device_name,
        system_device_name,
        meeting_name
    );

    // Clone meeting_name for notification use later
    let meeting_name_for_notification = meeting_name.clone();

    // Call the recording module functions that support meeting names
    let recording_result = match (mic_device_name.clone(), system_device_name.clone()) {
        (None, None) => {
            log_info!(
                "No devices specified, starting with defaults and meeting: {:?}",
                meeting_name
            );
            audio::recording_commands::start_recording_with_meeting_name(app.clone(), meeting_name)
                .await
        }
        _ => {
            log_info!(
                "Starting with specified devices: mic={:?}, system={:?}, meeting={:?}",
                mic_device_name,
                system_device_name,
                meeting_name
            );
            audio::recording_commands::start_recording_with_devices_and_meeting(
                app.clone(),
                mic_device_name,
                system_device_name,
                meeting_name,
            )
            .await
        }
    };

    match recording_result {
        Ok(_) => {
            log_info!("Recording started successfully via tauri command");

            // Show recording started notification through NotificationManager
            // This respects user's notification preferences
            let notification_manager_state = app.state::<NotificationManagerState<R>>();
            if let Err(e) = notifications::commands::show_recording_started_notification(
                &app,
                &notification_manager_state,
                meeting_name_for_notification.clone(),
            )
            .await
            {
                log_error!("Failed to show recording started notification: {}", e);
            }

            Ok(())
        }
        Err(e) => {
            log_error!("Failed to start recording via tauri command: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
async fn set_language_preference(language: String) -> Result<(), String> {
    let mut lang_pref = LANGUAGE_PREFERENCE
        .lock()
        .map_err(|e| format!("Failed to set language preference: {}", e))?;
    log_info!("Setting language preference to: {}", language);
    *lang_pref = language;
    Ok(())
}

// Internal helper function to get language preference (for use within Rust code)
pub fn get_language_preference_internal() -> Option<String> {
    LANGUAGE_PREFERENCE.lock().ok().map(|lang| lang.clone())
}

pub fn run() {
    log::set_max_level(log::LevelFilter::Info);

    bootstrap::configure_builder(tauri::Builder::default())
        .setup(bootstrap::setup_app)
        .invoke_handler(app_invoke_handler!())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                log::info!("Application exiting, cleaning up resources...");
                tauri::async_runtime::block_on(bootstrap::cleanup_on_exit(app_handle));
                log::info!("Application cleanup complete");
            }
        });
}
