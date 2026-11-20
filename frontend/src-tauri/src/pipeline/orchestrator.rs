//! The pipeline loop: derive outstanding work, pick one meeting, run its
//! next stage, record the outcome, repeat.

use crate::database::models::PendingMeetingModel;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::setting::SettingsRepository;
use crate::pipeline::settings::{self, PipelineRunState, PipelineSettings};
use crate::pipeline::transcribe_stage;
use crate::pipeline::{
    handle, idle, meta, scan_stage, summary_stage, CurrentItem, FailureKind, StageError,
};
use crate::state::AppState;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// How often the loop re-derives work when nothing wakes it sooner.
const TICK: Duration = Duration::from_secs(60);
/// Grace period before the pipeline touches a brand-new meeting, so it never
/// races a just-stopped recording or an in-flight IndexedDB recovery.
const STARTUP_DELAY: Duration = Duration::from_secs(20);

/// What a pending meeting needs next.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Transcribe,
    Summarize,
}

impl Stage {
    fn as_str(&self) -> &'static str {
        match self {
            Stage::Transcribe => "transcribe",
            Stage::Summarize => "summarize",
        }
    }
}

/// A pending meeting plus what the pipeline intends to do with it.
#[derive(Debug, Clone, Serialize)]
pub struct PendingSnapshotItem {
    pub meeting_id: String,
    pub title: String,
    pub stage: Stage,
    pub created_at: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub suppressed: bool,
    /// False while the meeting is inside its grace window or backing off.
    pub eligible: bool,
    /// True when the user clicked "Process now": runs next, even while the
    /// pipeline is paused.
    pub queued: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineStatus {
    pub state: String,
    pub detail: Option<String>,
    pub enabled: bool,
    pub current: Option<CurrentItem>,
    pub pending: Vec<PendingSnapshotItem>,
    pub last_scan_at: Option<String>,
}

fn stage_for(meeting: &PendingMeetingModel) -> Stage {
    // A partial live transcript is never summarized: recover it from audio.
    if meeting.transcript_count == 0 || meeting.transcription_incomplete {
        Stage::Transcribe
    } else {
        Stage::Summarize
    }
}

fn pool_of(app: &AppHandle) -> Option<SqlitePool> {
    app.try_state::<AppState>()
        .map(|state| state.db_manager.pool().clone())
}

/// Build the current view of outstanding work, annotated with retry state.
pub async fn snapshot(
    pool: &SqlitePool,
    config: &PipelineSettings,
) -> Vec<PendingSnapshotItem> {
    let pending = match MeetingsRepository::get_pending_meetings(pool).await {
        Ok(pending) => pending,
        Err(e) => {
            log::warn!("[pipeline] failed to list pending meetings: {}", e);
            return Vec::new();
        }
    };

    let retry_state = meta::load_all(pool).await;
    let now = Utc::now();
    let grace_cutoff = now - ChronoDuration::minutes(config.grace_minutes.max(0));
    let forced = handle();

    pending
        .into_iter()
        .map(|meeting| {
            let row = retry_state.get(&meeting.id);
            let is_forced = forced.map(|h| h.is_forced(&meeting.id)).unwrap_or(false);
            let past_grace = meeting.created_at.0 <= grace_cutoff;
            let retry_ok = row.map(|r| r.eligible_at(now)).unwrap_or(true);

            PendingSnapshotItem {
                meeting_id: meeting.id.clone(),
                title: meeting.title.clone(),
                stage: stage_for(&meeting),
                created_at: meeting.created_at.0.to_rfc3339(),
                attempts: row.map(|r| r.attempts).unwrap_or(0),
                last_error: row.and_then(|r| r.last_error.clone()),
                suppressed: row.map(|r| r.suppressed()).unwrap_or(false),
                eligible: is_eligible(is_forced, past_grace, retry_ok),
                queued: is_forced,
            }
        })
        .collect()
}

pub async fn build_status(app: &AppHandle) -> PipelineStatus {
    let config = settings::load_settings(app).await.unwrap_or_default();
    let (state, current) = match handle() {
        Some(pipeline) => (pipeline.run_state().await, pipeline.current().await),
        None => (config.run_state.clone(), None),
    };

    let pending = match pool_of(app) {
        Some(pool) => snapshot(&pool, &config).await,
        None => Vec::new(),
    };

    let detail = match &state {
        PipelineRunState::AuthRequired { host, .. } => {
            Some(format!("Sign in to SharePoint ({}) to resume importing", host))
        }
        _ => None,
    };

    PipelineStatus {
        state: state.kind().to_string(),
        detail,
        enabled: config.enabled,
        current,
        pending,
        last_scan_at: config.last_scan_at.clone(),
    }
}

pub async fn emit_status(app: &AppHandle) {
    let status = build_status(app).await;
    if let Err(e) = app.emit("pipeline-status", &status) {
        log::debug!("[pipeline] failed to emit status: {}", e);
    }
    crate::tray::update_tray_menu(app);
}

/// Whether a summary provider runs inference on this machine.
///
/// Unknown providers are treated as local: the only consequence is deferring
/// the job until the recording ends, which is the safe way to be wrong.
fn is_local_summary_provider(provider: &str) -> bool {
    !matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "openai" | "claude" | "groq" | "openrouter"
    )
}

