//! Automatic meeting pipeline.
//!
//! A single long-lived background task that turns new recordings into
//! finished, exported meeting notes without any UI interaction:
//!
//! ```text
//! SharePoint scan → import → transcribe → summarize → Obsidian export
//! ```
//!
//! Design notes:
//!
//! - **The work list is derived, never stored.** Every tick re-runs
//!   `MeetingsRepository::get_pending_meetings`, the same query the
//!   pending-work panel uses. A crash mid-stage therefore needs no
//!   reconciliation: whatever did not get committed simply still looks
//!   pending. The only persisted pipeline state is retry bookkeeping
//!   (`pipeline_meta`) and the run state / scan watermark in the settings
//!   store.
//! - **One meeting at a time.** Sequential processing means the pipeline
//!   needs at most one remote-summary slot and one engine claim, so it never
//!   has to queue against itself, and a conflict with user-initiated work
//!   just means "try again next tick".
//! - **The existing fail-fast guards are the lock.** Import, retranscription
//!   and recording already refuse to run concurrently; the pipeline treats
//!   those refusals as transient and backs off.

pub mod commands;
pub mod idle;
pub mod meta;
pub mod orchestrator;
pub mod scan_stage;
pub mod settings;
pub mod summary_stage;
pub mod transcribe_stage;

use serde::Serialize;
use settings::PipelineRunState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::{Notify, RwLock};

/// How the retry policy treats a failed stage attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Counts against the attempt budget; gives up after `max_attempts`.
    Hard,
    /// The endpoint is down, busy, or yielded to a recording: back off and
    /// never give up.
    Transient,
    /// The stage ran out of time. Backs off like `Transient`, but gives up
    /// after a run of consecutive timeouts so the same meeting cannot burn
    /// GPU time forever.
    Timeout,
}

/// Phrases that mean the endpoint is unreachable, busy, or rate limiting. They
/// say nothing about the meeting. Checked before the timeout phrases so that
/// "504 Gateway Timeout" stays an outage.
const OUTAGE_NEEDLES: &[&str] = &[
    "connection",
    "connect",
    "network",
    "unreachable",
    "refused",
    "dns",
    "temporarily",
    "503",
    "502",
    "504",
    "429",
    "rate limit",
    "overloaded",
];

const TIMEOUT_NEEDLES: &[&str] = &["timed out", "timeout"];

/// Classify a stage failure from its message. `stage_transient` holds phrases
/// a stage treats as retry-later on top of the shared outage list (e.g.
/// cancellation, engine contention). They are checked first.
pub(crate) fn classify_message(error: &str, stage_transient: &[&str]) -> FailureKind {
    let error = error.to_lowercase();
    let mentions = |needles: &[&str]| needles.iter().any(|needle| error.contains(needle));
    if mentions(stage_transient) || mentions(OUTAGE_NEEDLES) {
        FailureKind::Transient
    } else if mentions(TIMEOUT_NEEDLES) {
        FailureKind::Timeout
    } else {
        FailureKind::Hard
    }
}

/// A stage failure, classified for the retry policy.
#[derive(Debug, Clone)]
pub struct StageError {
    pub message: String,
    pub kind: FailureKind,
}

impl StageError {
    pub fn hard(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: FailureKind::Hard,
        }
    }

    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: FailureKind::Transient,
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: FailureKind::Timeout,
        }
    }
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What the pipeline is doing right now, for the UI and tray.
#[derive(Debug, Clone, Serialize)]
pub struct CurrentItem {
    pub meeting_id: String,
    pub title: String,
    /// "scan" | "import" | "transcribe" | "summarize"
    pub stage: String,
    pub started_at: String,
}

/// Shared handles the orchestrator task and the Tauri commands both touch.
pub struct PipelineHandle {
    notify: Notify,
    state: RwLock<PipelineRunState>,
    current: RwLock<Option<CurrentItem>>,
    /// Meetings the user explicitly asked to process now: these skip the
    /// grace window and the idle gate.
    forced: Mutex<Vec<String>>,
    /// Set by a user stop while a stage is running, so the orchestrator can
    /// tell that stage's cancellation apart from a failure.
    stop_requested: AtomicBool,
}

