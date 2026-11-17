use log::{debug as log_debug, error as log_error, info as log_info, warn as log_warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_store::StoreExt;

use crate::{
    database::{
        models::{MeetingModel, MeetingSpeaker, PendingMeetingModel},
        repositories::{
            meeting::MeetingsRepository,
            meeting_calendar::{MeetingCalendar, MeetingCalendarRepository},
            meeting_tags::MeetingTagsRepository,
            setting::SettingsRepository,
            transcript::TranscriptsRepository,
        },
    },
    state::AppState,
    summary::{CopilotCliConfig, CustomOpenAIConfig},
};

// Hardcoded server URL
const APP_SERVER_URL: &str = "http://localhost:5167";

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

/// A sidebar row: the meeting plus which pipeline stages it has finished.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    /// When the meeting was recorded or imported (RFC 3339, UTC).
    pub created_at: crate::database::models::DateTimeUtc,
    /// Has a transcript that live transcription finished writing.
    pub transcribed: bool,
    /// Has a recording still to transcribe; false when there is no audio to
    /// recover a transcript from.
    pub transcript_needed: bool,
    /// Has a completed AI summary.
    pub summarized: bool,
    /// The meeting has a note in the Obsidian vault.
    pub obsidian_exported: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptSearchResult {
    pub id: String,
    pub title: String,
    #[serde(rename = "matchContext")]
    pub match_context: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileRequest {
    pub email: String,
    pub license_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveProfileRequest {
    pub id: String,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProfileRequest {
    pub email: String,
    pub license_key: String,
    pub company: String,
    pub position: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetApiKeyRequest {
    pub provider: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptConfig {
    pub provider: String,
    pub model: String,
    #[serde(rename = "realtimeTranscriptionEnabled")]
    pub realtime_transcription_enabled: bool,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    /// Base URL for the openaiCompatible provider (e.g. "http://127.0.0.1:8000/v1")
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    /// A user-managed list of names and domain terms passed to local
    /// whisper-rs as `initial_prompt` for every transcription.
    #[serde(rename = "vocabularyHint")]
    pub vocabulary_hint: String,
    /// Send `vocabulary_hint` to the remote (openaiCompatible) server as the
    /// transcription `prompt`. Explicit opt-in, off by default.
    #[serde(rename = "remoteVocabularyEnabled")]
    pub remote_vocabulary_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "realtimeTranscriptionEnabled")]
    pub realtime_transcription_enabled: bool,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "vocabularyHint")]
    pub vocabulary_hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteMeetingRequest {
    pub meeting_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingDetails {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub transcripts: Vec<MeetingTranscript>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingTranscript {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    // Recording-relative timestamps for audio-transcript synchronization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_start_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_end_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    // Speaker attribution (e.g. imported Teams transcripts); null for audio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// Meeting metadata without transcripts (for pagination)
#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingMetadata {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
    /// The Outlook event the recording was matched to; `None` for most meetings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<MeetingCalendar>,
}

/// Paginated transcripts response with total count
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedTranscriptsResponse {
    pub transcripts: Vec<MeetingTranscript>,
    pub total_count: i64,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveMeetingTitleRequest {
    pub meeting_id: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveMeetingSummaryRequest {
    pub meeting_id: String,
    pub summary: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveTranscriptRequest {
    pub meeting_title: String,
    pub transcripts: Vec<TranscriptSegment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    // NEW: Recording-relative timestamps for playback synchronization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_start_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_end_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    // Speaker attribution, when known (e.g. imported Teams transcripts). Audio
    // transcription leaves this None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: Option<String>,
    pub email: String,
    pub license_key: String,
    pub company: Option<String>,
    pub position: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub is_licensed: bool,
}

// Helper function to get auth token from store (optional)
#[allow(dead_code)]
async fn get_auth_token<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let store = match app.store("store.json") {
        Ok(store) => store,
        Err(_) => return None,
    };

    match store.get("authToken") {
        Some(token) => {
            if let Some(token_str) = token.as_str() {
                let truncated = token_str.chars().take(20).collect::<String>();
                log_info!("Found auth token: {}", truncated);
                Some(token_str.to_string())
            } else {
                log_warn!("Auth token is not a string");
                None
            }
        }
        None => {
            log_warn!("No auth token found in store");
            None
        }
    }
}

// Helper function to get server address - now hardcoded
async fn get_server_address<R: Runtime>(_app: &AppHandle<R>) -> Result<String, String> {
    log_info!("Using hardcoded server URL: {}", APP_SERVER_URL);
    Ok(APP_SERVER_URL.to_string())
}

// Generic API call function with optional authentication
async fn make_api_request<R: Runtime, T: for<'de> Deserialize<'de>>(
    app: &AppHandle<R>,
    endpoint: &str,
    method: &str,
    body: Option<&str>,
    additional_headers: Option<HashMap<String, String>>,
    auth_token: Option<String>, // Pass auth token from frontend
) -> Result<T, String> {
    let client = reqwest::Client::new();
    let server_url = get_server_address(app).await?;

    let url = format!("{}{}", server_url, endpoint);
    log_info!("Making {} request", method);

    let mut request = match method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => return Err(format!("Unsupported HTTP method: {}", method)),
    };

    // Add authorization header if auth token is provided
    if let Some(token) = auth_token {
        log_info!("Adding authorization header");
        request = request.header("Authorization", format!("Bearer {}", token));
    } else {
        log_warn!("No auth token provided, making unauthenticated request");
    }

    request = request.header("Content-Type", "application/json");

    // Add additional headers if provided
    if let Some(headers) = additional_headers {
        for (key, value) in headers {
            request = request.header(&key, &value);
        }
    }

    // Add body if provided
    if let Some(body_str) = body {
        request = request.body(body_str.to_string());
    }

    let response = request.send().await.map_err(|error| {
        let (category, message) = if error.is_timeout() {
            ("timeout", "Request timed out")
        } else if error.is_connect() {
            ("connection", "Could not connect to the server")
        } else {
            ("request", "Request failed")
        };
        log_error!("API request failed ({})", category);
        message.to_string()
    })?;

    let status = response.status();
    log_info!("Response status: {}", status);

    if !status.is_success() {
        let error_msg = format!("HTTP request failed with status {}", status);
        log_error!("API request failed with HTTP status {}", status);
        return Err(error_msg);
    }

    let response_text = response.text().await.map_err(|e| {
        let error_msg = format!("Failed to read response: {}", e);
        log_error!("{}", error_msg);
        error_msg
    })?;

    serde_json::from_str(&response_text).map_err(|e| {
        let error_msg = format!("Failed to parse JSON: {}", e);
        log_error!("{}", error_msg);
        error_msg
    })
}

/// Tells the UI to refetch `api_get_meetings`: the meeting list or one
/// meeting's transcribed / summarized / Obsidian-exported flags changed.
pub fn emit_meetings_changed<R: Runtime>(app: &AppHandle<R>, meeting_id: &str) {
    if let Err(e) = app.emit("meetings-changed", serde_json::json!({ "meeting_id": meeting_id })) {
        log_warn!("Failed to emit meetings-changed for {}: {}", meeting_id, e);
    }
}

// API Commands for Tauri

#[tauri::command]
pub async fn api_get_meetings<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    auth_token: Option<String>,
) -> Result<Vec<Meeting>, String> {
    log_info!(
        "api_get_meetings called with auth_token(native) : {}",
        auth_token.is_some()
    );
    let pool = state.db_manager.pool();
    match MeetingsRepository::get_meetings(pool).await {
        Ok(meetings) => {
            log_info!("Successfully got {} meetings", meetings.len());
            Ok(meetings)
        }
        Err(e) => {
            log_error!("Error getting meetings: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_get_meeting_tags(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<String>, String> {
    MeetingTagsRepository::get_tags(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| e.to_string())
}

/// Replace a meeting's tags and mirror them into its Obsidian note when one
/// was already exported. Returns the normalized tags actually stored.
#[tauri::command]
pub async fn api_set_meeting_tags<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    tags: Vec<String>,
) -> Result<Vec<String>, String> {
    let stored = MeetingTagsRepository::set_tags(state.db_manager.pool(), &meeting_id, &tags)
        .await
        .map_err(|e| e.to_string())?;
    if let Err(e) = crate::obsidian::sync_note_tags(&app, &meeting_id, &stored).await {
        log_warn!("Failed to update Obsidian note tags for {}: {}", meeting_id, e);
    }
    Ok(stored)
}

#[tauri::command]
pub async fn api_get_all_meeting_tags(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    MeetingTagsRepository::all_tags(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_get_pending_meetings<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PendingMeetingModel>, String> {
    let pool = state.db_manager.pool();
    match MeetingsRepository::get_pending_meetings(pool).await {
        Ok(pending) => {
            log_info!("Found {} meetings with pending work", pending.len());
            Ok(pending)
        }
        Err(e) => {
            log_error!("Error getting pending meetings: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_search_transcripts<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    query: String,
    auth_token: Option<String>,
) -> Result<Vec<TranscriptSearchResult>, String> {
    log_info!(
        "api_search_transcripts called with a {} char query, auth_token: {}",
        query.chars().count(),
        auth_token.is_some()
    );

    let pool = state.db_manager.pool();

    match TranscriptsRepository::search_transcripts(pool, &query).await {
        Ok(results) => {
            log_info!(
                "Search completed successfully with {} results.",
                results.len()
            );
            Ok(results)
        }
        Err(e) => {
            log_error!("Error searching transcripts ({} char query): {}", query.chars().count(), e);
            Err(format!("Failed to search transcripts: {}", e))
        }
    }
}

#[tauri::command]
pub async fn api_get_profile<R: Runtime>(
    app: AppHandle<R>,
    email: String,
    license_key: String,
    auth_token: Option<String>,
) -> Result<Profile, String> {
    log_info!(
        "api_get_profile called for email: {}, auth_token: {}",
        email,
        auth_token.is_some()
    );

    let profile_request = ProfileRequest { email, license_key };
    let body = serde_json::to_string(&profile_request).map_err(|e| e.to_string())?;

    make_api_request::<R, Profile>(&app, "/get-profile", "POST", Some(&body), None, auth_token)
        .await
}

#[tauri::command]
pub async fn api_save_profile<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    email: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_profile called for email: {}, auth_token: {}",
        email,
        auth_token.is_some()
    );

    let save_request = SaveProfileRequest { id, email };
    let body = serde_json::to_string(&save_request).map_err(|e| e.to_string())?;

    make_api_request::<R, serde_json::Value>(
        &app,
        "/save-profile",
        "POST",
        Some(&body),
        None,
        auth_token,
    )
    .await
}

#[tauri::command]
pub async fn api_update_profile<R: Runtime>(
    app: AppHandle<R>,
    email: String,
    license_key: String,
    company: String,
    position: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_update_profile called for email: {}, auth_token: {}",
        email,
        auth_token.is_some()
    );

    let update_request = UpdateProfileRequest {
        email,
        license_key,
        company,
        position,
    };
    let body = serde_json::to_string(&update_request).map_err(|e| e.to_string())?;

    make_api_request::<R, serde_json::Value>(
        &app,
        "/update-profile",
        "POST",
        Some(&body),
        None,
        auth_token,
    )
    .await
}

#[tauri::command]
pub async fn api_get_model_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    _auth_token: Option<String>,
) -> Result<Option<ModelConfig>, String> {
    log_info!("api_get_model_config called (native)");
    let pool = state.db_manager.pool();

    match SettingsRepository::get_model_config(pool).await {
        Ok(Some(config)) => {
            log_info!(
                "✅ Found model config in database: provider={}, model={}, whisperModel={}, ollamaEndpoint={:?}",
                &config.provider,
                &config.model,
                &config.whisper_model,
                &config.ollama_endpoint
            );
            match SettingsRepository::get_api_key(pool, &config.provider).await {
                Ok(api_key) => {
                    log_info!("Successfully retrieved model config and API key.");
                    Ok(Some(ModelConfig {
                        provider: config.provider,
                        model: config.model,
                        whisper_model: config.whisper_model,
                        api_key,
                        ollama_endpoint: config.ollama_endpoint,
                    }))
                }
                Err(e) => {
                    log_error!(
                        "Failed to get API key for provider {}: {}",
                        &config.provider,
                        e
                    );
                    Err(e.to_string())
                }
            }
        }
        Ok(None) => {
            log_warn!("⚠️ No model config found in database - database may be empty or settings table not initialized");
            Ok(None)
        }
        Err(e) => {
            log_error!("❌ Failed to get model config from database: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_save_model_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    model: String,
    whisper_model: String,
    api_key: Option<String>,
    ollama_endpoint: Option<String>,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "💾 api_save_model_config called (native): provider='{}', model='{}', whisperModel='{}', ollamaEndpoint={:?}",
        &provider,
        &model,
        &whisper_model,
        &ollama_endpoint
    );
    let pool = state.db_manager.pool();

    if let Err(e) = SettingsRepository::save_model_config(
        pool,
        &provider,
        &model,
        &whisper_model,
        ollama_endpoint.as_deref(),
    )
    .await
    {
        log_error!("❌ Failed to save model config to database: {}", e);
        return Err(e.to_string());
    }

    // Skip API key saving for custom-openai provider (it uses customOpenAIConfig JSON instead)
    if let Some(key) = api_key {
        if !key.is_empty() && provider != "custom-openai" {
            log_info!("🔑 API key provided, saving...");
            if let Err(e) = SettingsRepository::save_api_key(pool, &provider, &key).await {
                log_error!("❌ Failed to save API key: {}", e);
                return Err(e.to_string());
            }
        }
    }

    // Trigger graceful shutdown of built-in AI sidecar if it's running
    // This ensures that if the user switched models/providers, the old one is cleaned up
    // The shutdown happens in the background, so it won't block the UI
    if let Err(e) = crate::summary::summary_engine::client::shutdown_sidecar_gracefully().await {
        log_warn!("Failed to initiate graceful sidecar shutdown: {}", e);
    }

    log_info!("✅ Successfully saved model configuration to database");
    Ok(
        serde_json::json!({ "status": "success", "message": "Model configuration saved successfully" }),
    )
}

#[tauri::command]
pub async fn api_get_api_key<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    _auth_token: Option<String>,
) -> Result<String, String> {
    log_info!(
        "api_get_api_key called (native) for provider '{}'",
        &provider
    );
    match SettingsRepository::get_api_key(&state.db_manager.pool(), &provider).await {
        Ok(key) => {
            log_info!(
                "Successfully retrieved API key for provider '{}'.",
                &provider
            );
            Ok(key.unwrap_or_default())
        }
        Err(e) => {
            log_error!("Failed to get API key for provider '{}': {}", &provider, e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_get_transcript_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    _auth_token: Option<String>,
) -> Result<Option<TranscriptConfig>, String> {
    log_info!("api_get_transcript_config called (native)");
    let pool = state.db_manager.pool();

    match SettingsRepository::get_transcript_config(pool).await {
        Ok(Some(config)) => {
            log_info!(
                "Found transcript config: provider={}, model={}",
                &config.provider,
                &config.model
            );
            match SettingsRepository::get_transcript_api_key(pool, &config.provider).await {
                Ok(api_key) => {
                    log_info!("Successfully retrieved transcript config and API key.");
                    // Each remote provider stores its endpoint in its own
                    // column; surface the one matching the active provider.
                    let base_url = match config.provider.as_str() {
                        crate::config::PROVIDER_GEMINI_TRANSCRIBE => {
                            config.gemini_transcribe_base_url
                        }
                        _ => config.openai_compatible_base_url,
                    };
                    Ok(Some(TranscriptConfig {
                        provider: config.provider,
                        model: config.model,
                        realtime_transcription_enabled: config.realtime_transcription_enabled,
                        api_key,
                        base_url,
                        vocabulary_hint: config.whisper_vocabulary_hint,
                        remote_vocabulary_enabled: config.remote_vocabulary_enabled,
                    }))
                }
                Err(e) => {
                    log_error!(
                        "Failed to get transcript API key for provider {}: {}",
                        &config.provider,
                        e
                    );
                    Err(e.to_string())
                }
            }
        }
        Ok(None) => {
            log_info!("No transcript config found, returning default.");
            Ok(Some(TranscriptConfig {
                provider: "parakeet".to_string(),
                model: crate::config::DEFAULT_PARAKEET_MODEL.to_string(),
                realtime_transcription_enabled: false,
                api_key: None,
                base_url: None,
                vocabulary_hint: crate::config::DEFAULT_WHISPER_VOCABULARY_HINT.to_string(),
                remote_vocabulary_enabled: false,
            }))
        }
        Err(e) => {
            log_error!("Failed to get transcript config: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_save_transcript_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    model: String,
    realtime_transcription_enabled: Option<bool>,
    api_key: Option<String>,
    base_url: Option<String>,
    vocabulary_hint: Option<String>,
    remote_vocabulary_enabled: Option<bool>,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_transcript_config called (native) for provider '{}'",
        &provider
    );
    let pool = state.db_manager.pool();

    let realtime_transcription_enabled = match realtime_transcription_enabled {
        Some(value) => value,
        None => SettingsRepository::get_transcript_config(pool)
            .await
            .ok()
            .flatten()
            .map(|config| config.realtime_transcription_enabled)
            .unwrap_or(false),
    };

    if let Err(e) = SettingsRepository::save_transcript_config(
        pool,
        &provider,
        &model,
        realtime_transcription_enabled,
    )
    .await
    {
        log_error!("Failed to save transcript config: {}", e);
        return Err(e.to_string());
    }

    if let Some(key) = api_key {
        if !key.is_empty() {
            log_info!("API key provided, saving for transcript provider...");
            if let Err(e) = SettingsRepository::save_transcript_api_key(pool, &provider, &key).await
            {
                log_error!("Failed to save transcript API key: {}", e);
                return Err(e.to_string());
            }
        }
    }

    if let Some(url) = base_url {
        if !url.is_empty() {
            log_info!("Base URL provided, saving for transcript provider...");
            if let Err(e) =
                SettingsRepository::save_transcript_base_url(pool, &provider, &url).await
            {
                log_error!("Failed to save transcript base URL: {}", e);
                return Err(e.to_string());
            }
        }
    }

    if let Some(vocabulary_hint) = vocabulary_hint {
        if vocabulary_hint.contains('\0') {
            return Err("Vocabulary hints cannot contain a null character".to_string());
        }

        let vocabulary_hint = vocabulary_hint.trim();
        if let Err(e) = SettingsRepository::save_whisper_vocabulary_hint(pool, vocabulary_hint).await {
            log_error!("Failed to save Whisper vocabulary hints: {}", e);
            return Err(e.to_string());
        }

        // Update an already-loaded engine immediately; future engines load
        // this persisted value before their first transcription.
        crate::whisper_engine::commands::set_whisper_vocabulary_hint(
            vocabulary_hint.to_string(),
        )
        .await;
    }

    if let Some(enabled) = remote_vocabulary_enabled {
        if let Err(e) = SettingsRepository::save_remote_vocabulary_enabled(pool, enabled).await {
            log_error!("Failed to save remote vocabulary preference: {}", e);
            return Err(e.to_string());
        }
    }

    log_info!("Successfully saved transcript configuration.");
    Ok(
        serde_json::json!({ "status": "success", "message": "Transcript configuration saved successfully" }),
    )
}

#[tauri::command]
pub async fn api_get_transcript_api_key<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    _auth_token: Option<String>,
) -> Result<String, String> {
    log_info!(
        "api_get_transcript_api_key called (native) for provider '{}'",
        &provider
    );
    match SettingsRepository::get_transcript_api_key(&state.db_manager.pool(), &provider).await {
        Ok(key) => {
            log_info!(
                "Successfully retrieved transcript API key for provider '{}'.",
                &provider
            );
            Ok(key.unwrap_or_default())
        }
        Err(e) => {
            log_error!(
                "Failed to get transcript API key for provider '{}': {}",
                &provider,
                e
            );
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_delete_api_key<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    _auth_token: Option<String>,
) -> Result<(), String> {
    log_info!(
        "log_api_delete_api_key called (native) for provider '{}'",
        &provider
    );
    match SettingsRepository::delete_api_key(&state.db_manager.pool(), &provider).await {
        Ok(_) => {
            log_info!("Successfully deleted API key for provider '{}'.", &provider);
            Ok(())
        }
        Err(e) => {
            log_error!(
                "Failed to delete API key for provider '{}': {}",
                &provider,
                e
            );
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_delete_meeting<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_delete_meeting called for meeting_id(native): {}, auth_token: {}",
        meeting_id,
        auth_token.is_some()
    );

    let pool = state.db_manager.pool();

    match MeetingsRepository::delete_meeting(pool, &meeting_id).await {
        Ok(true) => {
            log_info!("Successfully deleted meeting {}", meeting_id);
            emit_meetings_changed(&app, &meeting_id);
            Ok(serde_json::json!({
                "status": "success",
                "message": "Meeting deleted successfully"
            }))
        }
        Ok(false) => {
            log_warn!("Meeting not found or already deleted: {}", meeting_id);
            Err(format!(
                "Meeting not found or could not be deleted: {}",
                meeting_id
            ))
        }
        Err(e) => {
            log_error!("Error deleting meeting {}: {}", meeting_id, e);
            Err(format!("Failed to delete meeting: {}", e))
        }
    }
}

#[tauri::command]
pub async fn api_get_meeting<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
    auth_token: Option<String>,
) -> Result<MeetingDetails, String> {
    log_info!(
        "api_get_meeting called(native) for meeting_id: {}, auth_token: {}",
        meeting_id,
        auth_token.is_some()
    );

    let pool = state.db_manager.pool();

    match MeetingsRepository::get_meeting(pool, &meeting_id).await {
        Ok(Some(meeting)) => {
            log_info!("Successfully retrieved meeting {}", meeting_id);
            Ok(meeting)
        }
        Ok(None) => {
            log_warn!("Meeting not found: {}", meeting_id);
            Err(format!("Meeting not found: {}", meeting_id))
        }
        Err(e) => {
            log_error!("Error retrieving meeting {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve meeting: {}", e))
        }
    }
}

/// Get meeting metadata without transcripts (for pagination)
#[tauri::command]
pub async fn api_get_meeting_metadata<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<MeetingMetadata, String> {
    log_info!("api_get_meeting_metadata called for meeting_id: {}", meeting_id);

    let pool = state.db_manager.pool();

    match MeetingsRepository::get_meeting_metadata(pool, &meeting_id).await {
        Ok(Some(meeting)) => {
            log_info!("Successfully retrieved meeting metadata {}", meeting_id);
            let calendar = MeetingCalendarRepository::get(pool, &meeting_id)
                .await
                .unwrap_or_else(|e| {
                    log_warn!("Calendar details unavailable for meeting {}: {}", meeting_id, e);
                    None
                });
            Ok(MeetingMetadata {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                folder_path: meeting.folder_path,
                calendar,
            })
        }
        Ok(None) => {
            log_warn!("Meeting not found: {}", meeting_id);
            Err(format!("Meeting not found: {}", meeting_id))
        }
        Err(e) => {
            log_error!("Error retrieving meeting metadata {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve meeting metadata: {}", e))
        }
    }
}

/// Get paginated transcripts for a meeting
#[tauri::command]
pub async fn api_get_meeting_transcripts<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    limit: i64,
    offset: i64,
    state: tauri::State<'_, AppState>,
) -> Result<PaginatedTranscriptsResponse, String> {
    log_info!(
        "api_get_meeting_transcripts called for meeting_id: {}, limit: {}, offset: {}",
        meeting_id,
        limit,
        offset
    );

    let pool = state.db_manager.pool();

    match MeetingsRepository::get_meeting_transcripts_paginated(pool, &meeting_id, limit, offset).await {
        Ok((transcripts, total_count)) => {
            log_info!(
                "Successfully retrieved {} transcripts for meeting {} (total: {})",
                transcripts.len(),
                meeting_id,
                total_count
            );

            // Convert Transcript to MeetingTranscript
            let meeting_transcripts = transcripts
                .into_iter()
                .map(|t| MeetingTranscript {
                    id: t.id,
                    text: t.transcript,
                    timestamp: t.timestamp,
                    audio_start_time: t.audio_start_time,
                    audio_end_time: t.audio_end_time,
                    duration: t.duration,
                    speaker: t.speaker,
                })
                .collect::<Vec<_>>();

            let has_more = (offset + meeting_transcripts.len() as i64) < total_count;

            Ok(PaginatedTranscriptsResponse {
                transcripts: meeting_transcripts,
                total_count,
                has_more,
            })
        }
        Err(e) => {
            log_error!("Error retrieving transcripts for meeting {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve transcripts: {}", e))
        }
    }
}

/// Follow a user-initiated title change into the Obsidian vault, returning the
/// note's new vault-relative path when it actually moved.
///
/// Entirely best-effort: a missing vault, a locked file, or a note the user
/// deleted must never turn a successful rename into an error toast.
async fn rename_obsidian_note<R: Runtime>(
    app: &AppHandle<R>,
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    title: &str,
) -> Option<String> {
    // `created_at` drives the `{date}` token in the filename template.
    let created_at = match MeetingsRepository::get_meeting_metadata(pool, meeting_id).await {
        Ok(Some(meeting)) => meeting.created_at.0.to_rfc3339(),
        Ok(None) => return None,
        Err(e) => {
            log_warn!(
                "Obsidian rename: failed to load meeting {}: {}",
                meeting_id,
                e
            );
            return None;
        }
    };

    match crate::obsidian::rename_meeting_note(app, pool, meeting_id, title, &created_at).await {
        Ok(Some(result)) => Some(result.relative_path),
        Ok(None) => None,
        Err(e) => {
            log_warn!(
                "Obsidian rename failed for meeting {} (title change still saved): {}",
                meeting_id,
                e
            );
            None
        }
    }
}

#[tauri::command]
pub async fn api_save_meeting_title<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    title: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_meeting_title called for meeting_id: {}, auth_token: {}",
        meeting_id,
        auth_token.is_some()
    );
    let pool = state.db_manager.pool();
    match MeetingsRepository::update_meeting_title(pool, &meeting_id, &title).await {
        Ok(true) => {
            log_info!("Successfully saved meeting title");
            emit_meetings_changed(&app, &meeting_id);
            let obsidian_note = rename_obsidian_note(&app, pool, &meeting_id, &title).await;
            Ok(serde_json::json!({
                "message": "Meeting title saved successfully",
                "obsidian_note": obsidian_note,
            }))
        }
        Ok(false) => {
            log_error!("No meeting found with id {}", meeting_id);
            Err(format!("No meeting found with id {}", meeting_id))
        }
        Err(e) => {
            log_error!("Failed to update meeting {}", e);
            Err(format!("Failed to update meeting: {}", e))
        }
    }
}

/// Longest speaker name `api_rename_meeting_speaker` accepts, in characters.
const MAX_SPEAKER_NAME_CHARS: usize = 80;

#[tauri::command]
pub async fn api_get_meeting_speakers(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<MeetingSpeaker>, String> {
    log_info!("api_get_meeting_speakers called for meeting_id: {}", meeting_id);
    TranscriptsRepository::get_meeting_speakers(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| {
            log_error!("Failed to load speakers for meeting {}: {}", meeting_id, e);
            format!("Failed to load meeting speakers: {}", e)
        })
}

/// Rename one speaker across a whole meeting by rewriting the stored
/// `speaker` value. `old_speaker` is the stored value and matched exactly;
/// renaming onto an existing name merges the two speakers. Returns the
/// number of segments rewritten.
#[tauri::command]
pub async fn api_rename_meeting_speaker<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    old_speaker: String,
    new_speaker: String,
) -> Result<u64, String> {
    // Names are meeting content: log only the meeting id.
    log_info!("api_rename_meeting_speaker called for meeting_id: {}", meeting_id);
    let new_speaker = new_speaker.trim();
    if new_speaker.is_empty() {
        return Err("Speaker name cannot be empty".to_string());
    }
    if new_speaker.chars().count() > MAX_SPEAKER_NAME_CHARS {
        return Err(format!(
            "Speaker name is too long (at most {} characters)",
            MAX_SPEAKER_NAME_CHARS
        ));
    }
    if new_speaker == old_speaker {
        return Ok(0);
    }

    let pool = state.db_manager.pool();
    let rows = TranscriptsRepository::rename_meeting_speaker(pool, &meeting_id, &old_speaker, new_speaker)
        .await
        .map_err(|e| {
            log_error!("Failed to rename speaker in meeting {}: {}", meeting_id, e);
            format!("Failed to rename speaker: {}", e)
        })?;
    log_info!("Renamed a speaker on {} segments of meeting {}", rows, meeting_id);
    if rows == 0 {
        return Ok(0);
    }

    // Keep the meeting folder's transcripts.json sidecar in step. Best-effort:
    // the database is the source of truth.
    match MeetingsRepository::get_meeting_metadata(pool, &meeting_id).await {
        Ok(Some(MeetingModel { folder_path: Some(folder), .. })) if !folder.is_empty() => {
            if let Err(e) = crate::audio::common::rename_speaker_in_transcripts_json(
                std::path::Path::new(&folder),
                &old_speaker,
                new_speaker,
            ) {
                log_warn!(
                    "Speaker renamed in the database but transcripts.json for meeting {} was not updated: {}",
                    meeting_id,
                    e
                );
            }
        }
        Ok(_) => {}
        Err(e) => log_warn!("Failed to load folder of meeting {}: {}", meeting_id, e),
    }

    emit_meetings_changed(&app, &meeting_id);
    Ok(rows)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn api_save_transcript<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_title: String,
    transcripts: Vec<serde_json::Value>,
    folder_path: Option<String>,
    transcription_incomplete: Option<bool>,
    auth_token: Option<String>,
    calendar_event: Option<crate::calendar::CalendarEvent>,
) -> Result<serde_json::Value, String> {
    let transcription_incomplete = transcription_incomplete.unwrap_or(false);
    log_info!(
        "api_save_transcript called for meeting: {}, transcripts: {}, folder_path: {:?}, incomplete: {}, auth_token: {}",
        meeting_title,
        transcripts.len(),
        folder_path,
        transcription_incomplete,
        auth_token.is_some()
    );

    // Convert serde_json::Value to TranscriptSegment
    let transcripts_to_save: Vec<TranscriptSegment> = transcripts
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            log_error!("Failed to parse transcript segments: {}", e);
            format!("Invalid transcript data format: {}. Please check the data structure.", e)
        })?;

    // Timing only: segment text is meeting content.
    if let Some(first_seg) = transcripts_to_save.first() {
        log_debug!("First parsed segment: audio_start_time={:?}, audio_end_time={:?}, duration={:?}",
                   first_seg.audio_start_time,
                   first_seg.audio_end_time,
                   first_seg.duration);
    }

    let pool = state.db_manager.pool();

    // Now, call the repository with the correctly typed data.
    match TranscriptsRepository::save_transcript(
        pool,
        &meeting_title,
        &transcripts_to_save,
        folder_path,
        transcription_incomplete,
    )
    .await
    {
        Ok(meeting_id) => {
            log_info!(
                "Successfully saved transcript and created meeting with id: {}",
                meeting_id
            );
            if let Some(event) = calendar_event.as_ref() {
                // The meeting is saved either way; a failed attach only loses
                // the calendar details.
                if let Err(e) = MeetingCalendarRepository::attach(pool, &meeting_id, event).await {
                    log_error!("Failed to attach calendar event to meeting {}: {}", meeting_id, e);
                } else {
                    log_info!("Attached calendar event {} to meeting {}", event.id, meeting_id);
                }
            }
            emit_meetings_changed(&app, &meeting_id);
            Ok(serde_json::json!({
                "status": "success",
                "message": "Transcript saved successfully",
                "meeting_id": meeting_id
            }))
        }
        Err(e) => {
            log_error!(
                "Error saving transcript for meeting '{}': {}",
                meeting_title,
                e
            );
            Err(format!("Failed to save transcript: {}", e))
        }
    }
}

/// Opens the meeting's recording folder in the system file explorer
#[tauri::command]
pub async fn open_meeting_folder<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<(), String> {
    log_info!("open_meeting_folder called for meeting_id: {}", meeting_id);

    let pool = state.db_manager.pool();

    // Get meeting with folder_path
    let meeting: Option<MeetingModel> = sqlx::query_as(
        "SELECT id, title, created_at, updated_at, folder_path FROM meetings WHERE id = ?",
    )
    .bind(&meeting_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    match meeting {
        Some(m) => {
            if let Some(folder_path) = m.folder_path {
                log_info!("Opening meeting folder: {}", folder_path);

                // Verify folder exists
                let path = std::path::Path::new(&folder_path);
                if !path.exists() {
                    log_warn!("Folder path does not exist: {}", folder_path);
                    return Err(format!("Recording folder not found: {}", folder_path));
                }

                // Open folder based on OS
                #[cfg(target_os = "macos")]
                {
                    std::process::Command::new("open")
                        .arg(&folder_path)
                        .spawn()
                        .map_err(|e| format!("Failed to open folder: {}", e))?;
                }

                #[cfg(target_os = "windows")]
                {
                    std::process::Command::new("explorer")
                        .arg(&folder_path)
                        .spawn()
                        .map_err(|e| format!("Failed to open folder: {}", e))?;
                }

                #[cfg(target_os = "linux")]
                {
                    std::process::Command::new("xdg-open")
                        .arg(&folder_path)
                        .spawn()
                        .map_err(|e| format!("Failed to open folder: {}", e))?;
                }

                log_info!("Successfully opened folder: {}", folder_path);
                Ok(())
            } else {
                log_warn!("Meeting {} has no folder_path set", meeting_id);
                Err("Recording folder path not available for this meeting".to_string())
            }
        }
        None => {
            log_warn!("Meeting not found: {}", meeting_id);
            Err("Meeting not found".to_string())
        }
    }
}

// Simple test command to check backend connectivity
#[tauri::command]
pub async fn test_backend_connection<R: Runtime>(
    app: AppHandle<R>,
    auth_token: Option<String>,
) -> Result<String, String> {
    log_debug!("Testing backend connection...");

    let client = reqwest::Client::new();
    let server_url = get_server_address(&app).await?;

    log_debug!("Testing connection to: {}", server_url);

    let mut request = client.get(&format!("{}/docs", server_url));

    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            log_debug!("Backend responded with status: {}", status);
            Ok(format!("Backend is reachable. Status: {}", status))
        }
        Err(e) => {
            let error_msg = format!("Failed to connect to backend: {}", e);
            log_debug!("{}", error_msg);
            Err(error_msg)
        }
    }
}

#[tauri::command]
pub async fn debug_backend_connection<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    log_debug!("=== DEBUG: Testing backend connection ===");

    // Test 1: Check server address from store
    let server_url = match get_server_address(&app).await {
        Ok(url) => {
            log_debug!("✓ Server URL from store: {}", url);
            url
        }
        Err(e) => {
            log_error!("✗ Failed to get server URL: {}", e);
            return Err(format!("Failed to get server URL: {}", e));
        }
    };

    // Test 2: Make a simple HTTP request to the backend
    let client = reqwest::Client::new();
    let test_url = format!("{}/docs", server_url); // Try the docs endpoint which should be public

    log_debug!("Testing connection to: {}", test_url);

    match client.get(&test_url).send().await {
        Ok(response) => {
            let status = response.status();
            log_debug!("✓ Backend responded with status: {}", status);
            Ok(format!(
                "Backend connection successful! Status: {}, URL: {}",
                status, server_url
            ))
        }
        Err(e) => {
            log_error!("✗ Backend connection failed: {}", e);
            Err(format!("Backend connection failed: {}", e))
        }
    }
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    use std::process::Command;

    let result = if cfg!(target_os = "windows") {
        Command::new("cmd").args(&["/C", "start", &url]).output()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(&url).output()
    } else {
        // Linux and other Unix-like systems
        Command::new("xdg-open").arg(&url).output()
    };

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to open URL: {}", e)),
    }
}

// ===== CUSTOM OPENAI API COMMANDS =====

/// Saves the custom OpenAI configuration
/// This configuration is stored as JSON and includes endpoint, apiKey, model, and optional parameters
#[tauri::command]
pub async fn api_save_custom_openai_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    endpoint: String,
    api_key: Option<String>,
    model: String,
    max_tokens: Option<i32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_custom_openai_config called: endpoint='{}', model='{}'",
        &endpoint,
        &model
    );

    // Validate required fields
    if endpoint.trim().is_empty() {
        return Err("Endpoint URL is required".to_string());
    }
    if model.trim().is_empty() {
        return Err("Model name is required".to_string());
    }

    // Validate endpoint URL format
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err("Endpoint must start with http:// or https://".to_string());
    }

    // Validate optional numeric parameters
    if let Some(temp) = temperature {
        if !(0.0..=2.0).contains(&temp) {
            return Err("Temperature must be between 0.0 and 2.0".to_string());
        }
    }
    if let Some(top) = top_p {
        if !(0.0..=1.0).contains(&top) {
            return Err("Top P must be between 0.0 and 1.0".to_string());
        }
    }
    if let Some(tokens) = max_tokens {
        if tokens < 1 {
            return Err("Max tokens must be at least 1".to_string());
        }
    }

    let config = CustomOpenAIConfig {
        endpoint: endpoint.trim().to_string(),
        api_key: api_key.filter(|k| !k.trim().is_empty()),
        model: model.trim().to_string(),
        max_tokens,
        temperature,
        top_p,
    };

    let pool = state.db_manager.pool();

    match SettingsRepository::save_custom_openai_config(pool, &config).await {
        Ok(()) => {
            log_info!("✅ Successfully saved custom OpenAI config for endpoint: {}", config.endpoint);
            Ok(serde_json::json!({
                "status": "success",
                "message": "Custom OpenAI configuration saved successfully"
            }))
        }
        Err(e) => {
            log_error!("❌ Failed to save custom OpenAI config: {}", e);
            Err(format!("Failed to save custom OpenAI configuration: {}", e))
        }
    }
}

/// Gets the custom OpenAI configuration
#[tauri::command]
pub async fn api_get_custom_openai_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CustomOpenAIConfig>, String> {
    log_info!("api_get_custom_openai_config called");

    let pool = state.db_manager.pool();

    match SettingsRepository::get_custom_openai_config(pool).await {
        Ok(config) => {
            if let Some(ref c) = config {
                log_info!("✅ Found custom OpenAI config: endpoint='{}', model='{}'",
                    c.endpoint, c.model);
            } else {
                log_info!("No custom OpenAI config found");
            }
            Ok(config)
        }
        Err(e) => {
            log_error!("❌ Failed to get custom OpenAI config: {}", e);
            Err(format!("Failed to get custom OpenAI configuration: {}", e))
        }
    }
}

/// Tests the connection to a custom OpenAI-compatible endpoint
/// Makes a minimal request to verify the endpoint is reachable and responds correctly
#[tauri::command]
pub async fn api_test_custom_openai_connection<R: Runtime>(
    _app: AppHandle<R>,
    endpoint: String,
    api_key: Option<String>,
    model: String,
) -> Result<serde_json::Value, String> {
    log_info!("api_test_custom_openai_connection called for model '{}'", &model);

    // Validate endpoint URL format
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err("Endpoint must start with http:// or https://".to_string());
    }

    // Build the URL - append /chat/completions to the base endpoint
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

    // Create a minimal test request
    let test_request = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": "Hi"
            }
        ],
        "max_tokens": 5
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let mut request = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&test_request);

    // Add authorization if API key provided
    if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let response_text = response.text().await.unwrap_or_default();

            if status.is_success() {
                // Parse response as JSON to verify it's a valid OpenAI-compatible response
                match serde_json::from_str::<serde_json::Value>(&response_text) {
                    Ok(json) => {
                        // Verify the response has the expected OpenAI structure
                        if let Some(choices) = json.get("choices") {
                            if let Some(choices_array) = choices.as_array() {
                                if !choices_array.is_empty() {
                                    // Verify the first choice has the required message structure
                                    if let Some(first_choice) = choices_array.get(0) {
                                        // Check if message.content field exists (can be empty string)
                                        let has_message_structure = first_choice
                                            .get("message")
                                            .and_then(|m| {
                                                m.get("content")
                                                .or_else(|| m.get("reasoning_content"))
                                            })
                                            .is_some();

                                        if has_message_structure {
                                            log_info!("✅ Custom OpenAI connection test successful - response validated");
                                            return Ok(serde_json::json!({
                                                "status": "success",
                                                "message": "Connection successful and response validated",
                                                "http_status": status.as_u16()
                                            }));
                                        }
                                    }
                                }
                            }
                        }

                        // Response was 200 but doesn't match OpenAI format
                        log_warn!(
                            "Custom OpenAI endpoint returned HTTP 200 with an incompatible response ({} bytes)",
                            response_text.len()
                        );
                        Err("Endpoint is reachable but doesn't appear to be OpenAI-compatible. Response is missing 'choices' array or 'message.content' / 'message.reasoning_content' field.".to_string())
                    }
                    Err(e) => {
                        log_warn!(
                            "Custom OpenAI endpoint returned invalid JSON ({} bytes): {}",
                            response_text.len(),
                            e
                        );
                        Err("Endpoint is reachable but returned invalid JSON.".to_string())
                    }
                }
            } else {
                log_warn!(
                    "Custom OpenAI connection test failed with HTTP status {} ({} response bytes)",
                    status,
                    response_text.len()
                );
                Err(format!("Connection failed with status {}.", status))
            }
        }
        Err(e) => {
            log_error!("❌ Custom OpenAI connection test failed: {}", e);
            if e.is_timeout() {
                Err("Connection timed out. Please check the endpoint URL.".to_string())
            } else if e.is_connect() {
                Err("Could not connect to endpoint. Please verify the URL is correct and the server is running.".to_string())
            } else {
                Err(format!("Connection failed: {}", e))
            }
        }
    }
}

