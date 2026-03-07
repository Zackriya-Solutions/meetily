use crate::database::repositories::speaker::{NameSuggestion, ResolvedSpeaker, SpeakerProfile, SpeakerRepository};
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_speaker_profiles(
    state: State<'_, AppState>,
) -> Result<Vec<SpeakerProfile>, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::list_profiles(pool).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_self_profile(
    state: State<'_, AppState>,
) -> Result<Option<SpeakerProfile>, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::get_self_profile(pool).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_speaker_profile(
    state: State<'_, AppState>,
    name: String,
    color: String,
    is_self: Option<bool>,
    global_auto_apply: Option<bool>,
) -> Result<SpeakerProfile, String> {
    let pool = state.db_manager.pool();
    let is_self_val = is_self.unwrap_or(false);
    let auto_apply = global_auto_apply.unwrap_or(is_self_val); // self profiles auto-apply by default
    // Only one self-profile allowed — clear previous if setting new one
    if is_self_val {
        sqlx::query("UPDATE speaker_profiles SET is_self = 0 WHERE is_self = 1")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    SpeakerRepository::create_profile(pool, &name, &color, is_self_val, auto_apply)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_speaker_profile(
    state: State<'_, AppState>,
    id: String,
    name: String,
    color: String,
    is_self: Option<bool>,
    global_auto_apply: Option<bool>,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let is_self_val = is_self.unwrap_or(false);
    let auto_apply = global_auto_apply.unwrap_or(is_self_val);
    if is_self_val {
        sqlx::query("UPDATE speaker_profiles SET is_self = 0 WHERE is_self = 1 AND id != ?")
            .bind(&id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    SpeakerRepository::update_profile(pool, &id, &name, &color, is_self_val, auto_apply)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_speaker_profile(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::delete_profile(pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_speaker_mapping(
    state: State<'_, AppState>,
    meeting_id: String,
    speaker_id: String,
    profile_id: String,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::set_mapping(pool, &meeting_id, &speaker_id, &profile_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_speaker_mapping(
    state: State<'_, AppState>,
    meeting_id: String,
    speaker_id: String,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::remove_mapping(pool, &meeting_id, &speaker_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_meeting_speakers(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<ResolvedSpeaker>, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::get_resolved_speakers(pool, &meeting_id)
        .await
        .map_err(|e| e.to_string())
}

/// Detect speaker names from transcript text patterns (e.g. "I'm James", "my name is Sarah").
#[tauri::command]
pub async fn detect_speaker_names(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<NameSuggestion>, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::detect_names_from_transcripts(pool, &meeting_id)
        .await
        .map_err(|e| e.to_string())
}

/// Ensure mic is mapped for this meeting (auto-applies self-profile if one exists).
#[tauri::command]
pub async fn ensure_mic_mapping(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<SpeakerProfile>, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::ensure_mic_mapping(pool, &meeting_id)
        .await
        .map_err(|e| e.to_string())
}

