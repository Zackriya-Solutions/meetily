use crate::audio::{recording_saver::TranscriptSegment, RecordingState};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[cfg(test)]
mod persistence_tests;

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub phase: String,
    pub started_at: Option<DateTime<Utc>>,
    pub folder: Option<PathBuf>,
    pub state: Option<Arc<RecordingState>>,
    pub transcripts: Arc<Mutex<Vec<TranscriptSegment>>>,
    pub processing_error: Arc<Mutex<Option<String>>>,
    pub pauses: Arc<Mutex<Vec<(f64, Option<f64>)>>>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static LAST: Mutex<Option<Value>> = Mutex::new(None);
static COMPLETION: tokio::sync::Notify = tokio::sync::Notify::const_new();

pub fn current() -> Option<Session> {
    SESSION.lock().unwrap().clone()
}

pub fn snapshot() -> Option<Value> {
    current()
        .map(|s| {
            json!({
                "sessionId": s.id,
                "meetingTitle": s.title,
                "phase": s.phase,
                "startedAt": s.started_at,
            })
        })
        .or_else(|| LAST.lock().unwrap().clone())
}

pub async fn reserve<R: Runtime>(
    app: &AppHandle<R>,
    title: String,
) -> Result<String, String> {
    let state = app
        .try_state::<crate::state::AppState>()
        .ok_or("Finish database setup before recording.")?;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut slot = SESSION.lock().unwrap();
        if slot.is_some() {
            return Err("Recording already in progress (starting, active, or stopping).".into());
        }
        *slot = Some(Session {
            id: id.clone(),
            title: title.clone(),
            phase: "starting".into(),
            started_at: None,
            folder: None,
            state: None,
            transcripts: Arc::new(Mutex::new(Vec::new())),
            processing_error: Arc::new(Mutex::new(None)),
            pauses: Arc::new(Mutex::new(Vec::new())),
        });
    }

    let result: Result<(), String> = async {
        sqlx::query("INSERT INTO recording_sessions (id, title, status) VALUES (?, ?, 'starting')")
            .bind(&id)
            .bind(&title)
            .execute(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;

    if let Err(e) = result {
        release(&id);
        return Err(e);
    }
    emit(app);
    Ok(id)
}

fn release(id: &str) {
    let mut slot = SESSION.lock().unwrap();
    if slot.as_ref().is_some_and(|s| s.id == id) {
        *slot = None;
    }
}

fn emit<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit("recording-session-changed", snapshot());
    crate::tray::update_tray_menu(app);
}

pub async fn fail<R: Runtime>(app: &AppHandle<R>, id: &str, error: &str) {
    let session = current().filter(|s| s.id == id);
    let mut result = LAST
        .lock()
        .unwrap()
        .clone()
        .filter(|r| r["sessionId"] == id)
        .unwrap_or_else(|| json!({"sessionId": id}));
    if let Some(s) = &session {
        result["meetingTitle"] = json!(s.title);
        result["folderPath"] = json!(s.folder.as_ref().map(|p| p.to_string_lossy().to_string()));
        result["transcriptCount"] = json!(s.transcripts.lock().unwrap().len());
    }
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        let _ = sqlx::query(
            "UPDATE recording_sessions SET status='error', error=?, ended_at=? WHERE id=?",
        )
        .bind(error)
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(state.db_manager.pool())
        .await;

        if let Ok(Some(meeting)) = sqlx::query_scalar::<_, Option<String>>(
            "SELECT meeting_id FROM recording_sessions WHERE id=?",
        )
        .bind(id)
        .fetch_optional(state.db_manager.pool())
        .await
        {
            result["meetingId"] = json!(meeting);
        }
    }
    result["phase"] = json!("error");
    result["error"] = json!(error);
    *LAST.lock().unwrap() = Some(result.clone());
    release(id);
    COMPLETION.notify_waiters();
    emit(app);
    if result["meetingId"].is_string() {
        let _ = app.emit("meeting-saved", &result);
    }
    let notify_app = app.clone();
    tauri::async_runtime::spawn(async move {
        notify_completion(&notify_app, &result).await;
    });
}

