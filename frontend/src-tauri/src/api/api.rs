use log::{debug as log_debug, error as log_error, info as log_info, warn as log_warn};
use crate::utils::url_origin_for_log;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use crate::{
    database::{
        models::MeetingModel,
        repositories::{
            meeting::MeetingsRepository, setting::SettingsRepository,
            transcript::TranscriptsRepository,
        },
    },
    state::AppState,
    summary::CustomOpenAIConfig,
};

// Hardcoded server URL
const APP_SERVER_URL: &str = "http://localhost:5167";

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
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
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
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
    /// "mic" (the local user) or "system" (the other side); None for old rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// Which voice on the other side ("Speaker 2"); None for the mic and old rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_label: Option<String>,
    /// The name Dor gave this line or its voice; None until named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_name: Option<String>,
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
    /// "mic" (the local user) or "system" (the other side); None for old rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// Which voice on the other side ("Speaker 2"); None for the mic and old rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_label: Option<String>,
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
        Some(token) if token.as_str().is_some() => {
            log_info!("Auth token loaded from store");
            token.as_str().map(str::to_owned)
        }
        Some(_) => {
            log_warn!("Auth token in store is not a string");
            None
        }
        None => {
            log_warn!("No auth token found in store");
            None
        }
    }
}

// Helper function to get server address - now hardcoded
async fn get_server_address<R: Runtime>(_app: &AppHandle<R>) -> Result<String, String> {
    log_info!(
        "Using configured API origin={}",
        url_origin_for_log(APP_SERVER_URL)
    );
    Ok(APP_SERVER_URL.to_string())
}