// ===== GITHUB COPILOT CLI API COMMANDS =====

/// Saves the GitHub Copilot CLI configuration
/// Stored as JSON: {binaryPath, model, githubToken} - all fields optional
#[tauri::command]
pub async fn api_save_copilot_cli_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    binary_path: Option<String>,
    model: Option<String>,
    github_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_copilot_cli_config called: binaryPath={:?}, model={:?}",
        &binary_path,
        &model
    );

    if let Some(model) = model.as_deref() {
        crate::summary::copilot_cli::validate_copilot_model(model)?;
    }

    let config = CopilotCliConfig {
        binary_path: binary_path.filter(|p| !p.trim().is_empty()),
        model: model.filter(|m| !m.trim().is_empty()),
        github_token: github_token.filter(|t| !t.trim().is_empty()),
    };

    let pool = state.db_manager.pool();

    match SettingsRepository::save_copilot_cli_config(pool, &config).await {
        Ok(()) => {
            log_info!("✅ Successfully saved Copilot CLI config");
            Ok(serde_json::json!({
                "status": "success",
                "message": "GitHub Copilot CLI configuration saved successfully"
            }))
        }
        Err(e) => {
            log_error!("❌ Failed to save Copilot CLI config: {}", e);
            Err(format!("Failed to save Copilot CLI configuration: {}", e))
        }
    }
}