/// Whether a local (on-device engine) job may start right now.
async fn local_work_allowed(config: &PipelineSettings, forced: bool) -> Result<(), &'static str> {
    if crate::audio::recording_commands::is_recording().await {
        return Err("a recording is in progress");
    }
    // A forced item skips the idle wait but never the recording check.
    if !forced && !idle::is_idle_for(config.idle_minutes) {
        return Err("the machine is in use");
    }
    Ok(())
}

/// Whether a pending meeting may be attempted at all right now, before the
/// run state is consulted.
///
/// "Process now" skips the grace window, but not the retry backoff: it calls
/// `meta::reset` first, so a freshly forced meeting has no backoff to skip.
/// Keeping the backoff matters because the force flag survives a transient
/// failure — without it a meeting whose summariser is down would retry every
/// tick forever, and queued meetings ignore the pause, so nothing could stop it.
fn is_eligible(is_forced: bool, past_grace: bool, retry_ok: bool) -> bool {
    (is_forced || past_grace) && retry_ok
}

/// Whether the loop may pick this meeting up right now. A user pause stops
/// automatic work, but "Process now" (queued) meetings run even while paused.
fn ready_to_process(item: &PendingSnapshotItem, allows_local_work: bool) -> bool {
    item.eligible && (allows_local_work || item.queued)
}

fn should_scan(config: &PipelineSettings, now: DateTime<Utc>) -> bool {
    match config
        .last_scan_at
        .as_deref()
        .and_then(|when| DateTime::parse_from_rfc3339(when).ok())
    {
        Some(last) => {
            now - last.with_timezone(&Utc)
                >= ChronoDuration::minutes(config.scan_interval_minutes.max(1) as i64)
        }
        None => true,
    }
}