// Generic API call function with optional authentication
fn reqwest_error_class(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_status() {
        "status"
    } else if error.is_decode() {
        "decode"
    } else {
        "request"
    }
}
async fn make_api_request<R: Runtime, T: for<'de> Deserialize<'de>>(
    app: &AppHandle<R>,
    endpoint: &str,
    method: &str,
    body: Option<&str>,
    additional_headers: Option<HashMap<String, String>>,
    auth_token: Option<String>,
) -> Result<T, String> {
    let client = reqwest::Client::new();
    let server_url = get_server_address(app).await?;
    let url = format!("{}{}", server_url, endpoint);
    let origin = url_origin_for_log(&url);
    log_info!(
        "API request started; method={}, route={}, origin={}",
        method,
        endpoint.split('?').next().unwrap_or(endpoint),
        origin
    );

    let mut request = match method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => return Err(format!("Unsupported HTTP method: {}", method)),
    };

    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {}", token));
    } else {
        log_warn!("API request has no authorization token");
    }

    request = request.header("Content-Type", "application/json");
    if let Some(headers) = additional_headers {
        for (key, value) in headers {
            request = request.header(&key, &value);
        }
    }
    if let Some(body_str) = body {
        request = request.body(body_str.to_string());
    }

    let response = request.send().await.map_err(|error| {
        log_error!(
            "API request failed; method={}, route={}, origin={}, error_class={}",
            method,
            endpoint.split('?').next().unwrap_or(endpoint),
            origin,
            reqwest_error_class(&error)
        );
        format!("Request failed: {}", error)
    })?;
    let status = response.status();

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        log_error!(
            "API response failed; method={}, route={}, origin={}, status={}, response_bytes={}",
            method,
            endpoint.split('?').next().unwrap_or(endpoint),
            origin,
            status,
            error_text.len()
        );
        return Err(format!("HTTP {}: {}", status, error_text));
    }

    let response_text = response.text().await.map_err(|error| {
        log_error!(
            "API response read failed; method={}, route={}, origin={}, error_class={}",
            method,
            endpoint.split('?').next().unwrap_or(endpoint),
            origin,
            reqwest_error_class(&error)
        );
        format!("Failed to read response: {}", error)
    })?;
    log_info!(
        "API response received; method={}, route={}, origin={}, status={}, response_bytes={}",
        method,
        endpoint.split('?').next().unwrap_or(endpoint),
        origin,
        status,
        response_text.len()
    );

    serde_json::from_str(&response_text).map_err(|error| {
        log_error!(
            "API response parse failed; method={}, route={}, origin={}, status={}, error_class=json",
            method,
            endpoint.split('?').next().unwrap_or(endpoint),
            origin,
            status
        );
        format!("Failed to parse JSON: {}", error)
    })
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
    let meetings: Result<Vec<MeetingModel>, sqlx::Error> =
        MeetingsRepository::get_meetings(pool).await;

    match meetings {
        Ok(meeting_models) => {
            log_info!("Successfully got {} meetings", meeting_models.len());

            let result: Vec<Meeting> = meeting_models
                .into_iter()
                .map(|m| Meeting {
                    id: m.id,
                    title: m.title,
                })
                .collect();
            Ok(result)
        }
        Err(e) => {
            log_error!("Error getting meetings: {}", e);
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
        "Transcript search requested; query_chars={}, auth_present={}",
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
            log_error!("Transcript search failed");
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
    log_info!("Profile lookup requested; auth_present={}", auth_token.is_some());

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
    log_info!("Profile save requested; auth_present={}", auth_token.is_some());

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
    log_info!("Profile update requested; auth_present={}", auth_token.is_some());

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
                "Model config loaded; provider={}, model={}, whisper_model={}, endpoint_configured={}",
                config.provider,
                config.model,
                config.whisper_model,
                config.ollama_endpoint.is_some()
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
        "Model config save requested; provider={}, model={}, whisper_model={}, endpoint_configured={}",
        provider,
        model,
        whisper_model,
        ollama_endpoint.is_some()
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
                    Ok(Some(TranscriptConfig {
                        provider: config.provider,
                        model: config.model,
                        api_key,
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
                api_key: None,
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
    api_key: Option<String>,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "Transcript config save requested; provider={}, model={}",
        provider,
        model
    );
    let pool = state.db_manager.pool();

    if let Err(e) = SettingsRepository::save_transcript_config(pool, &provider, &model).await {
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
    _app: AppHandle<R>,
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

/// Names of the people on the other side of a meeting, found in its transcript.
#[tauri::command]
pub async fn api_get_meeting_speakers(
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    crate::summary::speaker_names::get_other_speaker_names(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to read speaker names: {}", e))
}

/// Names Dor sets or clears on the meeting page; automatic passes keep them.
#[tauri::command]
pub async fn api_set_meeting_speakers(
    meeting_id: String,
    names: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    crate::summary::speaker_names::set_other_speaker_names(state.db_manager.pool(), &meeting_id, &names)
        .await
        .map_err(|e| format!("Failed to save speaker names: {}", e))
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
            Ok(MeetingMetadata {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                folder_path: meeting.folder_path,
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
                    speaker_label: t.speaker_label,
                    speaker_name: t.speaker_name,
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

#[tauri::command]
pub async fn api_save_meeting_title<R: Runtime>(
    _app: AppHandle<R>,
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
            Ok(serde_json::json!({"message": "Meeting title saved successfully"}))
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

#[tauri::command]
pub async fn api_save_transcript<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_title: String,
    transcripts: Vec<serde_json::Value>,
    folder_path: Option<String>,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "Transcript save requested; transcript_count={}, folder_configured={}, auth_present={}",
        transcripts.len(),
        folder_path.is_some(),
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


    let pool = state.db_manager.pool();
    let transcript_text = transcripts_to_save
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    // Now, call the repository with the correctly typed data.
    match TranscriptsRepository::save_transcript(
        pool,
        &meeting_title,
        &transcripts_to_save,
        folder_path,
    )
    .await
    {
        Ok(meeting_id) => {
            log_info!(
                "Successfully saved transcript and created meeting with id: {}",
                meeting_id
            );
            // Title first, then names: both use the summary model, which the
            // built-in AI runs one request at a time.
            let (app_bg, pool_bg, meeting_bg) = (app.clone(), pool.clone(), meeting_id.clone());
            tauri::async_runtime::spawn(async move {
                crate::summary::title::generate_meeting_title(
                    app_bg.clone(),
                    pool_bg.clone(),
                    meeting_bg.clone(),
                    transcript_text,
                    None,
                )
                .await;
                crate::summary::speaker_names::identify_other_speakers(app_bg, pool_bg, meeting_bg).await;
            });
            Ok(serde_json::json!({
                "status": "success",
                "message": "Transcript saved successfully",
                "meeting_id": meeting_id
            }))
        }
        Err(e) => {
            log_error!("Transcript save failed");
            Err(format!("Failed to save transcript: {}", e))
        }
    }
}

/// Trims a typed speaker name; blank clears it.
fn clean_speaker_name(name: Option<String>) -> Option<String> {
    name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty())
}

/// Names every line one automatic speaker label carries in a meeting
/// ("Speaker 2" is Alex), except lines named one at a time, which keep their
/// own name. A blank name clears them. Returns the ids changed.
#[tauri::command]
pub async fn name_meeting_speaker(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    speaker_label: String,
    name: Option<String>,
) -> Result<Vec<String>, String> {
    let name = clean_speaker_name(name);
    let pool = state.db_manager.pool();
    let changed = sqlx::query_scalar(
        "UPDATE transcripts SET speaker_name = ?
         WHERE meeting_id = ? AND speaker_label = ? AND COALESCE(speaker_name_pinned, 0) = 0
         RETURNING id",
    )
    .bind(&name)
    .bind(&meeting_id)
    .bind(&speaker_label)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;
    // The names are saved either way: a missing or unreadable voiceprint
    // only means this meeting adds nothing to the person's profile.
    if let Err(e) = sync_voice_sample(pool, &meeting_id, &speaker_label).await {
        log_warn!("Speaker named, but its voice was not kept: {}", e);
    }
    Ok(changed)
}

/// Keeps a speaker's voice as a sample of the person it is named after, from
/// the voiceprints saved with the meeting. The name comes from the lines
/// themselves: lines named one at a time are left out, and the sample is kept
/// only while the rest carry exactly one name. So clearing or renaming the
/// lines, from either command, corrects or drops the sample, and a voice
/// whose lines were split between people (all pinned) adds to no one.
async fn sync_voice_sample(pool: &sqlx::SqlitePool, meeting_id: &str, speaker_label: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM voice_samples WHERE meeting_id = ? AND speaker_label = ?")
        .bind(meeting_id)
        .bind(speaker_label)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT speaker_name FROM transcripts
         WHERE meeting_id = ? AND speaker_label = ? AND speaker_name IS NOT NULL
           AND COALESCE(speaker_name_pinned, 0) = 0",
    )
    .bind(meeting_id)
    .bind(speaker_label)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let [name] = names.as_slice() else { return Ok(()) };
    let folder: Option<String> = sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .flatten();
    let Some(folder) = folder else { return Ok(()) };
    let voiceprint = crate::diarization::voiceprints::read_voiceprints(std::path::Path::new(&folder))
        .map_err(|e| format!("reading voiceprints: {}", e))?
        .into_iter()
        .find(|v| v.label == speaker_label);
    let Some(voiceprint) = voiceprint else { return Ok(()) };
    sqlx::query("INSERT INTO voice_samples (meeting_id, speaker_label, name, segments, embedding) VALUES (?, ?, ?, ?, ?)")
        .bind(meeting_id)
        .bind(speaker_label)
        .bind(name)
        .bind(voiceprint.segments as i64)
        .bind(serde_json::to_string(&voiceprint.embedding).map_err(|e| e.to_string())?)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Names one transcript line on its own, so naming its whole voice later
/// leaves it. A blank name clears it and lets it follow its voice again.
#[tauri::command]
pub async fn set_transcript_speaker_name(
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    name: Option<String>,
) -> Result<(), String> {
    let name = clean_speaker_name(name);
    let pool = state.db_manager.pool();
    let line: Option<(String, Option<String>)> = sqlx::query_as(
        "UPDATE transcripts SET speaker_name = ?, speaker_name_pinned = ? WHERE id = ?
         RETURNING meeting_id, speaker_label",
    )
    .bind(&name)
    .bind(name.is_some() as i64)
    .bind(&transcript_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;
    let Some((meeting_id, speaker_label)) = line else {
        return Err(format!("Transcript line {} not found", transcript_id));
    };
    if let Some(label) = speaker_label {
        if let Err(e) = sync_voice_sample(pool, &meeting_id, &label).await {
            log_warn!("Speaker line named, but its voice sample was not updated: {}", e);
        }
    }
    Ok(())
}

/// How often the automatic labels agree with the names given, over every
/// meeting with named lines (see diarization::accuracy).
#[tauri::command]
pub async fn get_speaker_accuracy(
    state: tauri::State<'_, AppState>,
) -> Result<crate::diarization::accuracy::SpeakerAccuracy, String> {
    let rows: Vec<(String, Option<String>, Option<String>, Option<f64>)> = sqlx::query_as(
        "SELECT meeting_id, speaker_label, speaker_name, duration FROM transcripts WHERE speaker_name IS NOT NULL",
    )
    .fetch_all(state.db_manager.pool())
    .await
    .map_err(|e| format!("Database error: {}", e))?;
    let mut guesses: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (meeting_id, ..) in &rows {
        if !guesses.contains_key(meeting_id) {
            let found = meeting_speaker_guesses(state.db_manager.pool(), meeting_id).await;
            guesses.insert(meeting_id.clone(), found);
        }
    }
    let lines: Vec<_> = rows
        .into_iter()
        .map(|(meeting_id, speaker_label, speaker_name, duration)| {
            let speaker_guess = speaker_label.as_ref().and_then(|l| guesses[&meeting_id].get(l).cloned());
            crate::diarization::accuracy::ScoredLine { meeting_id, speaker_label, speaker_name, duration, speaker_guess }
        })
        .collect();
    Ok(crate::diarization::accuracy::speaker_accuracy(&lines))
}

/// The names the app guessed for a meeting's voices, by label, from the
/// voiceprints saved with it. Empty when it has none or they can't be read.
async fn meeting_speaker_guesses(pool: &sqlx::SqlitePool, meeting_id: &str) -> HashMap<String, String> {
    let folder: Option<String> = sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .flatten();
    let Some(folder) = folder else { return HashMap::new() };
    match crate::diarization::voiceprints::read_voiceprints(std::path::Path::new(&folder)) {
        Ok(voices) => voices.into_iter().filter_map(|v| Some((v.label, v.name?))).collect(),
        Err(e) => {
            log_warn!("Speaker guesses not read: {}", e);
            HashMap::new()
        }
    }
}

/// The guessed names of the recording in progress, by label, for a page
/// reloaded mid-call.
#[tauri::command]
pub async fn get_live_speaker_guesses() -> Result<HashMap<String, String>, String> {
    Ok(crate::diarization::session::current_guesses())
}

/// The names the app guessed for a meeting's voices, by label, shown where
/// Dor has not named a line himself.
#[tauri::command]
pub async fn get_meeting_speaker_guesses(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<HashMap<String, String>, String> {
    Ok(meeting_speaker_guesses(state.db_manager.pool(), &meeting_id).await)
}

/// Path of the meeting's recorded audio, or None when nothing was saved
/// (auto-save off, or the folder was moved). Recordings live in the user's
/// chosen folder, outside the asset protocol's static scope, so the file is
/// allowed for this session before the player loads it.
#[tauri::command]
pub async fn get_meeting_audio_path<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<String>, String> {
    use tauri::Manager;

    let folder_path: Option<Option<String>> =
        sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_optional(state.db_manager.pool())
            .await
            .map_err(|e| format!("Database error: {}", e))?;
    let Some(folder) = folder_path.flatten() else {
        return Ok(None);
    };
    let Ok(audio) = crate::audio::retranscription::find_audio_file(std::path::Path::new(&folder)) else {
        return Ok(None);
    };
    app.asset_protocol_scope()
        .allow_file(&audio)
        .map_err(|e| format!("Failed to allow audio file: {}", e))?;
    Ok(Some(audio.to_string_lossy().into_owned()))
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
                log_info!("Opening meeting folder; meeting_id={}", meeting_id);

                // Verify folder exists
                let path = std::path::Path::new(&folder_path);
                if !path.exists() {
                    log_warn!("Meeting folder path does not exist; meeting_id={}", meeting_id);
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

                log_info!("Meeting folder opened; meeting_id={}", meeting_id);
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

    log_debug!("Testing connection to origin={}", url_origin_for_log(&server_url));

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
            log_debug!(
                "Backend connection failed; error_class={}",
                reqwest_error_class(&e)
            );
            Err(format!("Failed to connect to backend: {}", e))
        }
    }
}

#[tauri::command]
pub async fn debug_backend_connection<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    log_debug!("=== DEBUG: Testing backend connection ===");

    // Test 1: Check server address from store
    let server_url = match get_server_address(&app).await {
        Ok(url) => {
            log_debug!("Configured backend origin={}", url_origin_for_log(&url));
            url
        }
        Err(e) => {
            log_error!("Failed to resolve configured backend origin");
            return Err(format!("Failed to get server URL: {}", e));
        }
    };

    // Test 2: Make a simple HTTP request to the backend
    let client = reqwest::Client::new();
    let test_url = format!("{}/docs", server_url); // Try the docs endpoint which should be public

    log_debug!("Testing connection to origin={}", url_origin_for_log(&test_url));

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
            log_error!(
                "Backend connection failed; error_class={}",
                reqwest_error_class(&e)
            );
            Err(format!("Backend connection failed: {}", e))
        }
    }
}

fn parse_external_url(raw: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(raw).map_err(|_| "Invalid external URL".to_string())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("Only HTTPS external URLs are allowed".to_string());
    }
    Ok(url)
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    let url = parse_external_url(&url)?;
    // Desktop launchers can wait for a browser; never run them on an async worker.
    tauri::async_runtime::spawn_blocking(move || {
        // This feature uses ShellExecute on Windows, not cmd /C start.
        #[cfg(windows)]
        let result = open::that_detached(url.as_str());
        #[cfg(not(windows))]
        let result = open::that(url.as_str());
        result.map_err(|error| format!("Failed to open URL: {error}"))
    })
    .await
    .map_err(|error| format!("Failed to open URL: {error}"))?
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
        "Custom OpenAI config save requested; endpoint_origin={}, model={}",
        url_origin_for_log(&endpoint),
        model
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
            log_info!(
                "Custom OpenAI config saved; endpoint_origin={}",
                url_origin_for_log(&config.endpoint)
            );
            Ok(serde_json::json!({
                "status": "success",
                "message": "Custom OpenAI configuration saved successfully"
            }))
        }
        Err(e) => {
            log_error!("Failed to save custom OpenAI config");
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
            if let Some(config) = &config {
                log_info!(
                    "Custom OpenAI config loaded; endpoint_origin={}, model={}",
                    url_origin_for_log(&config.endpoint),
                    config.model
                );
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
    let endpoint_origin = url_origin_for_log(&endpoint);
    log_info!(
        "Custom OpenAI connection test requested; endpoint_origin={}, model={}",
        endpoint_origin,
        model
    );

    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err("Endpoint must start with http:// or https://".to_string());
    }

    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
    let test_request = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 5
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Failed to create HTTP client: {}", error))?;
    let mut request = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&test_request);

    if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let response_text = response.text().await.unwrap_or_default();
            log_info!(
                "Custom OpenAI connection response; endpoint_origin={}, status={}, response_bytes={}",
                endpoint_origin,
                status,
                response_text.len()
            );

            if status.is_success() {
                match serde_json::from_str::<serde_json::Value>(&response_text) {
                    Ok(json) if json
                        .get("choices")
                        .and_then(serde_json::Value::as_array)
                        .and_then(|choices| choices.first())
                        .and_then(|choice| choice.get("message"))
                        .and_then(|message| {
                            message
                                .get("content")
                                .or_else(|| message.get("reasoning_content"))
                        })
                        .is_some() =>
                    {
                        log_info!(
                            "Custom OpenAI connection validated; endpoint_origin={}, status={}",
                            endpoint_origin,
                            status
                        );
                        Ok(serde_json::json!({
                            "status": "success",
                            "message": "Connection successful and response validated",
                            "http_status": status.as_u16()
                        }))
                    }
                    Ok(_) => {
                        log_warn!(
                            "Custom OpenAI response is not compatible; endpoint_origin={}, status={}",
                            endpoint_origin,
                            status
                        );
                        Err("Endpoint is reachable but doesn't appear to be OpenAI-compatible. Response is missing 'choices' array or 'message.content' / 'message.reasoning_content' field.".to_string())
                    }
                    Err(error) => {
                        log_warn!(
                            "Custom OpenAI response is invalid JSON; endpoint_origin={}, status={}",
                            endpoint_origin,
                            status
                        );
                        Err(format!("Endpoint is reachable but returned invalid JSON: {}. Response: {}", error, response_text))
                    }
                }
            } else {
                log_warn!(
                    "Custom OpenAI connection failed; endpoint_origin={}, status={}",
                    endpoint_origin,
                    status
                );
                Err(format!("Connection failed with status {}: {}", status, response_text))
            }
        }
        Err(error) => {
            log_error!(
                "Custom OpenAI connection request failed; endpoint_origin={}, error_class={}",
                endpoint_origin,
                reqwest_error_class(&error)
            );
            if error.is_timeout() {
                Err("Connection timed out. Please check the endpoint URL.".to_string())
            } else if error.is_connect() {
                Err("Could not connect to endpoint. Please verify the URL is correct and the server is running.".to_string())
            } else {
                Err(format!("Connection failed: {}", error))
            }
        }
    }
}

#[cfg(test)]
mod external_url_tests {
    use super::parse_external_url;

    #[test]
    fn external_url_preserves_https_query_parameters_and_escapes() {
        let raw = "https://example.invalid/path?a=1&b=2&literal=%25PATH%25#section";
        let url = parse_external_url(raw).unwrap();
        assert_eq!(url.as_str(), raw);
        assert_eq!(url.query_pairs().collect::<Vec<_>>(), [
            ("a".into(), "1".into()),
            ("b".into(), "2".into()),
            ("literal".into(), "%PATH%".into()),
        ]);
    }

    #[test]
    fn external_url_rejects_invalid_urls_and_non_https_schemes() {
        for raw in [
            "", "not a URL", "/relative", "//example.invalid", "https://",
            "http://example.invalid", "javascript:alert(1)", "data:text/html,test",
            "file:///C:/Windows/System32/cmd.exe", "mailto:test@example.invalid",
        ] {
            assert!(parse_external_url(raw).is_err(), "accepted {raw:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diarization::voiceprints::{write_voiceprints, Voiceprint};

    #[tokio::test]
    async fn a_voice_is_kept_only_while_its_lines_carry_one_name() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::database::manager::ensure_voice_samples_table(&pool).await.unwrap();
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY, folder_path TEXT)").execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE transcripts (id TEXT PRIMARY KEY, meeting_id TEXT, speaker_label TEXT,
             speaker_name TEXT, speaker_name_pinned INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transcripts VALUES ('a', 'm1', 'Speaker 2', NULL, NULL), ('b', 'm1', 'Speaker 2', NULL, NULL),
             ('c', 'm1', 'Speaker 9', 'Nobody', NULL), ('d', 'm2', 'Speaker 1', 'Sam', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let folder = tempfile::tempdir().unwrap();
        write_voiceprints(
            folder.path(),
            &[Voiceprint { label: "Speaker 2".into(), segments: 7, embedding: vec![0.6, 0.8], name: None }],
        )
        .unwrap();
        sqlx::query("INSERT INTO meetings VALUES ('m1', ?), ('m2', NULL)")
            .bind(folder.path().to_string_lossy().to_string())
            .execute(&pool)
            .await
            .unwrap();
        let samples = || async {
            sqlx::query_as::<_, (String, String, i64, String)>(
                "SELECT meeting_id, name, segments, embedding FROM voice_samples ORDER BY meeting_id",
            )
            .fetch_all(&pool)
            .await
            .unwrap()
        };

        let name_lines = |sql: &'static str| {
            let pool = pool.clone();
            async move { sqlx::query(sql).execute(&pool).await.unwrap() }
        };
        name_lines("UPDATE transcripts SET speaker_name = 'Alexa' WHERE id IN ('a', 'b')").await;
        sync_voice_sample(&pool, "m1", "Speaker 2").await.unwrap();
        sync_voice_sample(&pool, "m1", "Speaker 9").await.unwrap();
        sync_voice_sample(&pool, "m2", "Speaker 1").await.unwrap();
        assert_eq!(samples().await, vec![("m1".into(), "Alexa".into(), 7, "[0.6,0.8]".into())]);

        // One line pinned to someone else leaves the rest speaking for the voice.
        name_lines("UPDATE transcripts SET speaker_name = 'Sam', speaker_name_pinned = 1 WHERE id = 'b'").await;
        sync_voice_sample(&pool, "m1", "Speaker 2").await.unwrap();
        assert_eq!(samples().await.len(), 1);

        // Every line pinned or cleared: the voice speaks for no one.
        name_lines("UPDATE transcripts SET speaker_name = NULL WHERE id = 'a'").await;
        sync_voice_sample(&pool, "m1", "Speaker 2").await.unwrap();
        assert!(samples().await.is_empty());

        // Two names on the voice's own lines: no sample either.
        name_lines("UPDATE transcripts SET speaker_name = 'Alexa', speaker_name_pinned = 0 WHERE id = 'a'").await;
        name_lines("UPDATE transcripts SET speaker_name = 'Dana', speaker_name_pinned = 0 WHERE id = 'b'").await;
        sync_voice_sample(&pool, "m1", "Speaker 2").await.unwrap();
        assert!(samples().await.is_empty());
    }
}
