//! Tauri commands for observing and steering the automatic pipeline.

use crate::pipeline::orchestrator::{self, PipelineStatus};
use crate::pipeline::scan_stage;
use crate::pipeline::settings::{self, PipelineRunState, PipelineSettings};
use crate::pipeline::{handle, meta};
use crate::state::AppState;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn pipeline_get_status(app: AppHandle) -> Result<PipelineStatus, String> {
    Ok(orchestrator::build_status(&app).await)
}

#[tauri::command]
pub async fn pipeline_get_settings(app: AppHandle) -> Result<PipelineSettings, String> {
    settings::load_settings(&app).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pipeline_set_settings(
    app: AppHandle,
    settings_update: PipelineSettings,
) -> Result<PipelineSettings, String> {
    // Run state is owned by the pipeline itself, not the settings form.
    let mut next = settings_update;
    next.run_state = settings::load_settings(&app)
        .await
        .map(|s| s.run_state)
        .unwrap_or_default();

    settings::save_settings(&app, &next)
        .await
        .map_err(|e| e.to_string())?;
    crate::pipeline::wake();
    orchestrator::emit_status(&app).await;
    Ok(next)
}

#[tauri::command]
pub async fn pipeline_pause(app: AppHandle) -> Result<(), String> {
    let pipeline = handle().ok_or("The pipeline is not running")?;
    pipeline.set_run_state(PipelineRunState::PausedByUser).await;
    settings::save_run_state(&app, &PipelineRunState::PausedByUser).await;
    orchestrator::emit_status(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn pipeline_resume(app: AppHandle) -> Result<(), String> {
    let pipeline = handle().ok_or("The pipeline is not running")?;
    pipeline.set_run_state(PipelineRunState::Running).await;
    settings::save_run_state(&app, &PipelineRunState::Running).await;
    pipeline.wake();
    orchestrator::emit_status(&app).await;
    Ok(())
}

/// Stop everything the user can see in the pending-work panel: pause the
/// pipeline, drop every "Process now" request, and cancel the meeting being
/// transcribed or summarized. The meetings stay pending, with no failure
/// recorded, so Resume or Process now picks them up again.
#[tauri::command]
pub async fn pipeline_stop(app: AppHandle) -> Result<(), String> {
    let pipeline = handle().ok_or("The pipeline is not running")?;

    // Pause before touching the running stage, so the loop cannot start the
    // next meeting in between.
    pipeline.set_run_state(PipelineRunState::PausedByUser).await;
    settings::save_run_state(&app, &PipelineRunState::PausedByUser).await;
    let unqueued = pipeline.clear_all_forced();

    let current = pipeline.current().await;
    if let Some(current) = &current {
        pipeline.request_stop();
        if current.stage == "transcribe" {
            crate::audio::retranscription::cancel_retranscription(Some(&current.meeting_id));
        } else {
            crate::summary::service::SummaryService::cancel_active_summary(&current.meeting_id);
        }
    }
    log::info!(
        "[pipeline] stopped by the user ({} queued meeting(s) released, stage in progress: {})",
        unqueued,
        current.as_ref().map(|c| c.stage.as_str()).unwrap_or("none")
    );

    orchestrator::emit_status(&app).await;
    Ok(())
}

/// Process the given meetings now: clears their retry state and lets them
/// skip the grace window, the idle gate, and a user pause (but never a live
/// recording).
#[tauri::command]
pub async fn pipeline_process_now(
    app: AppHandle,
    meeting_ids: Vec<String>,
) -> Result<(), String> {
    let pipeline = handle().ok_or("The pipeline is not running")?;

    if let Some(state) = app.try_state::<AppState>() {
        let pool = state.db_manager.pool().clone();
        for id in &meeting_ids {
            meta::reset(&pool, id).await;
        }
    }

    pipeline.force(&meeting_ids);
    orchestrator::emit_status(&app).await;
    Ok(())
}

/// Open the SharePoint sign-in window after the pipeline paused for auth,
/// then resume. This is the only place the pipeline shows login UI, and only
/// because the user asked for it.
#[tauri::command]
pub async fn pipeline_sign_in_to_sharepoint(app: AppHandle) -> Result<(), String> {
    let sync_state = crate::audio::sharepoint_sync::load_sync_state_public(&app);
    let hub_url = sync_state
        .hub_url
        .filter(|u| !u.trim().is_empty())
        .ok_or("No SharePoint hub URL is configured yet")?;

    let root_host = url::Url::parse(&hub_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .ok_or("The configured SharePoint hub URL is not valid")?;
    let my_host = crate::audio::sharepoint_sync::derive_my_host_public(&root_host);

    let extra_hosts: Vec<String> = my_host.into_iter().collect();
    crate::audio::sharepoint::ensure_multi_host_auth(
        &app,
        &hub_url,
        &extra_hosts,
        |msg| log::info!("[pipeline] {msg}"),
    )
    .await
    .map_err(|e| e.to_string())?;

    scan_stage::clear_auth_required(&app).await;
    orchestrator::emit_status(&app).await;
    Ok(())
}