/// Run one meeting's next stage. Returns whether progress was made.
async fn process_one(
    app: &AppHandle,
    pool: &SqlitePool,
    config: &PipelineSettings,
    item: &PendingSnapshotItem,
) -> bool {
    let Some(pipeline) = handle() else { return false };
    let forced = pipeline.is_forced(&item.meeting_id);

    let result: Result<(), StageError> = match item.stage {
        Stage::Transcribe => {
            let transcript_config = transcribe_stage::load_transcript_config(pool).await;
            if !transcript_config.is_remote() {
                if let Err(reason) = local_work_allowed(config, forced).await {
                    log::debug!(
                        "[pipeline] deferring transcription of {}: {}",
                        item.meeting_id,
                        reason
                    );
                    return false;
                }
            }

            let folder = match meeting_folder(pool, &item.meeting_id).await {
                Some(folder) => folder,
                None => {
                    meta::record_failure(
                        pool,
                        &item.meeting_id,
                        "transcribe",
                        "Meeting has no recording folder",
                        FailureKind::Hard,
                        config.max_attempts,
                    )
                    .await;
                    return false;
                }
            };

            pipeline.begin_stage();
            pipeline
                .set_current(Some(CurrentItem {
                    meeting_id: item.meeting_id.clone(),
                    title: item.title.clone(),
                    stage: Stage::Transcribe.as_str().to_string(),
                    started_at: Utc::now().to_rfc3339(),
                }))
                .await;
            emit_status(app).await;

            // Counted before the stage runs: transcription is the stage that can
            // take the process down with it, and an uncounted attempt repeats
            // forever.
            meta::begin_attempt(
                pool,
                &item.meeting_id,
                Stage::Transcribe.as_str(),
                config.max_attempts,
            )
            .await;

            if pipeline.stop_requested() {
                Err(stopped_before_start())
            } else {
                transcribe_stage::run_transcribe_stage(
                    app,
                    &item.meeting_id,
                    &folder,
                    &transcript_config,
                    config.diarization,
                )
                .await
            }
        }
        Stage::Summarize => {
            // A local summariser (Ollama / on-device sidecar) competes with the
            // recording for CPU, so it waits for the same conditions as local
            // transcription. Hosted providers are network-bound and exempt.
            let summary_provider = SettingsRepository::get_model_config(pool)
                .await
                .ok()
                .flatten()
                .map(|c| c.provider)
                .unwrap_or_default();

            if is_local_summary_provider(&summary_provider) {
                if let Err(reason) = local_work_allowed(config, forced).await {
                    log::debug!(
                        "[pipeline] deferring summary of {}: {}",
                        item.meeting_id,
                        reason
                    );
                    return false;
                }
            }

            pipeline.begin_stage();
            pipeline
                .set_current(Some(CurrentItem {
                    meeting_id: item.meeting_id.clone(),
                    title: item.title.clone(),
                    stage: Stage::Summarize.as_str().to_string(),
                    started_at: Utc::now().to_rfc3339(),
                }))
                .await;
            emit_status(app).await;

            meta::begin_attempt(
                pool,
                &item.meeting_id,
                Stage::Summarize.as_str(),
                config.max_attempts,
            )
            .await;

            if pipeline.stop_requested() {
                Err(stopped_before_start())
            } else {
                summary_stage::run_summary_stage(app, pool, &item.meeting_id, config).await
            }
        }
    };

    // Read while the item is still current: a stop that arrives later finds
    // nothing running and leaves no request behind.
    let stopped_by_user = pipeline.take_stop_request();
    pipeline.set_current(None).await;
    if item.stage == Stage::Transcribe {
        crate::api::emit_meetings_changed(app, &item.meeting_id);
    }

    match result {
        Ok(()) => {
            log::info!(
                "[pipeline] {} complete for meeting {}",
                item.stage.as_str(),
                item.meeting_id
            );
            meta::record_success(pool, &item.meeting_id).await;
            // A freshly transcribed meeting is immediately pending a summary,
            // so keep the force flag until that last stage lands.
            if item.stage == Stage::Summarize {
                pipeline.clear_forced(&item.meeting_id);
            }
            true
        }
        // The user stopped this stage: not a failure, so no backoff and no
        // error on the row. The meeting simply stays pending.
        Err(_) if stopped_by_user => {
            log::info!(
                "[pipeline] {} of meeting {} stopped by the user",
                item.stage.as_str(),
                item.meeting_id
            );
            meta::reset(pool, &item.meeting_id).await;
            pipeline.clear_forced(&item.meeting_id);
            false
        }
        Err(e) => {
            log::warn!(
                "[pipeline] {} failed for meeting {}: {}",
                item.stage.as_str(),
                item.meeting_id,
                e.message
            );
            meta::record_failure(
                pool,
                &item.meeting_id,
                item.stage.as_str(),
                &e.message,
                e.kind,
                config.max_attempts,
            )
            .await;
            if e.kind == FailureKind::Hard {
                pipeline.clear_forced(&item.meeting_id);
            }
            false
        }
    }
}

/// The user pressed Stop between this meeting being picked and its stage
/// starting, before there was anything to cancel.
fn stopped_before_start() -> StageError {
    StageError::transient("Stopped before it started")
}

async fn meeting_folder(pool: &SqlitePool, meeting_id: &str) -> Option<String> {
    MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .ok()
        .flatten()
        .and_then(|meeting| meeting.folder_path)
        .filter(|path| !path.trim().is_empty())
}

