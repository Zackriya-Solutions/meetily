//! Speaker identification jobs: one at a time, queued, cancellable.
use super::assign::{carry_over_names, label_rows, RowLabel, RowSpan, CARRY_OVER_MIN_SIMILARITY};
use super::diarizer::{Diarization, DiarizeOptions, Diarizer};
use super::models::{self, DownloadProgress};
use super::timing::{read_metadata, recording_time_map, TimeMap};
use super::{Cancelled, Turn};
use crate::api::TranscriptSegment;
use crate::audio::common::{
    acquire_batch_engine_lock, batch_engine_busy, unload_engine_after_batch, write_transcripts_json, BatchEngine,
};
use crate::audio::decoder::decode_audio_file;
use crate::database::repositories::speaker::{NewSpeaker, SpeakerWrite, SpeakersRepository, SplitRow};
use crate::state::AppState;
use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use serde::Serialize;
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const PROGRESS_EVENT: &str = "diarization-progress";
pub const COMPLETE_EVENT: &str = "diarization-complete";
pub const ERROR_EVENT: &str = "diarization-error";
/// Automatic jobs are queued after the recording is saved, when its audio is normally final;
/// this only covers a file that is still being written.
const AUTO_AUDIO_WAIT: Duration = Duration::from_secs(30);
/// How often cancellable waits re-check the cancel flag.
const LOCK_POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
pub struct IdentifyRequest {
    pub meeting_id: String,
    pub folder_path: PathBuf,
    pub num_speakers: Option<usize>,
    pub automatic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub meeting_id: String,
    pub state: JobState,
    /// What the job is doing: None while queued, then "audio", "waiting", "download",
    /// "segmentation", "embeddings", "clustering", "splitting", "saving" or "done".
    pub stage: Option<&'static str>,
    pub percent: u32,
    pub message: String,
}

/// Result of asking to cancel a meeting's job.
#[derive(Debug)]
pub enum CancelOutcome {
    /// The job had not started; it was removed from the queue.
    Queued(IdentifyRequest),
    /// The job is running; it stops at its next cancel check.
    Running,
    NotFound,
}

#[derive(Default)]
pub struct JobQueue {
    queue: VecDeque<IdentifyRequest>,
    statuses: HashMap<String, JobStatus>,
    worker_running: bool,
}

impl JobQueue {
    pub fn push(&mut self, req: IdentifyRequest) -> Result<(), String> {
        if self.statuses.contains_key(&req.meeting_id) {
            return Err("Speaker identification is already queued or running for this meeting".into());
        }
        self.statuses.insert(
            req.meeting_id.clone(),
            JobStatus {
                meeting_id: req.meeting_id.clone(),
                state: JobState::Queued,
                stage: None,
                percent: 0,
                message: "Waiting to start…".into(),
            },
        );
        self.queue.push_back(req);
        Ok(())
    }

    pub fn start_next(&mut self) -> Option<IdentifyRequest> {
        let req = self.queue.pop_front()?;
        if let Some(s) = self.statuses.get_mut(&req.meeting_id) {
            s.state = JobState::Running;
        }
        Some(req)
    }

    /// Remove a job that has not started yet.
    pub fn cancel_queued(&mut self, meeting_id: &str) -> Option<IdentifyRequest> {
        let index = self.queue.iter().position(|r| r.meeting_id == meeting_id)?;
        let req = self.queue.remove(index)?;
        self.statuses.remove(meeting_id);
        Some(req)
    }

    pub fn is_running(&self, meeting_id: &str) -> bool {
        matches!(self.statuses.get(meeting_id), Some(JobStatus { state: JobState::Running, .. }))
    }

    pub fn cancel(&mut self, meeting_id: &str) -> CancelOutcome {
        if let Some(req) = self.cancel_queued(meeting_id) {
            return CancelOutcome::Queued(req);
        }
        if self.is_running(meeting_id) {
            CancelOutcome::Running
        } else {
            CancelOutcome::NotFound
        }
    }

