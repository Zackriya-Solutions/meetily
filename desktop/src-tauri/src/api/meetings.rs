use crate::{api::api, state::AppState};
use tauri::{AppHandle, Runtime};

#[tauri::command]
pub async fn meetings_list<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    auth_token: Option<String>,
) -> Result<Vec<api::Meeting>, String> {
    api::api_get_meetings(app, state, auth_token).await
}

#[tauri::command]
pub async fn transcript_search<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    query: String,
    auth_token: Option<String>,
) -> Result<Vec<api::TranscriptSearchResult>, String> {
    // Deprecated compatibility wrapper. Prefer `transcript_search_with_filters`.
    api::api_search_transcripts(app, state, query, auth_token).await
}

#[tauri::command]
pub async fn transcript_search_with_filters<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    request: api::TranscriptSearchRequest,
    auth_token: Option<String>,
) -> Result<api::TranscriptSearchResponse, String> {
    api::api_search_transcripts_with_filters(app, state, request, auth_token).await
}

#[tauri::command]
pub async fn meeting_delete<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    api::api_delete_meeting(app, state, meeting_id, auth_token).await
}

#[tauri::command]
pub async fn meeting_get<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
    auth_token: Option<String>,
) -> Result<api::MeetingDetails, String> {
    api::api_get_meeting(app, meeting_id, state, auth_token).await
}

#[tauri::command]
pub async fn meeting_meta_get<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<api::MeetingMetadata, String> {
    api::api_get_meeting_metadata(app, meeting_id, state).await
}

#[tauri::command]
pub async fn meeting_transcripts_get<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    limit: i64,
    offset: i64,
    state: tauri::State<'_, AppState>,
) -> Result<api::PaginatedTranscriptsResponse, String> {
    api::api_get_meeting_transcripts(app, meeting_id, limit, offset, state).await
}

#[tauri::command]
pub async fn meeting_title_set<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    title: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    api::api_save_meeting_title(app, state, meeting_id, title, auth_token).await
}

#[tauri::command]
pub async fn transcript_save<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_title: String,
    transcripts: Vec<serde_json::Value>,
    folder_path: Option<String>,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    api::api_save_transcript(
        app,
        state,
        meeting_title,
        transcripts,
        folder_path,
        auth_token,
    )
    .await
}

#[tauri::command]
pub async fn meeting_folder_open<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<(), String> {
    api::open_meeting_folder(app, state, meeting_id).await
}

#[tauri::command]
pub async fn external_url_open(url: String) -> Result<(), String> {
    api::open_external_url(url).await
}
