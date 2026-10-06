//! Tauri commands for speaker identification.
use super::models::{self, DownloadProgress, ModelsStatus};
use tauri::{AppHandle, Emitter, Runtime};

pub const MODEL_DOWNLOAD_PROGRESS_EVENT: &str = "diarization-model-download-progress";

pub(crate) fn emit_download_progress<R: Runtime>(app: &AppHandle<R>, progress: DownloadProgress) {
    let _ = app.emit(MODEL_DOWNLOAD_PROGRESS_EVENT, progress);
}

#[tauri::command]
pub async fn diarization_models_status() -> Result<ModelsStatus, String> {
    let dir = models::models_directory().map_err(|e| e.to_string())?;
    Ok(models::status(&dir))
}

#[tauri::command]
pub async fn diarization_download_models<R: Runtime>(app: AppHandle<R>) -> Result<ModelsStatus, String> {
    let dir = models::models_directory().map_err(|e| e.to_string())?;
    models::ensure_models(&dir, |p| emit_download_progress(&app, p), || false)
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(models::status(&dir))
}

#[tauri::command]
pub async fn diarization_delete_models() -> Result<ModelsStatus, String> {
    let dir = models::models_directory().map_err(|e| e.to_string())?;
    models::delete_models(&dir).map_err(|e| e.to_string())?;
    Ok(models::status(&dir))
}

use super::jobs::{self, IdentifyRequest, JobStatus};
use super::people::{self, PropagatedLink};
use crate::audio::common::speaker_count_from_command;
use crate::database::repositories::person::{PeopleRepository, PersonSummary};
use crate::database::repositories::speaker::{MeetingSpeaker, ReassignTarget, SpeakersRepository};
use crate::state::AppState;
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::PathBuf;

#[tauri::command]
pub async fn start_speaker_identification<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    num_speakers: Option<u32>,
) -> Result<(), String> {
    // enqueue refuses a meeting that already has a job or is being retranscribed. Batch engine
    // use is serialised by the engine lock, so other meetings' jobs do not block this one.
    jobs::enqueue(
        &app,
        IdentifyRequest {
            meeting_id,
            folder_path: PathBuf::from(meeting_folder_path),
            num_speakers: speaker_count_from_command(num_speakers),
            automatic: false,
        },
    )
}

#[tauri::command]
pub async fn cancel_speaker_identification<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<(), String> {
    jobs::cancel(&app, &meeting_id)
}

#[tauri::command]
pub async fn get_speaker_identification_status(meeting_id: String) -> Result<Option<JobStatus>, String> {
    Ok(jobs::status(&meeting_id))
}

#[tauri::command]
pub async fn api_list_meeting_speakers(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<MeetingSpeaker>, String> {
    SpeakersRepository::list(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to load speakers: {}", e))
}

/// What naming or confirming a speaker did: the person it is linked to, and the speakers of other
/// meetings that were named after them (what Undo reverts).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct NameOutcome {
    pub person_id: Option<String>,
    pub propagated: Vec<PropagatedLink>,
}

enum NameAction {
    Name(String),
    Confirm,
}

/// Message for the user: refusals and not-found errors as written (their `Display` adds a
/// protocol prefix), anything else with what failed.
fn user_error(action: &str, e: sqlx::Error) -> String {
    match e {
        sqlx::Error::Protocol(message) => message,
        e => format!("Failed to {action}: {e}"),
    }
}

/// Applies the action, then names the person's voice in other meetings when voices are
/// remembered and a person is linked; rewrites transcripts.json for the meeting and every
/// changed meeting.
async fn name_and_propagate(
    pool: &SqlitePool,
    meeting_id: &str,
    speaker_key: &str,
    action: NameAction,
    remember_voices: bool,
) -> Result<NameOutcome, String> {
    let person_id = match action {
        NameAction::Name(name) => SpeakersRepository::name(pool, meeting_id, speaker_key, &name, remember_voices)
            .await
            .map_err(|e| user_error("name the speaker", e))?,
        NameAction::Confirm => SpeakersRepository::confirm(pool, meeting_id, speaker_key, remember_voices)
            .await
            .map_err(|e| user_error("confirm the name", e))?,
    };
    let propagated = match person_id.as_deref() {
        Some(person_id) if remember_voices => people::propagate_person(pool, person_id, &jobs::is_busy, people::VOICE_STRONG)
            .await
            .unwrap_or_else(|e| {
                // The name itself is saved; other meetings simply keep their names.
                log::warn!("Failed to name person {} in other meetings: {}", person_id, e);
                Vec::new()
            }),
        _ => Vec::new(),
    };
    jobs::rewrite_transcripts_json(pool, meeting_id, None).await;
    // Propagation never touches this meeting (the person is linked here) and makes at most one
    // link per meeting, so each link is a distinct other meeting.
    for link in &propagated {
        jobs::rewrite_transcripts_json(pool, &link.meeting_id, None).await;
    }
    Ok(NameOutcome { person_id, propagated })
}

#[tauri::command]
pub async fn api_name_meeting_speaker<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    speaker_key: String,
    name: String,
) -> Result<NameOutcome, String> {
    jobs::ensure_idle(&meeting_id)?;
    let remember_voices = crate::audio::recording_preferences::remember_voices(&app).await;
    name_and_propagate(state.db_manager.pool(), &meeting_id, &speaker_key, NameAction::Name(name), remember_voices).await
}