/// Gets the GitHub Copilot CLI configuration
#[tauri::command]
pub async fn api_get_copilot_cli_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CopilotCliConfig>, String> {
    log_info!("api_get_copilot_cli_config called");

    let pool = state.db_manager.pool();

    match SettingsRepository::get_copilot_cli_config(pool).await {
        Ok(config) => Ok(config),
        Err(e) => {
            log_error!("❌ Failed to get Copilot CLI config: {}", e);
            Err(format!("Failed to get Copilot CLI configuration: {}", e))
        }
    }
}

/// Tests the GitHub Copilot CLI by running a minimal prompt through it
/// Verifies the binary is found, authentication works, and the model is available
#[tauri::command]
pub async fn api_test_copilot_cli<R: Runtime>(
    _app: AppHandle<R>,
    binary_path: Option<String>,
    model: Option<String>,
    github_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_test_copilot_cli called: binaryPath={:?}, model={:?}",
        &binary_path,
        &model
    );

    let _response = crate::summary::copilot_cli::generate_with_copilot_cli(
        binary_path.as_deref(),
        model.as_deref().unwrap_or(""),
        github_token.as_deref(),
        "You are a connection test.",
        "Reply with exactly: OK",
        None,
    )
    .await?;

    log_info!("✅ Copilot CLI test successful");
    Ok(serde_json::json!({
        "status": "success",
        "message": "GitHub Copilot CLI responded successfully"
    }))
}