pub fn attach(
    id: &str,
    state: Arc<RecordingState>,
    folder: Option<PathBuf>,
    transcripts: Arc<Mutex<Vec<TranscriptSegment>>>,
) {
    let mut slot = SESSION.lock().unwrap();
    if let Some(s) = slot.as_mut().filter(|s| s.id == id) {
        let started = state.capture_start().unwrap_or_else(Utc::now);
        s.state = Some(state);
        s.folder = folder;
        s.transcripts = transcripts;
        s.started_at = Some(started);
    }
}

pub fn capture_ready(id: &str) {
    if let Some(s) = SESSION
        .lock()
        .unwrap()
        .as_mut()
        .filter(|s| s.id == id && s.phase == "starting")
    {
        s.phase = "recording".into();
    }
}

pub async fn started<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    let session = current()
        .filter(|s| s.id == id)
        .ok_or("Recording session disappeared during startup.")?;
    let state = app.state::<crate::state::AppState>();
    sqlx::query(
        "UPDATE recording_sessions SET status='recording', started_at=?, folder_path=? WHERE id=?",
    )
    .bind(session.started_at.map(|t| t.to_rfc3339()))
    .bind(
        session
            .folder
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
    )
    .bind(id)
    .execute(state.db_manager.pool())
    .await
    .map_err(|e| e.to_string())?;

    capture_ready(id);
    emit(app);
    Ok(())
}

pub fn transcription_error(id: &str, error: &str) {
    if let Some(session) = current().filter(|s| s.id == id) {
        *session.processing_error.lock().unwrap() = Some(format!(
            "Some audio could not be transcribed: {error}. Check the meeting folder for recovery."
        ));
    }
}