#[tauri::command]
pub async fn api_confirm_meeting_speaker_name<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    speaker_key: String,
) -> Result<NameOutcome, String> {
    jobs::ensure_idle(&meeting_id)?;
    let remember_voices = crate::audio::recording_preferences::remember_voices(&app).await;
    name_and_propagate(state.db_manager.pool(), &meeting_id, &speaker_key, NameAction::Confirm, remember_voices).await
}

#[tauri::command]
pub async fn api_reject_meeting_speaker_name(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    speaker_key: String,
) -> Result<(), String> {
    jobs::ensure_idle(&meeting_id)?;
    let pool = state.db_manager.pool();
    SpeakersRepository::reject(pool, &meeting_id, &speaker_key)
        .await
        .map_err(|e| user_error("reject the name", e))?;
    jobs::rewrite_transcripts_json(pool, &meeting_id, None).await;
    Ok(())
}

/// Undo the names a naming spread to other meetings. Returns the meetings that changed.
#[tauri::command]
pub async fn api_undo_name_propagation(
    state: tauri::State<'_, AppState>,
    links: Vec<PropagatedLink>,
) -> Result<Vec<String>, String> {
    let pool = state.db_manager.pool();
    let changed = people::undo_propagation(pool, &links)
        .await
        .map_err(|e| user_error("undo the names", e))?;
    for meeting_id in &changed {
        jobs::rewrite_transcripts_json(pool, meeting_id, None).await;
    }
    Ok(changed)
}

#[tauri::command]
pub async fn api_list_people(state: tauri::State<'_, AppState>) -> Result<Vec<PersonSummary>, String> {
    PeopleRepository::list(state.db_manager.pool())
        .await
        .map_err(|e| user_error("load people", e))
}