    /// Record progress. Returns None when nothing changed, so no event is sent.
    pub fn update(&mut self, meeting_id: &str, stage: &'static str, percent: u32, message: &str) -> Option<JobStatus> {
        let s = self.statuses.get_mut(meeting_id)?;
        if s.stage == Some(stage) && s.percent == percent && s.message == message {
            return None;
        }
        s.stage = Some(stage);
        s.percent = percent;
        s.message = message.to_string();
        Some(s.clone())
    }

    pub fn finish(&mut self, meeting_id: &str) {
        self.statuses.remove(meeting_id);
    }

    pub fn status(&self, meeting_id: &str) -> Option<JobStatus> {
        self.statuses.get(meeting_id).cloned()
    }

    /// Why the meeting's speakers and rows must not be changed by anything else right now.
    pub fn busy_message(&self, meeting_id: &str) -> Option<&'static str> {
        self.statuses
            .contains_key(meeting_id)
            .then_some("Speaker identification is running for this meeting; try again when it finishes")
    }
}

static JOBS: Lazy<Mutex<JobQueue>> = Lazy::new(|| Mutex::new(JobQueue::default()));
/// Cancel flag of the running job. Set and cleared only while holding the JOBS lock.
static CANCEL_CURRENT: AtomicBool = AtomicBool::new(false);
/// Meetings being retranscribed. Lock order everywhere: JOBS first, then this.
static BUSY_RETRANSCRIBING: Lazy<Mutex<HashSet<String>>> = Lazy::new(Default::default);
/// Only one diarization runs at a time, whether it comes from the Identify queue,
/// retranscription or import.
static DIARIZE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Serialises transcripts.json rewrites (jobs and speaker edits).
static JSON_REWRITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn jobs() -> std::sync::MutexGuard<'static, JobQueue> {
    JOBS.lock().unwrap_or_else(|e| e.into_inner())
}

fn busy_retranscribing() -> std::sync::MutexGuard<'static, HashSet<String>> {
    BUSY_RETRANSCRIBING.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_cancelled() -> bool {
    CANCEL_CURRENT.load(Ordering::SeqCst)
}

pub fn status(meeting_id: &str) -> Option<JobStatus> {
    jobs().status(meeting_id)
}

pub fn is_active(meeting_id: &str) -> bool {
    status(meeting_id).is_some()
}

/// Marks a meeting as being retranscribed until dropped.
pub struct RetranscribeClaim(String);

impl Drop for RetranscribeClaim {
    fn drop(&mut self) {
        busy_retranscribing().remove(&self.0);
    }
}

/// Atomically refuse when identification is queued or running for the meeting, or when the
/// meeting is already being retranscribed; otherwise mark it as being retranscribed so no
/// identification job can start on it.
pub fn claim_for_retranscription(meeting_id: &str) -> Result<RetranscribeClaim, String> {
    let q = jobs();
    if let Some(message) = q.busy_message(meeting_id) {
        return Err(message.to_string());
    }
    if !busy_retranscribing().insert(meeting_id.to_string()) {
        return Err("This meeting is already being retranscribed".into());
    }
    drop(q);
    Ok(RetranscribeClaim(meeting_id.to_string()))
}

pub fn is_retranscribing(meeting_id: &str) -> bool {
    busy_retranscribing().contains(meeting_id)
}

pub fn enqueue<R: Runtime>(app: &AppHandle<R>, req: IdentifyRequest) -> Result<(), String> {
    let meeting_id = req.meeting_id.clone();
    let (spawn_worker, queued) = {
        let mut q = jobs();
        if is_retranscribing(&meeting_id) {
            return Err("This meeting is being retranscribed; try again when it finishes".into());
        }
        q.push(req)?;
        let spawn = !q.worker_running;
        q.worker_running = true;
        (spawn, q.status(&meeting_id))
    };
    if let Some(s) = queued {
        let _ = app.emit(PROGRESS_EVENT, s);
    }
    if spawn_worker {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { worker(app).await });
    }
    Ok(())
}

