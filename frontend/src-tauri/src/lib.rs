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
pub(crate) use perf_debug;
pub(crate) use perf_trace;

// Re-export async logging macros for external use (removed due to macro conflicts)

// Declare audio module
/// On-device AI agents (follow-ups, action tracking) — DORMANT until EPIC F
/// (ADR-0013). Registered so it compiles and its tests run; NOT wired to any Tauri
/// command or the core path. Agents are off by default, produce human-approved
/// drafts only, and have no network dependency (see `agents::engine`).
pub mod agents;
pub mod analytics;
pub mod anthropic;
pub mod api;
/// "Ask This Meeting" (Product Intelligence slice 3): retrieve the meeting's
/// own passages first, answer only from those, and refuse rather than guess.
/// Every rendered claim is ground-checked against the retrieved window — a
/// citation to anything else is dropped, never repaired, because repairing it
/// would give an invented claim a real timestamp. Read-only: nothing here
/// persists or can become approved content.
pub mod ask;
pub mod audio;
pub mod config;
pub mod console_utils;
pub mod context;
/// Live Copilot — the private in-meeting panel (BACKLOG EPIC I, ADR-0038).
/// I1 is the shell: a window, a global shortcut and an honest screen-sharing
/// posture. No AI, no capture of its own, and OFF by default — with the flag
/// off nothing here registers a shortcut or creates a window.
pub mod copilot;
pub mod database;
pub mod diarization;
pub mod groq;
/// Local inference capability + backend selection (Tier-0 seam, DORMANT).
/// One offline picture of what this device can run — shared by STT and the
/// summary path, which until now had no hardware awareness at all — plus the
/// pure selection function the on-device OS-native runtimes (Apple Foundation
/// Models, Windows ONNX) slot into. Declared backends are structurally
/// unselectable until implemented. Nothing calls it yet; no behaviour changes.
pub mod inference;
/// Local learning (ADR-0030): the human-in-the-loop correction signal becomes
/// plain-language rules that shape the next summary. Learning is DATA, never
/// weights — an unwanted rule is one `DELETE`, which is what makes KVKK/GDPR
/// erasure satisfiable at all. Nothing leaves the device to make it work.
pub mod learning;
/// Licensing & trial (ADR-0023): Polar.sh license keys + client-side 14-day
/// trial. Local-first: status reads never touch the network, validation is
/// lazy/fail-open, and expiry gates ONLY new capture (recording/import) — never
/// existing data.
pub mod licensing;
/// Meeting modes (BACKLOG I4a, ADR-0038) — what kind of conversation this is,
/// which copilot actions suit it, how strictly it must cite, and what it may
/// read. Data and pure functions; registers nothing and changes no existing
/// summary behaviour.
pub mod modes;
pub mod notifications;
pub mod ollama;
pub mod onboarding;
pub mod openai;
pub mod openrouter;
pub mod parakeet_engine;
pub mod recording_consent;
/// Opt-in PII/keyword redaction (BACKLOG C6). OFF by default; when a workspace
/// enables it, transcript text is scrubbed before DB persistence and before it
/// reaches a summary LLM provider (docs/SECURITY_PRIVACY.md "Retention/redaction").
pub mod redaction;
pub mod secrets;
pub mod state;
pub mod summary;
/// Optional sync/collaboration client — DORMANT until Phase 2 (`docs/CONTRACTS.md`
/// §5, ADR-0012). Registered so it compiles and its tests run; NOT wired to any
/// Tauri command or the capture→transcript→summary→store path. Sync is off by
/// default and has no network dependency (see `sync::client`).
pub mod sync;
pub mod text;
pub mod tray;
pub mod utils;
pub mod whisper_engine;

use audio::{list_audio_devices, trigger_audio_permission, AudioDevice};
use log::{error as log_error, info as log_info};
use notifications::commands::NotificationManagerState;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::RwLock;

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