#[tauri::command]
pub async fn api_rename_person(
    state: tauri::State<'_, AppState>,
    person_id: String,
    name: String,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let changed = PeopleRepository::rename(pool, &person_id, &name)
        .await
        .map_err(|e| user_error("rename the person", e))?;
    for meeting_id in &changed {
        jobs::rewrite_transcripts_json(pool, meeting_id, None).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn api_merge_people(
    state: tauri::State<'_, AppState>,
    from_id: String,
    into_id: String,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let changed = PeopleRepository::merge(pool, &from_id, &into_id)
        .await
        .map_err(|e| user_error("merge people", e))?;
    for meeting_id in &changed {
        jobs::rewrite_transcripts_json(pool, meeting_id, None).await;
    }
    Ok(())
}

/// Forgetting keeps every display name, so transcripts.json needs no rewrite.
#[tauri::command]
pub async fn api_forget_person(state: tauri::State<'_, AppState>, person_id: String) -> Result<(), String> {
    PeopleRepository::forget(state.db_manager.pool(), &person_id)
        .await
        .map_err(|e| user_error("forget the person", e))
}

#[tauri::command]
pub async fn api_forget_all_voices(state: tauri::State<'_, AppState>) -> Result<(), String> {
    PeopleRepository::forget_all(state.db_manager.pool())
        .await
        .map_err(|e| user_error("forget voices", e))
}

#[tauri::command]
pub async fn api_merge_meeting_speakers(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    from_key: String,
    into_key: String,
) -> Result<(), String> {
    jobs::ensure_idle(&meeting_id)?;
    SpeakersRepository::merge(state.db_manager.pool(), &meeting_id, &from_key, &into_key)
        .await
        .map_err(|e| format!("Failed to merge speakers: {}", e))?;
    jobs::rewrite_transcripts_json(state.db_manager.pool(), &meeting_id, None).await;
    Ok(())
}

/// `speaker_key` None assigns the row to a new speaker.
#[tauri::command]
pub async fn api_set_transcript_speaker(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    transcript_id: String,
    speaker_key: Option<String>,
) -> Result<String, String> {
    jobs::ensure_idle(&meeting_id)?;
    let target = match speaker_key {
        Some(k) => ReassignTarget::Existing(k),
        None => ReassignTarget::New,
    };
    let key = SpeakersRepository::reassign_row(state.db_manager.pool(), &meeting_id, &transcript_id, target)
        .await
        .map_err(|e| format!("Failed to change speaker: {}", e))?;
    jobs::rewrite_transcripts_json(state.db_manager.pool(), &meeting_id, None).await;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::speaker::{NameSource, NewSpeaker, SpeakerLink, SpeakerWrite};
    use crate::database::test_support::{migrated_pool, seed_meeting, seed_person};
    use sqlx::SqlitePool;

    async fn meeting(pool: &SqlitePool, id: &str, speakers: Vec<NewSpeaker>) {
        seed_meeting(pool, id, &[]).await;
        let mut conn = pool.acquire().await.unwrap();
        SpeakersRepository::replace_for_meeting(&mut conn, id, &SpeakerWrite { speakers, ..Default::default() })
            .await
            .unwrap();
    }

    fn voice(key: &str, embedding: &[f32]) -> NewSpeaker {
        NewSpeaker { key: key.into(), embedding: embedding.to_vec(), speech_seconds: 1.0, ..Default::default() }
    }

    fn link(meeting_id: &str, key: &str, person_id: &str) -> PropagatedLink {
        PropagatedLink { meeting_id: meeting_id.into(), speaker_key: key.into(), person_id: person_id.into() }
    }

    #[test]
    fn refusals_reach_the_user_as_written() {
        let refusal = sqlx::Error::Protocol("A person named Noah already exists".into());
        assert_eq!(user_error("rename the person", refusal), "A person named Noah already exists");
        let other = sqlx::Error::RowNotFound;
        let expected = format!("Failed to rename the person: {}", sqlx::Error::RowNotFound);
        assert_eq!(user_error("rename the person", other), expected);
    }

    #[tokio::test]
    async fn name_and_propagate_returns_links_from_other_meetings() {
        let pool = migrated_pool().await;
        meeting(&pool, "cmd-a", vec![voice("spk_0", &[1.0, 0.0])]).await;
        meeting(&pool, "cmd-b", vec![voice("spk_0", &[0.99, 0.1]), voice("spk_1", &[0.0, 1.0])]).await;

        let outcome = name_and_propagate(&pool, "cmd-a", "spk_0", NameAction::Name("Noah".into()), true).await.unwrap();

        let person_id = outcome.person_id.clone().expect("linked to a person");
        assert_eq!(outcome.propagated, vec![link("cmd-b", "spk_0", &person_id)]);
        let b = SpeakersRepository::list(&pool, "cmd-b").await.unwrap();
        assert_eq!(b[0].display_name.as_deref(), Some("Noah"));
        assert_eq!(b[0].link.name_source, Some(NameSource::Voice));
        assert_eq!(b[1].display_name, None);
    }

    #[tokio::test]
    async fn name_and_propagate_with_remember_off_links_nothing() {
        let pool = migrated_pool().await;
        meeting(&pool, "cmd-a", vec![voice("spk_0", &[1.0, 0.0])]).await;
        meeting(&pool, "cmd-b", vec![voice("spk_0", &[0.99, 0.1])]).await;

        let outcome = name_and_propagate(&pool, "cmd-a", "spk_0", NameAction::Name("Noah".into()), false).await.unwrap();

        assert_eq!(outcome, NameOutcome::default());
        let a = SpeakersRepository::list(&pool, "cmd-a").await.unwrap();
        assert_eq!(a[0].display_name.as_deref(), Some("Noah"));
        assert_eq!(a[0].link.person_id, None);
        assert_eq!(SpeakersRepository::list(&pool, "cmd-b").await.unwrap()[0].display_name, None);
        assert!(PeopleRepository::list(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn confirm_propagates_like_naming() {
        let pool = migrated_pool().await;
        seed_person(&pool, "person-noah", "Noah").await;
        let auto = NewSpeaker {
            display_name: Some("Noah".into()),
            link: SpeakerLink { person_id: Some("person-noah".into()), name_source: Some(NameSource::Voice), ..Default::default() },
            ..voice("spk_0", &[1.0, 0.0])
        };
        meeting(&pool, "cmd-c", vec![auto]).await;
        meeting(&pool, "cmd-d", vec![voice("spk_0", &[0.99, 0.1])]).await;

        let outcome = name_and_propagate(&pool, "cmd-c", "spk_0", NameAction::Confirm, true).await.unwrap();

        assert_eq!(outcome.person_id.as_deref(), Some("person-noah"));
        assert_eq!(outcome.propagated, vec![link("cmd-d", "spk_0", "person-noah")]);
    }
}