pub fn persist_transcript_checkpoint(id: &str) {
    let Some(session) = current().filter(|s| s.id == id) else {
        return;
    };
    let Some(folder) = session.folder else { return };
    let segments = session.transcripts.lock().unwrap().clone();
    let checkpoint = json!({
        "version": "1.0",
        "segments": segments,
        "last_updated": Utc::now().to_rfc3339(),
        "total_segments": segments.len()
    });
    let result = (|| -> std::io::Result<()> {
        let temp = folder.join(".transcripts.json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&checkpoint)?)?;
        std::fs::rename(temp, folder.join("transcripts.json"))
    })();
    if let Err(e) = result {
        transcription_error(id, &format!("Transcript checkpoint failed: {e}"));
    }
}

/// Claim stop synchronously so UI and tray cannot finalize twice.
pub fn claim_stop(expected: Option<&str>) -> Option<Session> {
    let mut slot = SESSION.lock().unwrap();
    let s = slot.as_mut()?;
    if expected.is_some_and(|id| id != s.id) || !matches!(s.phase.as_str(), "recording" | "paused") {
        return None;
    }
    s.phase = "stopping".into();
    if let Some(state) = &s.state {
        state.halt_capture();
    }
    Some(s.clone())
}

pub fn phase<R: Runtime>(app: &AppHandle<R>, phase: &str) -> bool {
    if let Some(s) = SESSION.lock().unwrap().as_mut() {
        if matches!(phase, "recording" | "paused")
            && !matches!(s.phase.as_str(), "recording" | "paused")
        {
            return false;
        }
        s.phase = phase.into();
    }
    if let (Some(session), Some(state)) = (current(), app.try_state::<crate::state::AppState>()) {
        let pool = state.db_manager.pool().clone();
        let phase = phase.to_string();
        tauri::async_runtime::spawn(async move {
            let _ = sqlx::query("UPDATE recording_sessions SET status=? WHERE id=? AND status NOT IN ('completed','error','deleted','interrupted')")
                .bind(phase).bind(session.id).execute(&pool).await;
        });
    }
    emit(app);
    true
}

pub async fn save<R: Runtime>(
    app: &AppHandle<R>,
    session: &Session,
    processing_error: Option<&str>,
) -> Result<Value, String> {
    phase(app, "saving");
    let segments: Vec<crate::api::TranscriptSegment> = session
        .transcripts
        .lock()
        .unwrap()
        .iter()
        .map(|s| crate::api::TranscriptSegment {
            id: s.id.clone(),
            text: s.text.clone(),
            timestamp: s.display_time.clone(),
            audio_start_time: Some(s.audio_start_time),
            audio_end_time: Some(s.audio_end_time),
            duration: Some(s.duration),
        })
        .collect();
    let state = app.state::<crate::state::AppState>();
    let folder = session
        .folder
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let meeting =
        crate::database::repositories::transcript::TranscriptsRepository::save_session_transcript(
            state.db_manager.pool(),
            &session.title,
            &segments,
            folder.clone(),
            Some(&session.id),
        )
        .await
        .map_err(|e| e.to_string())?;

    if let Some(folder) = &session.folder {
        let status =
            if processing_error.is_some() || session.processing_error.lock().unwrap().is_some() {
                "error"
            } else {
                "completed"
            };
        if let Err(e) = write_session_metadata(folder, &session.id, &meeting, status) {
            *session.processing_error.lock().unwrap() = Some(format!(
                "Meeting identity metadata could not be saved: {e}. Recovery files were retained."
            ));
        }
    }

    if let Some(folder) = &session.folder {
        let pinned: Option<String> = sqlx::query_scalar(
            "SELECT value FROM session_preferences WHERE key='summaryLanguageDefault'",
        )
        .fetch_optional(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?
        .flatten();
        let language_result = if let Some(language) = pinned {
            crate::summary::metadata::write_summary_language_to_metadata(folder, Some(&language))
        } else {
            let detected = crate::summary::language_detection::detect_summary_language(
                &segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>(),
            );
            crate::summary::metadata::write_detected_summary_language_to_metadata(
                folder,
                detected.language.as_deref(),
            )
        };
        if let Err(e) = language_result {
            log::warn!("Meeting saved but summary language metadata failed: {e}");
            *session.processing_error.lock().unwrap() = Some(format!(
                "Meeting saved, but its summary language preference could not be saved: {e}"
            ));
        }
    }

    let worker_error = session.processing_error.lock().unwrap().clone();
    let processing_error = processing_error.or(worker_error.as_deref());
    let status = if processing_error.is_some() {
        "error"
    } else {
        "completed"
    };
    sqlx::query("UPDATE recording_sessions SET status=?, ended_at=?, error=? WHERE id=?")
        .bind(status)
        .bind(Utc::now().to_rfc3339())
        .bind(processing_error)
        .bind(&session.id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?;

    let result = json!({
        "sessionId": session.id,
        "meetingId": meeting,
        "meetingTitle": session.title,
        "folderPath": folder,
        "phase": status,
        "error": processing_error,
        "transcriptCount": segments.len()
    });
    *LAST.lock().unwrap() = Some(result.clone());
    release(&session.id);
    COMPLETION.notify_waiters();
    emit(app);
    let _ = app.emit("meeting-saved", &result);
    let notify_app = app.clone();
    let notify_result = result.clone();
    tauri::async_runtime::spawn(async move {
        notify_completion(&notify_app, &notify_result).await;
    });
    Ok(result)
}

async fn notify_completion<R: Runtime>(app: &AppHandle<R>, result: &Value) {
    use crate::notifications::{
        commands::NotificationManagerState,
        types::{Notification, NotificationType},
    };
    let Some(state) = app.try_state::<NotificationManagerState<R>>() else {
        return;
    };
    let manager = state.read().await;
    if let Some(manager) = manager.as_ref() {
        let title = result["meetingTitle"].as_str().unwrap_or("Meeting");
        let message = if result["error"].is_string() && !result["meetingId"].is_string() {
            if result["folderPath"].is_string() {
                format!("{title} could not be saved. Check the recording folder for recovery.")
            } else {
                format!(
                    "{title} could not start: {}",
                    result["error"]
                        .as_str()
                        .unwrap_or("Check recording settings.")
                )
            }
        } else if result["error"].is_string() {
            format!("{title} stopped; saved results need attention. Open meeting details.")
        } else {
            format!("{title} stopped and saved.")
        };
        let _ = manager
            .show_notification(Notification::new(
                "Meetily",
                message,
                NotificationType::RecordingStopped,
            ))
            .await;
    }
}

pub async fn completion_result(pool: &sqlx::SqlitePool, id: &str) -> Result<Value, String> {
    use sqlx::Row;
    let row = sqlx::query("SELECT * FROM recording_sessions WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("This recording session was not found.")?;
    let status: String = row.get("status");
    if !matches!(status.as_str(), "completed" | "error" | "interrupted") {
        return Err("Recording finalization is still in progress.".into());
    }
    let meeting: Option<String> = row.get("meeting_id");
    let error: Option<String> = row.get("error");
    let Some(meeting) = meeting else {
        return Err(error.unwrap_or_else(|| {
            "This recording session could not be saved. Recovery files were retained.".into()
        }));
    };
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transcripts WHERE meeting_id=?")
        .bind(&meeting)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "sessionId": id,
        "meetingId": meeting,
        "meetingTitle": row.get::<String, _>("title"),
        "folderPath": row.get::<Option<String>, _>("folder_path"),
        "phase": if error.is_some() { "error" } else { "completed" },
        "error": error,
        "transcriptCount": count
    }))
}

pub async fn wait_for_completion<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
) -> Result<Value, String> {
    loop {
        let notified = COMPLETION.notified();
        if current().is_none_or(|s| s.id != id) {
            return completion_result(app.state::<crate::state::AppState>().db_manager.pool(), id)
                .await;
        }
        notified.await;
    }
}