#[tauri::command(rename_all = "snake_case")]
async fn start_recording<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
    consent_ticket: String,
) -> Result<(), String> {
    log_info!(
        "start_recording called (microphone_selected={}, system_audio_selected={}, meeting_title_present={})",
        mic_device_name.is_some(),
        system_device_name.is_some(),
        meeting_name.is_some()
    );

    // Native consent authorization is single-use and fail-closed. It must be
    // consumed before any recording implementation can be entered.
    recording_consent::consume_recording_start_authorization(&app, &consent_ticket)?;

    // Licensing gate (ADR-0023 §5): TrialExpired/Revoked block starting a NEW
    // recording; Trial/Licensed pass. Existing data is never gated.
    licensing::commands::ensure_capture_allowed(&app).await?;

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
async fn stop_recording<R: Runtime>(
    app: AppHandle<R>,
    args: RecordingArgs,
) -> Result<bool, String> {
    log_info!("Attempting to stop recording...");

    // Check the actual audio recording system state instead of the flag
    if !audio::recording_commands::is_recording().await {
        log_info!("Recording is already stopped");
        return Ok(false);
    }

    // Call the actual audio recording system to stop
    match audio::recording_commands::stop_recording(
        app.clone(),
        audio::recording_commands::RecordingArgs {
            save_path: args.save_path.clone(),
        },
    )
    .await
    {
        Ok(false) => {
            log_info!("Recording stop is owned by another caller; skipping duplicate completion");
            Ok(false)
        }
        Ok(true) => {
            RECORDING_FLAG.store(false, Ordering::SeqCst);
            tray::update_tray_menu(&app);

            // Create the save directory if it doesn't exist
            if let Some(parent) = std::path::Path::new(&args.save_path).parent() {
                if !parent.exists() {
                    log_info!("Creating the managed recording directory");
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        // The authoritative native stop already completed and
                        // emitted its one-time completion token. This legacy
                        // caller path must not turn that success into a false
                        // frontend failure that strands post-processing.
                        log_error!("Failed to create the legacy save directory: {}", e);
                    }
                }
            }

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

            Ok(true)
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

// Analytics commands are now handled by analytics::commands module

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

#[tauri::command(rename_all = "snake_case")]
async fn start_recording_with_devices<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    consent_ticket: String,
) -> Result<(), String> {
    recording_consent::consume_recording_start_authorization(&app, &consent_ticket)?;
    licensing::commands::ensure_capture_allowed(&app).await?;

    start_recording_with_devices_and_meeting_authorized(
        app,
        mic_device_name,
        system_device_name,
        None,
    )
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn start_recording_with_devices_and_meeting<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
    consent_ticket: String,
) -> Result<(), String> {
    log_info!(
        "start_recording_with_devices_and_meeting called (microphone_selected={}, system_audio_selected={}, meeting_title_present={})",
        mic_device_name.is_some(),
        system_device_name.is_some(),
        meeting_name.is_some()
    );

    recording_consent::consume_recording_start_authorization(&app, &consent_ticket)?;

    // Licensing gate (ADR-0023 §5): blocks starting a NEW recording when the
    // trial expired or the license was revoked.
    licensing::commands::ensure_capture_allowed(&app).await?;

    start_recording_with_devices_and_meeting_authorized(
        app,
        mic_device_name,
        system_device_name,
        meeting_name,
    )
    .await
}

/// Shared implementation reached only after the Tauri entry point consumed a
/// native recording-consent ticket and passed the license gate.
async fn start_recording_with_devices_and_meeting_authorized<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    // Clone meeting_name for notification use later
    let meeting_name_for_notification = meeting_name.clone();

    // Call the recording module functions that support meeting names
    let recording_result = match (mic_device_name.clone(), system_device_name.clone()) {
        (None, None) => {
            log_info!("Starting recording with default devices");
            audio::recording_commands::start_recording_with_meeting_name(app.clone(), meeting_name)
                .await
        }
        _ => {
            log_info!("Starting recording with explicitly selected devices");
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
    let mut builder = tauri::Builder::default();

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log_info!("Second app instance requested");

            tray::focus_main_window(app);
        }));
    }

    // Global shortcuts for the copilot panel (BACKLOG I1). Registering the
    // plugin costs nothing on its own: it holds no shortcut until
    // `copilot::init` registers one, and it registers none while the copilot is
    // disabled (ADR-0038 invariant 4).
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }

    builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(whisper_engine::parallel_commands::ParallelProcessorState::new())
        .manage(Arc::new(RwLock::new(
            None::<notifications::manager::NotificationManager<tauri::Wry>>,
        )) as NotificationManagerState<tauri::Wry>)
        .manage(audio::init_system_audio_state())
        .manage(recording_consent::RecordingConsentTicketState::default())
        .manage(summary::summary_engine::ModelManagerState(Arc::new(
            tokio::sync::Mutex::new(None),
        )))
        .setup(|_app| {
            log::info!("Application setup complete");

            // Initialize system tray
            if let Err(e) = tray::create_tray(_app.handle()) {
                log::error!("Failed to create system tray: {}", e);
            }

            // Live copilot (BACKLOG I1): registers its global shortcut only if
            // the user enabled it. Default is off, so this is a no-op on a
            // fresh install.
            copilot::init(_app.handle());

            // Initialize notification system with proper defaults
            log::info!("Initializing notification system...");
            let app_for_notif = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let notif_state = app_for_notif.state::<NotificationManagerState<tauri::Wry>>();
                match notifications::commands::initialize_notification_manager(
                    app_for_notif.clone(),
                )
                .await
                {
                    Ok(manager) => {
                        // Set default consent and permissions on first launch
                        if let Err(e) = manager.set_consent(true).await {
                            log::error!("Failed to set initial consent: {}", e);
                        }
                        if let Err(e) = manager.request_permission().await {
                            log::error!("Failed to request initial permission: {}", e);
                        }

                        // Store the initialized manager
                        let mut state_lock = notif_state.write().await;
                        *state_lock = Some(manager);
                        log::info!("Notification system initialized with default permissions");
                    }
                    Err(e) => {
                        log::error!("Failed to initialize notification manager: {}", e);
                    }
                }
            });

            // Set models directory to use app_data_dir (unified storage location)
            whisper_engine::commands::set_models_directory(&_app.handle());

            // Initialize Whisper engine on startup, then warm the model scan once.
            // The warm scan ensures the engine is fully ready and its model cache
            // is populated before the user reaches Settings -> Transcription, which
            // previously could race and report already-downloaded models as
            // "Download" (empty scan) when the tab was opened too early.
            tauri::async_runtime::spawn(async {
                if let Err(e) = whisper_engine::commands::whisper_init().await {
                    log::error!("Failed to initialize Whisper engine on startup: {}", e);
                }
                let _ = whisper_engine::commands::whisper_get_available_models().await;
            });

            // Set Parakeet models directory
            parakeet_engine::commands::set_models_directory(&_app.handle());

            // Initialize Parakeet engine on startup
            tauri::async_runtime::spawn(async {
                if let Err(e) = parakeet_engine::commands::parakeet_init().await {
                    log::error!("Failed to initialize Parakeet engine on startup: {}", e);
                }
            });

            // Initialize ModelManager for summary engine (async, non-blocking)
            let app_handle_for_model_manager = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match summary::summary_engine::commands::init_model_manager_at_startup(
                    &app_handle_for_model_manager,
                )
                .await
                {
                    Ok(_) => log::info!("ModelManager initialized successfully at startup"),
                    Err(e) => {
                        log::warn!("Failed to initialize ModelManager at startup: {}", e);
                        log::warn!("ModelManager will be lazy-initialized on first use");
                    }
                }
            });

            // Trigger system audio permission request on startup (similar to microphone permission)
            // #[cfg(target_os = "macos")]
            // {
            //     tauri::async_runtime::spawn(async {
            //         if let Err(e) = audio::permissions::trigger_system_audio_permission() {
            //             log::warn!("Failed to trigger system audio permission: {}", e);
            //         }
            //     });
            // }

            // Initialize database (handles first launch detection and conditional setup)
            tauri::async_runtime::block_on(async {
                database::setup::initialize_database_on_startup(&_app.handle()).await
            })
            .expect("Failed to initialize database");

            // Initialize bundled templates directory for dynamic template discovery
            log::info!("Initializing bundled templates directory...");
            if let Ok(resource_path) = _app.handle().path().resource_dir() {
                let templates_dir = resource_path.join("templates");
                log::info!(
                    "Setting bundled templates directory to: {:?}",
                    templates_dir
                );
                summary::templates::set_bundled_templates_dir(templates_dir);
            } else {
                log::warn!("Failed to resolve resource directory for templates");
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        log::error!("Failed to hide main window on close request: {}", e);
                    } else {
                        log::info!("Main window hidden to tray on close request");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            is_recording,
            get_transcription_status,
            analytics::commands::init_analytics,
            analytics::commands::disable_analytics,
            analytics::commands::track_event,
            analytics::commands::identify_user,
            analytics::commands::track_meeting_started,
            analytics::commands::track_recording_started,
            analytics::commands::track_recording_stopped,
            analytics::commands::track_meeting_deleted,
            analytics::commands::track_settings_changed,
            analytics::commands::track_feature_used,
            analytics::commands::is_analytics_enabled,
            analytics::commands::start_analytics_session,
            analytics::commands::end_analytics_session,
            analytics::commands::track_daily_active_user,
            analytics::commands::track_user_first_launch,
            analytics::commands::is_analytics_session_active,
            analytics::commands::track_summary_generation_started,
            analytics::commands::track_summary_generation_completed,
            analytics::commands::track_summary_regenerated,
            analytics::commands::track_model_changed,
            analytics::commands::track_custom_prompt_used,
            analytics::commands::track_meeting_ended,
            analytics::commands::track_analytics_enabled,
            analytics::commands::track_analytics_disabled,
            analytics::commands::track_analytics_transparency_viewed,
            whisper_engine::commands::whisper_init,
            whisper_engine::commands::whisper_get_available_models,
            whisper_engine::commands::whisper_load_model,
            whisper_engine::commands::whisper_get_current_model,
            whisper_engine::commands::whisper_is_model_loaded,
            whisper_engine::commands::whisper_has_available_models,
            whisper_engine::commands::whisper_validate_model_ready,
            whisper_engine::commands::whisper_transcribe_audio,
            whisper_engine::commands::whisper_get_models_directory,
            whisper_engine::commands::whisper_download_model,
            whisper_engine::commands::whisper_cancel_download,
            whisper_engine::commands::whisper_delete_corrupted_model,
            // Parakeet engine commands
            parakeet_engine::commands::parakeet_init,
            parakeet_engine::commands::parakeet_get_available_models,
            parakeet_engine::commands::parakeet_load_model,
            parakeet_engine::commands::parakeet_get_current_model,
            parakeet_engine::commands::parakeet_is_model_loaded,
            parakeet_engine::commands::parakeet_has_available_models,
            parakeet_engine::commands::parakeet_validate_model_ready,
            parakeet_engine::commands::parakeet_transcribe_audio,
            parakeet_engine::commands::parakeet_get_models_directory,
            parakeet_engine::commands::parakeet_download_model,
            parakeet_engine::commands::parakeet_retry_download,
            parakeet_engine::commands::parakeet_cancel_download,
            parakeet_engine::commands::parakeet_delete_corrupted_model,
            parakeet_engine::commands::open_parakeet_models_folder,
            // Parallel processing commands
            whisper_engine::parallel_commands::initialize_parallel_processor,
            whisper_engine::parallel_commands::start_parallel_processing,
            whisper_engine::parallel_commands::pause_parallel_processing,
            whisper_engine::parallel_commands::resume_parallel_processing,
            whisper_engine::parallel_commands::stop_parallel_processing,
            whisper_engine::parallel_commands::get_parallel_processing_status,
            whisper_engine::parallel_commands::get_system_resources,
            whisper_engine::parallel_commands::check_resource_constraints,
            whisper_engine::parallel_commands::calculate_optimal_workers,
            whisper_engine::parallel_commands::prepare_audio_chunks,
            whisper_engine::parallel_commands::test_parallel_processing_setup,
            get_audio_devices,
            trigger_microphone_permission,
            recording_consent::authorize_recording_start,
            start_recording_with_devices,
            start_recording_with_devices_and_meeting,
            start_audio_level_monitoring,
            stop_audio_level_monitoring,
            is_audio_level_monitoring,
            // Recording pause/resume commands
            audio::recording_commands::pause_recording,
            audio::recording_commands::resume_recording,
            audio::recording_commands::is_recording_paused,
            audio::recording_commands::get_recording_state,
            audio::recording_commands::get_meeting_folder_path,
            // Reload sync commands (retrieve transcript history and meeting name)
            audio::recording_commands::get_transcript_history,
            audio::recording_commands::get_recording_meeting_name,
            // Device monitoring commands (AirPods/Bluetooth disconnect/reconnect)
            audio::recording_commands::poll_audio_device_events,
            audio::recording_commands::get_reconnection_status,
            audio::recording_commands::attempt_device_reconnect,
            // Playback device detection (Bluetooth warning)
            audio::recording_commands::get_active_audio_output,
            // Native audio checkpoint recovery remains disabled in v1.0.4.
            // The legacy commands accepted renderer-controlled filesystem paths.
            console_utils::show_console,
            console_utils::hide_console,
            console_utils::toggle_console,
            ollama::get_ollama_models,
            ollama::pull_ollama_model,
            ollama::delete_ollama_model,
            ollama::get_ollama_model_context,
            openai::openai::get_openai_models,
            anthropic::anthropic::get_anthropic_models,
            groq::groq::get_groq_models,
            api::api_get_meetings,
            context::api_get_current_workspace_id,
            api::api_search_evidence,
            ask::commands::api_ask_meeting,
            // Live copilot panel (BACKLOG I1/I2/I3b, ADR-0038): window, shortcuts,
            // and the on-demand grounded insight.
            copilot::commands::copilot_get_status,
            copilot::commands::copilot_set_config,
            copilot::commands::copilot_toggle_panel,
            copilot::commands::copilot_close_panel,
            copilot::commands::copilot_focus_main_window,
            copilot::commands::copilot_request_insight,
            copilot::commands::copilot_cancel_insight,
            diarization::commands::api_diarization_availability,
            diarization::commands::api_diarization_download_models,
            diarization::commands::api_diarize_meeting,
            diarization::commands::api_get_speaker_turns,
            api::api_search_transcripts,
            api::api_get_model_config,
            api::api_save_model_config,
            api::api_has_api_key,
            // api::api_get_auto_generate_setting,
            // api::api_save_auto_generate_setting,
            api::api_get_transcript_config,
            api::api_save_transcript_config,
            api::api_has_transcript_api_key,
            api::api_delete_meeting,
            api::api_get_meeting,
            api::api_get_meeting_metadata,
            api::api_get_meeting_transcripts,
            api::api_save_meeting_title,
            api::api_save_transcript,
            api::api_acknowledge_recording_post_processing,
            api::api_get_pending_recording_post_processing,
            api::api_abandon_recording_post_processing,
            api::open_meeting_folder,
            api::open_external_url,
            // Custom OpenAI commands
            api::api_save_custom_openai_config,
            api::api_get_custom_openai_config,
            api::api_test_custom_openai_connection,
            // Redaction config commands (BACKLOG C6)
            api::api_get_redaction_config,
            api::api_set_redaction_config,
            // Licensing & trial commands (ADR-0023)
            licensing::commands::get_licensing_status,
            licensing::commands::activate_license,
            licensing::commands::deactivate_license,
            // Summary commands
            summary::commands::api_process_transcript,
            summary::commands::api_get_summary,
            summary::commands::api_save_meeting_summary,
            summary::commands::api_get_meeting_summary_language,
            summary::commands::api_save_meeting_summary_language,
            summary::commands::api_get_meeting_detected_summary_language,
            summary::commands::api_save_meeting_detected_summary_language,
            summary::commands::api_detect_transcript_summary_language,
            summary::commands::api_cancel_summary,
            // C1.5 HITL commands (source-linked draft review/approval; ADR-0019)
            summary::commands::api_get_summary_draft,
            summary::commands::api_list_approved_action_items,
            summary::commands::api_approve_summary_block,
            summary::commands::api_reject_summary_block,
            summary::commands::api_edit_summary_block,
            summary::commands::api_restore_summary_block,
            summary::commands::api_approve_summary,
            summary::commands::api_get_open_action_items,
            summary::commands::api_approve_action_item,
            summary::commands::api_reject_action_item,
            summary::commands::api_restore_action_item,
            summary::commands::api_edit_action_item,
            // Learned-rule commands (ADR-0030 §9) — the surface that makes
            // auto-activation defensible: every rule visible, editable, deletable.
            learning::commands::api_list_learned_rules,
            learning::commands::api_create_learned_rule,
            learning::commands::api_activate_learned_rule,
            learning::commands::api_dismiss_learned_rule,
            learning::commands::api_edit_learned_rule,
            learning::commands::api_delete_learned_rule,
            learning::commands::api_get_rule_evidence,
            learning::commands::api_mine_learned_rules,
            learning::commands::api_get_learning_stats,
            learning::commands::api_get_learning_config,
            learning::commands::api_set_learning_config,
            // Template commands
            summary::template_commands::api_list_templates,
            summary::template_commands::api_get_template_details,
            summary::template_commands::api_validate_template,
            // Built-in AI commands
            summary::summary_engine::commands::builtin_ai_list_models,
            summary::summary_engine::commands::builtin_ai_get_model_info,
            summary::summary_engine::commands::builtin_ai_download_model,
            summary::summary_engine::commands::builtin_ai_cancel_download,
            summary::summary_engine::commands::builtin_ai_delete_model,
            summary::summary_engine::commands::builtin_ai_is_model_ready,
            summary::summary_engine::commands::builtin_ai_get_available_summary_model,
            summary::summary_engine::commands::builtin_ai_get_recommended_model,
            openrouter::get_openrouter_models,
            audio::recording_preferences::get_recording_preferences,
            audio::recording_preferences::set_recording_preferences,
            audio::recording_preferences::get_default_recordings_folder_path,
            audio::recording_preferences::open_recordings_folder,
            audio::recording_preferences::select_recording_folder,
            audio::recording_preferences::get_available_audio_backends,
            audio::recording_preferences::get_current_audio_backend,
            audio::recording_preferences::set_audio_backend,
            audio::recording_preferences::get_audio_backend_info,
            // Language preference commands
            set_language_preference,
            // Notification system commands
            notifications::commands::get_notification_settings,
            notifications::commands::set_notification_settings,
            notifications::commands::request_notification_permission,
            notifications::commands::show_notification,
            notifications::commands::show_test_notification,
            notifications::commands::is_dnd_active,
            notifications::commands::get_system_dnd_status,
            notifications::commands::set_manual_dnd,
            notifications::commands::set_notification_consent,
            notifications::commands::clear_notifications,
            notifications::commands::is_notification_system_ready,
            notifications::commands::initialize_notification_manager_manual,
            notifications::commands::test_notification_with_auto_consent,
            notifications::commands::get_notification_stats,
            // System audio capture commands
            // The unused raw system-audio capture start command is deliberately
            // not exposed to renderer IPC. Supported recording starts flow only
            // through the consent-ticket-protected commands above.
            audio::system_audio_commands::list_system_audio_devices_command,
            audio::system_audio_commands::check_system_audio_permissions_command,
            audio::system_audio_commands::start_system_audio_monitoring,
            audio::system_audio_commands::stop_system_audio_monitoring,
            audio::system_audio_commands::get_system_audio_monitoring_status,
            // Screen Recording permission commands
            audio::permissions::check_screen_recording_permission_command,
            audio::permissions::request_screen_recording_permission_command,
            audio::permissions::trigger_system_audio_permission_command,
            // Database import commands
            database::commands::check_first_launch,
            database::commands::initialize_fresh_database,
            // Legacy database import is disabled in v1.0.4. The old IPC
            // contract accepted renderer-controlled filesystem paths.
            // Database and Models path commands
            database::commands::get_database_directory,
            database::commands::open_database_folder,
            // At-rest encryption status (ADR-0014 follow-up: UI plaintext-fallback warning)
            database::commands::get_db_encryption_status,
            whisper_engine::commands::open_models_folder,
            // Onboarding commands
            onboarding::get_onboarding_status,
            onboarding::save_onboarding_status_cmd,
            onboarding::reset_onboarding_status_cmd,
            onboarding::complete_onboarding,
            // System settings commands
            #[cfg(target_os = "macos")]
            utils::open_system_settings,
            // Import/retranscription commands are intentionally not exposed in
            // v1.0.4. Their UI flag is forced off until every filesystem path is
            // resolved from tenant-scoped Rust state rather than renderer input.
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            match event {
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    tray::focus_main_window(_app_handle);
                }
                tauri::RunEvent::Exit => {
                    log::info!("Application exiting, cleaning up resources...");
                    tauri::async_runtime::block_on(async {
                        // Clean up database connection and checkpoint WAL
                        if let Some(app_state) = _app_handle.try_state::<state::AppState>() {
                            log::info!("Starting database cleanup...");
                            if let Err(e) = app_state.db_manager.cleanup().await {
                                log::error!("Failed to cleanup database: {}", e);
                            } else {
                                log::info!("Database cleanup completed successfully");
                            }
                        } else {
                            log::warn!(
                                "AppState not available for database cleanup (likely first launch)"
                            );
                        }

                        // Clean up sidecar
                        log::info!("Cleaning up sidecar...");
                        if let Err(e) = summary::summary_engine::force_shutdown_sidecar().await {
                            log::error!("Failed to force shutdown sidecar: {}", e);
                        }
                    });
                    log::info!("Application cleanup complete");
                }
                _ => {}
            }
        });
}