impl PipelineHandle {
    fn new(state: PipelineRunState) -> Self {
        Self {
            notify: Notify::new(),
            state: RwLock::new(state),
            current: RwLock::new(None),
            forced: Mutex::new(Vec::new()),
            stop_requested: AtomicBool::new(false),
        }
    }

    pub async fn run_state(&self) -> PipelineRunState {
        self.state.read().await.clone()
    }

    pub async fn set_run_state(&self, state: PipelineRunState) {
        *self.state.write().await = state;
    }

    pub async fn current(&self) -> Option<CurrentItem> {
        self.current.read().await.clone()
    }

    /// Non-blocking reads for synchronous callers (the tray menu builder,
    /// which runs on the UI thread and must never block or `block_on`).
    /// Returns `None` when the lock is momentarily held, in which case the
    /// caller simply omits the pipeline section until the next refresh.
    pub fn run_state_hint(&self) -> Option<PipelineRunState> {
        self.state.try_read().ok().map(|state| state.clone())
    }

    pub fn current_hint(&self) -> Option<CurrentItem> {
        self.current.try_read().ok().and_then(|item| item.clone())
    }

    pub(crate) async fn set_current(&self, item: Option<CurrentItem>) {
        *self.current.write().await = item;
    }

    /// Queue meetings for immediate processing and wake the loop.
    pub fn force(&self, meeting_ids: &[String]) {
        if let Ok(mut forced) = self.forced.lock() {
            for id in meeting_ids {
                if !forced.contains(id) {
                    forced.push(id.clone());
                }
            }
        }
        self.notify.notify_one();
    }

    pub(crate) fn is_forced(&self, meeting_id: &str) -> bool {
        self.forced
            .lock()
            .map(|forced| forced.iter().any(|id| id == meeting_id))
            .unwrap_or(false)
    }

    pub(crate) fn clear_forced(&self, meeting_id: &str) {
        if let Ok(mut forced) = self.forced.lock() {
            forced.retain(|id| id != meeting_id);
        }
    }

    /// Drop every "Process now" request. Returns how many were queued.
    pub(crate) fn clear_all_forced(&self) -> usize {
        self.forced
            .lock()
            .map(|mut forced| {
                let count = forced.len();
                forced.clear();
                count
            })
            .unwrap_or(0)
    }

    /// The user asked to stop the stage that is running now.
    pub(crate) fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::SeqCst);
    }

    /// A new stage is starting: a stop aimed at an earlier one no longer applies.
    pub(crate) fn begin_stage(&self) {
        self.stop_requested.store(false, Ordering::SeqCst);
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::SeqCst)
    }

    /// Whether the stage that just ended was stopped by the user. Clears the
    /// request, so it is answered once.
    pub(crate) fn take_stop_request(&self) -> bool {
        self.stop_requested.swap(false, Ordering::SeqCst)
    }

    pub fn wake(&self) {
        self.notify.notify_one();
    }

    /// Sleep until woken by new work or the tick deadline passes.
    pub(crate) async fn wait_for_work(&self, tick: Duration) {
        let _ = tokio::time::timeout(tick, self.notify.notified()).await;
    }
}

static PIPELINE: OnceLock<PipelineHandle> = OnceLock::new();

pub fn handle() -> Option<&'static PipelineHandle> {
    PIPELINE.get()
}

/// Nudge the pipeline to look for work now (e.g. an import just finished).
/// Safe to call before the pipeline starts.
pub fn wake() {
    if let Some(pipeline) = PIPELINE.get() {
        pipeline.wake();
    }
}

/// Tracks the retranscription the pipeline started, so a live recording can
/// reclaim the shared transcription engine from it.
pub(crate) struct PipelineRetranscription {
    meeting_id: Mutex<Option<String>>,
}

impl PipelineRetranscription {
    const fn new() -> Self {
        Self {
            meeting_id: Mutex::new(None),
        }
    }

    pub(crate) fn claim(&self, meeting_id: &str) {
        if let Ok(mut slot) = self.meeting_id.lock() {
            *slot = Some(meeting_id.to_string());
        }
    }

    pub(crate) fn release(&self, meeting_id: &str) {
        if let Ok(mut slot) = self.meeting_id.lock() {
            if slot.as_deref() == Some(meeting_id) {
                *slot = None;
            }
        }
    }