pub fn cancel<R: Runtime>(app: &AppHandle<R>, meeting_id: &str) -> Result<(), String> {
    let outcome = {
        let mut q = jobs();
        let outcome = q.cancel(meeting_id);
        if matches!(outcome, CancelOutcome::Running) {
            // Set under the queue lock: the worker clears the flag under the same lock before it
            // starts the next job, so this request cannot leak into another job.
            CANCEL_CURRENT.store(true, Ordering::SeqCst);
        }
        outcome
    };
    match outcome {
        CancelOutcome::Queued(req) => {
            let _ = app.emit(
                ERROR_EVENT,
                serde_json::json!({
                    "meeting_id": meeting_id,
                    "error": "Speaker identification cancelled",
                    "automatic": req.automatic,
                    "cancelled": true
                }),
            );
            Ok(())
        }
        CancelOutcome::Running => Ok(()),
        CancelOutcome::NotFound => Err("No speaker identification is running for this meeting".into()),
    }
}

fn emit_progress<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, stage: &'static str, percent: u32, message: &str) {
    let updated = jobs().update(meeting_id, stage, percent, message);
    if let Some(s) = updated {
        let _ = app.emit(PROGRESS_EVENT, s);
    }
}

/// Outcome of a finished job.
pub struct IdentifyOutcome {
    pub speaker_count: usize,
    /// Shown to the user when rows with two speakers had to keep their majority label.
    pub warning: Option<String>,
}

async fn worker<R: Runtime>(app: AppHandle<R>) {
    loop {
        let next = {
            let mut q = jobs();
            let next = q.start_next();
            if next.is_none() {
                q.worker_running = false;
            } else {
                CANCEL_CURRENT.store(false, Ordering::SeqCst);
            }
            next
        };
        let Some(req) = next else { break };

        // Run each job in its own task: a panic comes back as an error and the queue keeps going.
        let (job_app, job_req) = (app.clone(), req.clone());
        let result = match tauri::async_runtime::spawn(async move { run_identify(&job_app, &job_req).await }).await {
            Ok(result) => result,
            Err(e) => Err(anyhow!("Speaker identification task panicked: {}", e)),
        };
        jobs().finish(&req.meeting_id);

        match result {
            Ok(outcome) => {
                log::info!("Speaker identification finished for {}: {} speakers", req.meeting_id, outcome.speaker_count);
                let _ = app.emit(
                    COMPLETE_EVENT,
                    serde_json::json!({
                        "meeting_id": req.meeting_id,
                        "speaker_count": outcome.speaker_count,
                        "automatic": req.automatic,
                        "warning": outcome.warning
                    }),
                );
            }
            Err(e) => {
                let cancelled = e.is::<Cancelled>();
                if cancelled {
                    log::info!("Speaker identification cancelled for {}", req.meeting_id);
                } else {
                    log::warn!("Speaker identification failed for {}: {:#}", req.meeting_id, e);
                }
                let _ = app.emit(
                    ERROR_EVENT,
                    serde_json::json!({
                        "meeting_id": req.meeting_id,
                        "error": format!("{e:#}"),
                        "automatic": req.automatic,
                        "cancelled": cancelled
                    }),
                );
            }
        }
    }
}

/// Find the meeting's audio file, waiting up to `timeout` for it to appear and stop growing.
pub async fn wait_for_audio(folder: &Path, timeout: Duration, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<PathBuf> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut last_size: Option<u64> = None;
    loop {
        if cancelled() {
            return Err(Cancelled.into());
        }
        if let Ok(path) = crate::audio::retranscription::find_audio_file(folder) {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if timeout.is_zero() || (size > 0 && last_size == Some(size)) {
                return Ok(path);
            }
            last_size = Some(size);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("No audio file found in {}", folder.display()));
        }
        tokio::time::sleep(LOCK_POLL).await;
    }
}

/// 16 kHz samples plus what the time map needs to know about the decoded file.
struct DecodedMeetingAudio {
    samples: Arc<Vec<f32>>,
    native_rate: u32,
    native_frames: usize,
}