/// The pipeline task body.
pub async fn run(app: AppHandle) {
    // Let startup settle: database init, job reconciliation, and the
    // frontend's own recovery prompt all happen in the first seconds.
    tokio::time::sleep(STARTUP_DELAY).await;
    log::info!("[pipeline] started");
    emit_status(&app).await;

    loop {
        let Some(pipeline) = handle() else { return };
        let config = settings::load_settings(&app).await.unwrap_or_default();
        let state = pipeline.run_state().await;

        if !config.enabled {
            pipeline.wait_for_work(TICK).await;
            continue;
        }

        // The database is not managed until first-launch setup completes.
        let Some(pool) = pool_of(&app) else {
            pipeline.wait_for_work(TICK).await;
            continue;
        };

        let mut did_work = false;

        // 1. Look for new SharePoint recordings when due.
        if state.allows_scanning() && should_scan(&config, Utc::now()) {
            match scan_stage::run_scan_stage(&app, &pool, &config).await {
                Ok(imported) => {
                    settings::save_last_scan_at(&app, &Utc::now().to_rfc3339()).await;
                    if imported > 0 {
                        log::info!("[pipeline] imported {} new recording(s)", imported);
                        did_work = true;
                    }
                }
                Err(e) if e.auth_required => {
                    scan_stage::enter_auth_required(&app, &e.message).await;
                }
                Err(e) => {
                    log::warn!("[pipeline] SharePoint scan failed: {}", e.message);
                    // Still advance the watermark so a persistently broken
                    // scan does not run every tick.
                    settings::save_last_scan_at(&app, &Utc::now().to_rfc3339()).await;
                }
            }
            emit_status(&app).await;
        }

        // 2. Advance one meeting through its next stage.
        {
            let allows_local = state.allows_local_work();
            let pending = snapshot(&pool, &config).await;
            if let Some(item) = pending
                .into_iter()
                .find(|item| ready_to_process(item, allows_local))
            {
                if process_one(&app, &pool, &config, &item).await {
                    did_work = true;
                }
                emit_status(&app).await;
            }
        }

        // After finishing something there may be a follow-on stage ready
        // (transcribe → summarize), so loop again immediately.
        if !did_work {
            pipeline.wait_for_work(TICK).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::DateTimeUtc;

    fn meeting(transcripts: i64) -> PendingMeetingModel {
        PendingMeetingModel {
            id: "m".to_string(),
            title: "t".to_string(),
            created_at: DateTimeUtc(Utc::now()),
            folder_path: Some("/tmp/m".to_string()),
            transcript_count: transcripts,
            summary_status: None,
            transcription_incomplete: false,
        }
    }

    #[test]
    fn meetings_without_transcripts_transcribe_first() {
        assert_eq!(stage_for(&meeting(0)), Stage::Transcribe);
        assert_eq!(stage_for(&meeting(12)), Stage::Summarize);
    }

    fn snapshot_item(eligible: bool, queued: bool) -> PendingSnapshotItem {
        PendingSnapshotItem {
            meeting_id: "m".to_string(),
            title: "t".to_string(),
            stage: Stage::Summarize,
            created_at: Utc::now().to_rfc3339(),
            attempts: 0,
            last_error: None,
            suppressed: false,
            eligible,
            queued,
        }
    }

    #[test]
    fn forcing_a_meeting_skips_the_grace_window_but_not_its_backoff() {
        // "Process now" resets retry state, so the first attempt runs at once
        // even for a meeting still inside its grace window.
        assert!(is_eligible(true, false, true));
        // A meeting nobody forced still waits out the grace window.
        assert!(!is_eligible(false, false, true));
        // ...and still waits out its retry backoff.
        assert!(!is_eligible(false, true, false));
        // A forced meeting that just failed transiently must wait out the
        // backoff too. Otherwise it is retried every tick forever, and since
        // queued meetings now bypass the pause, the user cannot stop it.
        assert!(!is_eligible(true, true, false));
    }

    #[test]
    fn pausing_stops_a_forced_meeting_that_keeps_failing() {
        // The runaway scenario: user hits "Process now", the summariser is
        // unreachable, every attempt fails transiently so the force flag is
        // never cleared. Once it is backing off, a pause must hold it.
        let backing_off = snapshot_item(is_eligible(true, true, false), true);
        assert!(!ready_to_process(&backing_off, false));
    }

    #[test]
    fn a_pause_stops_automatic_work_but_not_queued_meetings() {
        // Running: any eligible meeting goes.
        assert!(ready_to_process(&snapshot_item(true, false), true));
        // Paused: only meetings the user queued with "Process now".
        assert!(!ready_to_process(&snapshot_item(true, false), false));
        assert!(ready_to_process(&snapshot_item(true, true), false));
        // Never a meeting that is not eligible.
        assert!(!ready_to_process(&snapshot_item(false, true), false));
    }

    #[test]
    fn incomplete_transcripts_are_retranscribed_never_summarized() {
        for count in [0, 7] {
            let mut partial = meeting(count);
            partial.transcription_incomplete = true;
            assert_eq!(stage_for(&partial), Stage::Transcribe);
        }
    }

    #[test]
    fn scan_is_due_when_never_run_and_after_the_interval() {
        let mut config = PipelineSettings::default();
        config.scan_interval_minutes = 30;
        let now = Utc::now();

        assert!(should_scan(&config, now));

        config.last_scan_at = Some((now - ChronoDuration::minutes(5)).to_rfc3339());
        assert!(!should_scan(&config, now));

        config.last_scan_at = Some((now - ChronoDuration::minutes(31)).to_rfc3339());
        assert!(should_scan(&config, now));
    }

    #[test]
    fn unparseable_scan_watermark_triggers_a_scan() {
        let mut config = PipelineSettings::default();
        config.last_scan_at = Some("not a timestamp".to_string());
        assert!(should_scan(&config, Utc::now()));
    }
}