    fn active(&self) -> Option<String> {
        self.meeting_id.lock().ok().and_then(|slot| slot.clone())
    }
}

pub(crate) static PIPELINE_RETRANSCRIPTION: PipelineRetranscription = PipelineRetranscription::new();

/// Give the shared transcription engine back for a live recording.
///
/// Only cancels a retranscription the *pipeline* started — a user-initiated
/// one still fails the recording start with the existing error, because the
/// user chose to run it. Returns once the engine is free or `timeout`
/// elapses.
pub async fn yield_engine_for_recording(timeout: Duration) -> bool {
    let Some(meeting_id) = PIPELINE_RETRANSCRIPTION.active() else {
        return true;
    };

    log::info!(
        "[pipeline] cancelling background transcription of {} so recording can use the engine",
        meeting_id
    );
    crate::audio::retranscription::cancel_retranscription(Some(&meeting_id));

    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if !crate::audio::retranscription::is_retranscription_active_for_meeting(&meeting_id) {
            log::info!("[pipeline] engine released for recording");
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    log::warn!(
        "[pipeline] background transcription of {} did not stop within {:?}",
        meeting_id,
        timeout
    );
    false
}

/// Start the pipeline task. Called once during app setup.
pub fn spawn_pipeline(app: tauri::AppHandle) {
    if PIPELINE.get().is_some() {
        log::warn!("[pipeline] already running; ignoring duplicate spawn");
        return;
    }

    let app_for_state = app.clone();
    tauri::async_runtime::spawn(async move {
        // Restore the persisted run state so a pause or a pending sign-in
        // survives a restart.
        let restored = settings::load_settings(&app_for_state)
            .await
            .map(|s| s.run_state)
            .unwrap_or_default();

        if PIPELINE.set(PipelineHandle::new(restored)).is_err() {
            return;
        }

        orchestrator::run(app_for_state).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_errors_carry_their_retry_class() {
        assert_eq!(StageError::transient("endpoint down").kind, FailureKind::Transient);
        assert_eq!(StageError::hard("bad config").kind, FailureKind::Hard);
        assert_eq!(StageError::timeout("too slow").kind, FailureKind::Timeout);
        assert_eq!(StageError::hard("boom").to_string(), "boom");
    }

    #[test]
    fn pipeline_retranscription_only_releases_its_own_claim() {
        let tracker = PipelineRetranscription::new();
        tracker.claim("meeting-a");
        tracker.release("meeting-b");
        assert_eq!(tracker.active().as_deref(), Some("meeting-a"));
        tracker.release("meeting-a");
        assert!(tracker.active().is_none());
    }

    #[tokio::test]
    async fn forced_meetings_are_tracked_until_cleared() {
        let handle = PipelineHandle::new(PipelineRunState::Running);
        handle.force(&["m1".to_string(), "m2".to_string()]);
        assert!(handle.is_forced("m1"));
        assert!(handle.is_forced("m2"));
        assert!(!handle.is_forced("m3"));

        handle.clear_forced("m1");
        assert!(!handle.is_forced("m1"));
        assert!(handle.is_forced("m2"));
    }

    #[test]
    fn stopping_empties_the_queue() {
        let handle = PipelineHandle::new(PipelineRunState::Running);
        handle.force(&["m1".to_string(), "m2".to_string()]);
        assert_eq!(handle.clear_all_forced(), 2);
        assert!(!handle.is_forced("m1"));
        assert!(!handle.is_forced("m2"));
        assert_eq!(handle.clear_all_forced(), 0);
    }

    #[test]
    fn a_stop_request_applies_to_one_stage_only() {
        let handle = PipelineHandle::new(PipelineRunState::Running);
        assert!(!handle.take_stop_request());

        handle.request_stop();
        assert!(handle.stop_requested());
        assert!(handle.take_stop_request());
        // Answered once: the next stage's failure is a real failure.
        assert!(!handle.take_stop_request());

        // A stop left over from an earlier stage does not reach the next one.
        handle.request_stop();
        handle.begin_stage();
        assert!(!handle.take_stop_request());
    }

    #[tokio::test]
    async fn yielding_is_a_no_op_when_the_pipeline_owns_nothing() {
        assert!(yield_engine_for_recording(Duration::from_millis(10)).await);
    }
}