async fn decode_16k(path: PathBuf) -> Result<DecodedMeetingAudio> {
    tokio::task::spawn_blocking(move || -> Result<DecodedMeetingAudio> {
        let decoded = decode_audio_file(&path)?;
        let native_rate = decoded.sample_rate;
        let native_frames = decoded.samples.len() / decoded.channels.max(1) as usize;
        // Consumes the decoded samples instead of copying them (long meetings are large).
        let samples = Arc::new(decoded.into_whisper_format());
        Ok(DecodedMeetingAudio { samples, native_rate, native_frames })
    })
    .await
    .map_err(|e| anyhow!("Decode task panicked: {}", e))?
}

/// Download models if needed, then diarize on a blocking thread. Only one diarization runs at a
/// time; `waiting` is called once when another one is running. `cancelled` is checked while
/// waiting, during the download and during diarization.
pub async fn diarize_samples(
    samples: Arc<Vec<f32>>,
    num_speakers: Option<usize>,
    on_download: impl Fn(DownloadProgress) + Send + Sync + 'static,
    waiting: impl Fn() + Send + 'static,
    progress: impl Fn(u32) + Send + 'static,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<Diarization> {
    let cancelled = Arc::new(cancelled);
    let _guard = match DIARIZE_LOCK.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            waiting();
            loop {
                if (*cancelled)() {
                    return Err(Cancelled.into());
                }
                if let Ok(guard) = tokio::time::timeout(LOCK_POLL, DIARIZE_LOCK.lock()).await {
                    break guard;
                }
            }
        }
    };
    let dir = models::models_directory()?;
    if !models::status(&dir).installed {
        let c = cancelled.clone();
        models::ensure_models(&dir, on_download, move || (*c)()).await?;
    }
    if (*cancelled)() {
        return Err(Cancelled.into());
    }
    let c = cancelled.clone();
    tokio::task::spawn_blocking(move || {
        let mut diarizer = Diarizer::load(&dir)?;
        let opts = DiarizeOptions { num_speakers, ..Default::default() };
        let mut report = |p: u32| progress(p);
        diarizer.diarize(&samples, &opts, &mut report, &|| (*c)())
    })
    .await
    .map_err(|e| anyhow!("Diarization task panicked: {}", e))?
}

/// New speakers for `d`, with names carried over from the meeting's previous speakers. Reads the
/// previous speakers through `conn`, so call it inside the write transaction.
pub async fn speaker_write_names(conn: &mut SqliteConnection, meeting_id: &str, d: &Diarization) -> Result<Vec<NewSpeaker>> {
    let old: Vec<(Option<String>, Vec<f32>)> = SpeakersRepository::list_conn(&mut *conn, meeting_id)
        .await?
        .into_iter()
        .filter_map(|s| s.embedding.map(|e| (s.display_name, e)))
        .collect();
    let new: Vec<(String, Vec<f32>)> = d.speakers.iter().map(|s| (s.key.clone(), s.embedding.clone())).collect();
    let names = carry_over_names(&new, &old, CARRY_OVER_MIN_SIMILARITY);
    Ok(d.speakers
        .iter()
        .map(|s| NewSpeaker {
            key: s.key.clone(),
            display_name: names.get(&s.key).cloned(),
            embedding: s.embedding.clone(),
            speech_seconds: s.speech_seconds,
        })
        .collect())
}