#[tauri::command]
pub fn get_last_recording_result() -> Option<Value> {
    LAST.lock().unwrap().clone()
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionStatus {
    pub status: String,
    pub meeting_id: Option<String>,
}

pub(crate) async fn persisted_session_status(
    pool: &sqlx::SqlitePool,
    session_id: &str,
) -> Result<Option<RecordingSessionStatus>, String> {
    sqlx::query_as("SELECT status, meeting_id FROM recording_sessions WHERE id=?")
        .bind(session_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_recording_session_status(
    state: tauri::State<'_, crate::state::AppState>,
    session_id: String,
) -> Result<Option<RecordingSessionStatus>, String> {
    let mut status = persisted_session_status(state.db_manager.pool(), &session_id).await?;
    if let Some(session) = current().filter(|session| session.id == session_id) {
        status = Some(RecordingSessionStatus {
            status: session.phase,
            meeting_id: status.and_then(|status| status.meeting_id),
        });
    }
    Ok(status)
}

#[tauri::command]
pub async fn set_native_summary_language_default(
    app: AppHandle,
    language: Option<String>,
) -> Result<(), String> {
    if let Some(code) = &language {
        crate::summary::metadata::normalise_supported_summary_language(code)
            .map_err(|e| e.to_string())?;
    }
    let state = app
        .try_state::<crate::state::AppState>()
        .ok_or("Finish database setup first.")?;
    if let Some(code) = language {
        sqlx::query(
            "INSERT OR REPLACE INTO session_preferences (key, value) VALUES ('summaryLanguageDefault', ?)",
        )
        .bind(code)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query("DELETE FROM session_preferences WHERE key='summaryLanguageDefault'")
            .execute(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn reconcile(pool: &sqlx::SqlitePool) -> Result<(), String> {
    use sqlx::Row;
    let sessions = sqlx::query("SELECT id,title,folder_path FROM recording_sessions WHERE meeting_id IS NULL AND status IN ('starting','recording','paused','stopping','processing','saving','interrupted')")
        .fetch_all(pool).await.map_err(|e| e.to_string())?;
    for row in sessions {
        let id: String = row.get("id");
        let title: String = row.get("title");
        let Some(folder) = row.get::<Option<String>, _>("folder_path") else {
            continue;
        };
        let recovered = async {
            let bytes = tokio::fs::read(PathBuf::from(&folder).join("transcripts.json"))
                .await
                .map_err(|e| e.to_string())?;
            let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let checkpoints: Vec<TranscriptSegment> =
                serde_json::from_value(value["segments"].clone()).map_err(|e| e.to_string())?;
            let segments: Vec<_> = checkpoints
                .iter()
                .map(|s| crate::api::TranscriptSegment {
                    id: s.id.clone(),
                    text: s.text.clone(),
                    timestamp: s.display_time.clone(),
                    audio_start_time: Some(s.audio_start_time),
                    audio_end_time: Some(s.audio_end_time),
                    duration: Some(s.duration),
                })
                .collect();
            let meeting =
                crate::database::repositories::transcript::TranscriptsRepository::save_session_transcript(
                    pool,
                    &title,
                    &segments,
                    Some(folder.clone()),
                    Some(&id),
                )
                .await
                .map_err(|e| e.to_string())?;
            let _ = write_session_metadata(&PathBuf::from(&folder), &id, &meeting, "interrupted");
            Ok::<_, String>(())
        }
        .await;
        if recovered.is_err() {
            log::warn!("Session {id} transcript recovery requires attention; checkpoint files retained");
        }
    }
    sqlx::query("UPDATE recording_sessions SET status='interrupted', error='Meetily exited during this recording. Available transcript checkpoints were recovered where readable. Recover remaining audio from the meeting folder.' WHERE status IN ('starting','recording','paused','stopping','processing','saving')")
        .execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

fn write_session_metadata(
    folder: &std::path::Path,
    session: &str,
    meeting: &str,
    status: &str,
) -> Result<(), String> {
    let path = folder.join("metadata.json");
    let mut metadata: Value =
        serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    metadata["meeting_id"] = json!(meeting);
    metadata["session_id"] = json!(session);
    metadata["status"] = json!(status);
    let temp = folder.join(".metadata.session.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_conflicts_stop_ownership_and_manual_stop_are_atomic() {
        let state = RecordingState::new();
        state.start_recording().unwrap();
        let mut session = Session {
            id: "test-session".into(),
            title: "Test".into(),
            phase: "starting".into(),
            started_at: Some(Utc::now()),
            folder: None,
            state: Some(state.clone()),
            transcripts: Arc::new(Mutex::new(Vec::new())),
            processing_error: Arc::new(Mutex::new(None)),
            pauses: Arc::new(Mutex::new(Vec::new())),
        };
        *SESSION.lock().unwrap() = Some(session.clone());
        assert!(claim_stop(Some("test-session")).is_none());
        capture_ready("different-session");
        assert_eq!(current().unwrap().phase, "starting");
        capture_ready("test-session");
        assert!(claim_stop(Some("test-session")).is_some());
        capture_ready("test-session");
        assert_eq!(current().unwrap().phase, "stopping");
        for phase in ["recording", "paused"] {
            session.phase = phase.into();
            *SESSION.lock().unwrap() = Some(session.clone());
            assert!(claim_stop(Some("different-session")).is_none());
            assert_eq!(current().unwrap().phase, phase);
            assert!(claim_stop(None).is_some());
            assert_eq!(current().unwrap().phase, "stopping");
            assert!(claim_stop(None).is_none());
        }
        release("different-session");
        assert!(current().is_some());
        release("test-session");
        assert!(current().is_none());
    }
}