/// Rewrite the meeting folder's transcripts.json from the database.
pub async fn rewrite_transcripts_json(pool: &SqlitePool, meeting_id: &str, folder: &Path) -> Result<()> {
    // Held across the read and the write, so concurrent rewrites cannot interleave or write stale data.
    let _guard = JSON_REWRITE_LOCK.lock().await;
    let rows = sqlx::query(
        "SELECT id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker
         FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;
    let segments: Vec<TranscriptSegment> = rows
        .into_iter()
        .map(|r| TranscriptSegment {
            id: r.get("id"),
            text: r.get("transcript"),
            timestamp: r.get("timestamp"),
            audio_start_time: r.get("audio_start_time"),
            audio_end_time: r.get("audio_end_time"),
            duration: r.get("duration"),
            speaker: r.get("speaker"),
        })
        .collect();
    let labels = SpeakersRepository::labels(pool, meeting_id).await?;
    let folder = folder.to_path_buf();
    tokio::task::spawn_blocking(move || write_transcripts_json(&folder, &segments, &labels))
        .await
        .map_err(|e| anyhow!("transcripts.json write task panicked: {}", e))?
}

struct StoredRow {
    id: String,
    span: Option<RowSpan>,
}

/// Labels and splits produced for the stored rows.
struct RowUpdates {
    row_labels: Vec<(String, Option<String>)>,
    row_splits: Vec<(String, Vec<SplitRow>)>,
    pieces_done: usize,
    warning: Option<String>,
}

async fn meeting_exists(conn: &mut SqliteConnection, meeting_id: &str) -> Result<bool> {
    let found: Option<i64> = sqlx::query_scalar("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(found.is_some())
}

async fn run_identify<R: Runtime>(app: &AppHandle<R>, req: &IdentifyRequest) -> Result<IdentifyOutcome> {
    let started = Instant::now();
    let pool = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?
        .db_manager
        .pool()
        .clone();
    let meeting_id = req.meeting_id.clone();

    emit_progress(app, &meeting_id, "audio", 0, "Finding recording…");
    let wait = if req.automatic { AUTO_AUDIO_WAIT } else { Duration::ZERO };
    let audio_path = wait_for_audio(&req.folder_path, wait, &is_cancelled).await?;

    emit_progress(app, &meeting_id, "audio", 1, "Decoding audio…");
    let decoded = decode_16k(audio_path).await?;
    let samples = decoded.samples;
    let t_decode = started.elapsed();
    if is_cancelled() {
        return Err(Cancelled.into());
    }
    {
        let mut conn = pool.acquire().await?;
        if !meeting_exists(&mut conn, &meeting_id).await? {
            // Deleted while the job waited: stop before diarizing.
            return Err(Cancelled.into());
        }
    }
    let time_map = recording_time_map(read_metadata(&req.folder_path).as_ref(), decoded.native_rate, decoded.native_frames);

    let rows: Vec<StoredRow> = sqlx::query(
        "SELECT id, audio_start_time, audio_end_time FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time",
    )
    .bind(&meeting_id)
    .fetch_all(&pool)
    .await?
    .into_iter()
    .map(|r| {
        let (a, b): (Option<f64>, Option<f64>) = (r.get("audio_start_time"), r.get("audio_end_time"));
        StoredRow {
            id: r.get("id"),
            span: match (a, b) {
                (Some(a), Some(b)) if b > a => Some(RowSpan { start_s: a, end_s: b }),
                _ => None,
            },
        }
    })
    .collect();

    let diarize_started = Instant::now();
    let (app_dl, app_wait, app_p) = (app.clone(), app.clone(), app.clone());
    let (id_dl, id_wait, id_p) = (meeting_id.clone(), meeting_id.clone(), meeting_id.clone());
    let diarization = diarize_samples(
        samples.clone(),
        req.num_speakers,
        move |p| {
            super::commands::emit_download_progress(&app_dl, p.clone());
            emit_progress(&app_dl, &id_dl, "download", 2, &format!("Downloading speaker models… {}%", p.percent));
        },
        move || emit_progress(&app_wait, &id_wait, "waiting", 2, "Waiting for another speaker identification…"),
        move |p| {
            let stage = if p < 40 { "segmentation" } else if p < 90 { "embeddings" } else { "clustering" };
            emit_progress(&app_p, &id_p, stage, 5 + p * 80 / 100, "Identifying speakers…");
        },
        is_cancelled,
    )
    .await?;
    let t_diarize = diarize_started.elapsed();
    if diarization.speakers.is_empty() {
        return Err(anyhow!("No speech found in the recording"));
    }

    // Turns are in decoded-file time; rows and stored split pieces use the transcript clock.
    let clock_turns: Vec<Turn> = diarization
        .turns
        .iter()
        .map(|t| Turn { start_s: time_map.clock_s(t.start_s), end_s: time_map.clock_s(t.end_s), key: t.key.clone() })
        .collect();
    let spans: Vec<Option<RowSpan>> = rows.iter().map(|r| r.span).collect();
    let labels = label_rows(&spans, &clock_turns);
    let mixed_count = labels.iter().filter(|l| matches!(l, RowLabel::Mixed { .. })).count();

    // Never cut rows when the timing cannot be trusted: a wrong cut replaces text with other audio.
    let decoded_s = samples.len() as f64 / 16_000.0;
    let rows_end = rows.iter().filter_map(|r| r.span.map(|s| s.end_s)).fold(0.0, f64::max);
    let timing_ok = time_map.allows_splitting() && time_map.file_s(rows_end) <= decoded_s + 1.0;
    let mut split_warning: Option<String> = None;
    if mixed_count > 0 && !timing_ok {
        log::warn!(
            "Recording timing does not match the transcript for {} ({:?}, rows end at {:.1}s, audio {:.1}s); keeping majority labels",
            meeting_id, time_map, rows_end, decoded_s
        );
        split_warning = Some("Lines with two speakers were kept whole: the recording's timing could not be matched to the transcript".into());
    }

    let load_started = Instant::now();
    let mut batch_guard: Option<tokio::sync::OwnedMutexGuard<()>> = None;
    let engine = if mixed_count > 0 && timing_ok {
        if batch_engine_busy() {
            emit_progress(app, &meeting_id, "waiting", 86, "Waiting for another transcription to finish…");
        }
        // Held from engine load to unload, so another batch job cannot unload or swap the model.
        batch_guard = Some(loop {
            if is_cancelled() {
                return Err(Cancelled.into());
            }
            if let Ok(guard) = tokio::time::timeout(LOCK_POLL, acquire_batch_engine_lock()).await {
                break guard;
            }
        });
        emit_progress(app, &meeting_id, "splitting", 86, "Loading transcription engine…");
        match crate::audio::retranscription::load_configured_engine(app).await {
            Ok(engine) => Some(engine),
            Err(e) => {
                log::warn!("Cannot split rows with speaker changes, keeping majority labels: {:#}", e);
                split_warning = Some("Lines with two speakers were kept whole: the transcription engine could not load".into());
                None
            }
        }
    } else {
        None
    };
    let t_load = load_started.elapsed();

    let split_started = Instant::now();
    // Same language and translate setting as live transcription, so split text matches its rows.
    let language = crate::get_language_preference_internal();
    let updates = build_row_updates(app, &meeting_id, &rows, labels, &samples, time_map, engine.as_ref(), mixed_count, language).await;
    // Unload on every exit from the split phase (success, piece failure or cancel).
    if let Some(engine) = &engine {
        unload_engine_after_batch(engine.is_parakeet()).await;
    }
    drop(batch_guard);
    let RowUpdates { row_labels, row_splits, pieces_done, warning: piece_warning } = updates?;
    let t_split = split_started.elapsed();
    if is_cancelled() {
        return Err(Cancelled.into());
    }

    emit_progress(app, &meeting_id, "saving", 97, "Saving speakers…");
    let save_started = Instant::now();
    let mut conn = pool.acquire().await?;
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    if !meeting_exists(&mut tx, &meeting_id).await? {
        // Deleted while the job ran; dropping the transaction rolls it back.
        return Err(Cancelled.into());
    }
    // Previous names are read inside the transaction, so a rename committed meanwhile is kept.
    let speakers = speaker_write_names(&mut tx, &meeting_id, &diarization).await?;
    let speaker_count = speakers.len();
    SpeakersRepository::replace_for_meeting(&mut tx, &meeting_id, &SpeakerWrite { speakers, row_labels, row_splits }).await?;
    tx.commit().await?;
    drop(conn);
    if let Err(e) = rewrite_transcripts_json(&pool, &meeting_id, &req.folder_path).await {
        log::warn!("Failed to rewrite transcripts.json for {}: {:#}", meeting_id, e);
    }
    let t_save = save_started.elapsed();

    let total = started.elapsed();
    log::info!(
        "Identify job {}: {:.1}s audio | decode {:?} | diarize {:?} | engine load {:?} | split {} rows/{} pieces {:?} | save {:?} | total {:?} ({:.2}% of audio)",
        meeting_id,
        decoded_s,
        t_decode,
        t_diarize,
        t_load,
        mixed_count,
        pieces_done,
        t_split,
        t_save,
        total,
        total.as_secs_f64() / decoded_s.max(1e-9) * 100.0
    );
    emit_progress(app, &meeting_id, "done", 100, "Done");
    Ok(IdentifyOutcome { speaker_count, warning: split_warning.or(piece_warning) })
}

/// Label every row and re-transcribe the pieces of rows with a clear speaker change. A piece
/// that fails to transcribe leaves its row whole with the majority label.
#[allow(clippy::too_many_arguments)]
async fn build_row_updates<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    rows: &[StoredRow],
    labels: Vec<RowLabel>,
    samples: &[f32],
    time_map: TimeMap,
    engine: Option<&BatchEngine>,
    mixed_count: usize,
    language: Option<String>,
) -> Result<RowUpdates> {
    let mut out = RowUpdates { row_labels: Vec::new(), row_splits: Vec::new(), pieces_done: 0, warning: None };
    let mut done_mixed = 0usize;
    for (row, label) in rows.iter().zip(labels) {
        if is_cancelled() {
            return Err(Cancelled.into());
        }
        match label {
            RowLabel::Unlabeled => out.row_labels.push((row.id.clone(), None)),
            RowLabel::Single(k) => out.row_labels.push((row.id.clone(), Some(k))),
            RowLabel::Mixed { majority, pieces } => {
                done_mixed += 1;
                emit_progress(
                    app,
                    meeting_id,
                    "splitting",
                    86 + (done_mixed * 10 / mixed_count.max(1)) as u32,
                    "Splitting lines with speaker changes…",
                );
                let Some(engine) = engine else {
                    out.row_labels.push((row.id.clone(), Some(majority)));
                    continue;
                };
                let mut split = Vec::new();
                for p in &pieces {
                    // Pieces are in transcript time; cut the audio at the matching file positions.
                    let a = ((time_map.file_s(p.start_s) * 16000.0) as usize).min(samples.len());
                    let b = ((time_map.file_s(p.end_s) * 16000.0) as usize).min(samples.len());
                    if b <= a {
                        continue;
                    }
                    let text = match engine.transcribe(samples[a..b].to_vec(), language.clone()).await {
                        Ok(t) => t,
                        Err(e) => {
                            log::warn!("Piece transcription failed for row {}, keeping majority label: {:#}", row.id, e);
                            out.warning.get_or_insert_with(|| "Some lines with two speakers were kept whole".into());
                            split.clear();
                            break;
                        }
                    };
                    out.pieces_done += 1;
                    if !text.trim().is_empty() {
                        split.push(SplitRow { text: text.trim().to_string(), start_s: p.start_s, end_s: p.end_s, speaker: p.key.clone() });
                    }
                }
                if split.is_empty() {
                    out.row_labels.push((row.id.clone(), Some(majority)));
                } else {
                    out.row_splits.push((row.id.clone(), split));
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str) -> IdentifyRequest {
        IdentifyRequest { meeting_id: id.into(), folder_path: PathBuf::from("/tmp"), num_speakers: None, automatic: false }
    }

    #[test]
    fn job_queue_rejects_duplicate_meeting() {
        let mut q = JobQueue::default();
        q.push(req("a")).unwrap();
        assert!(q.push(req("a")).is_err());
        q.push(req("b")).unwrap();
        assert_eq!(q.status("a").unwrap().state, JobState::Queued);
        assert_eq!(q.status("a").unwrap().stage, None);
        assert_eq!(q.start_next().unwrap().meeting_id, "a");
        assert_eq!(q.status("a").unwrap().state, JobState::Running);
        assert!(q.push(req("a")).is_err(), "running meeting must reject a second job");
        q.finish("a");
        q.push(req("a")).unwrap();
    }

    #[test]
    fn job_queue_runs_in_order_and_cancels_queued_jobs() {
        let mut q = JobQueue::default();
        q.push(req("a")).unwrap();
        q.push(req("b")).unwrap();
        q.push(req("c")).unwrap();
        assert_eq!(q.cancel_queued("b").map(|r| r.meeting_id), Some("b".to_string()));
        let first = q.start_next().unwrap();
        assert_eq!(first.meeting_id, "a");
        assert_eq!(q.status("a").unwrap().state, JobState::Running);
        q.finish("a");
        assert_eq!(q.start_next().unwrap().meeting_id, "c");
        assert!(q.status("b").is_none());
        q.finish("c");
        assert!(q.start_next().is_none());
    }

    #[test]
    fn cancel_reports_queued_running_and_unknown_jobs() {
        let mut q = JobQueue::default();
        q.push(IdentifyRequest { automatic: true, ..req("a") }).unwrap();
        q.push(req("b")).unwrap();
        assert_eq!(q.start_next().unwrap().meeting_id, "a");
        assert!(matches!(q.cancel("a"), CancelOutcome::Running));
        assert!(q.status("a").is_some(), "a running job stays visible until it stops");
        match q.cancel("b") {
            CancelOutcome::Queued(r) => assert!(!r.automatic),
            other => panic!("expected a queued job, got {other:?}"),
        }
        assert!(q.status("b").is_none());
        assert!(matches!(q.cancel("zzz"), CancelOutcome::NotFound));
    }

    #[test]
    fn progress_updates_without_changes_send_no_event() {
        let mut q = JobQueue::default();
        q.push(req("a")).unwrap();
        assert!(q.update("a", "segmentation", 10, "x").is_some());
        assert!(q.update("a", "segmentation", 10, "x").is_none());
        assert!(q.update("a", "segmentation", 11, "x").is_some());
        let s = q.update("a", "embeddings", 11, "x").unwrap();
        assert_eq!(s.stage, Some("embeddings"));
        assert!(q.update("missing", "segmentation", 1, "x").is_none());
    }

    #[test]
    fn busy_message_reports_queued_and_running_jobs() {
        let mut q = JobQueue::default();
        assert!(q.busy_message("a").is_none());
        q.push(req("a")).unwrap();
        assert!(q.busy_message("a").is_some());
        q.start_next();
        assert!(q.busy_message("a").is_some());
        q.finish("a");
        assert!(q.busy_message("a").is_none());
    }

    #[test]
    fn retranscription_claim_is_released_on_drop() {
        let id = "claim-test-meeting";
        let claim = claim_for_retranscription(id).unwrap();
        assert!(is_retranscribing(id));
        // A second claim on the same meeting is refused and must not release the first one.
        assert!(claim_for_retranscription(id).is_err());
        assert!(is_retranscribing(id));
        drop(claim);
        assert!(!is_retranscribing(id));
    }

    #[tokio::test]
    async fn wait_for_audio_finds_a_file_that_appears_late() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.mp4");
        let writer = {
            let path = path.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                std::fs::write(path, b"audio").unwrap();
            })
        };
        let found = wait_for_audio(dir.path(), Duration::from_secs(5), &|| false).await.unwrap();
        assert_eq!(found, path);
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn wait_for_audio_gives_up_when_no_file_appears() {
        let dir = tempfile::tempdir().unwrap();
        assert!(wait_for_audio(dir.path(), Duration::from_millis(200), &|| false).await.is_err());
        assert!(wait_for_audio(dir.path(), Duration::ZERO, &|| false).await.is_err());
    }

    #[tokio::test]
    async fn wait_for_audio_waits_until_the_file_stops_growing() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.mp4");
        std::fs::write(&path, b"a").unwrap();
        let writer = {
            let path = path.clone();
            tokio::spawn(async move {
                for _ in 0..15 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
                    f.write_all(b"a").unwrap();
                }
            })
        };
        let found = wait_for_audio(dir.path(), Duration::from_secs(5), &|| false).await.unwrap();
        assert_eq!(found, path);
        // Returned only after the writer stopped: the file holds all 16 bytes.
        assert_eq!(std::fs::metadata(&found).unwrap().len(), 16);
        assert!(writer.is_finished());
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn wait_for_audio_stops_when_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let err = wait_for_audio(dir.path(), Duration::from_secs(5), &|| true).await.unwrap_err();
        assert!(err.is::<Cancelled>());
    }
}
