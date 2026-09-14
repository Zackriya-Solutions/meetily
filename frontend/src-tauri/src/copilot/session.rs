//! The live-context service (BACKLOG I2, ADR-0038).
//!
//! Everything the copilot will ever know about the conversation *as it
//! happens* passes through here: a rolling window of recent speech for I3's
//! insights, a durable per-session buffer for a recap, and a deterministic cue
//! when someone just asked or requested something ([`super::cue`]).
//!
//! ## What it is built on — and two things the plan got wrong about it
//!
//! The service is a **consumer of the existing `transcript-update` event**
//! (invariant 2: the copilot never captures; `audio/` is untouched). It
//! subscribes the same way `audio/recording_commands.rs:323` does — `app.listen`
//! with the `EventId` kept for `unlisten`. Two facts about that producer
//! (`audio/transcription/worker.rs`) contradict BACKLOG I2's original wording,
//! and the code follows the producer, not the plan:
//!
//! - **`is_partial` is not a lifecycle.** Whisper sets it as
//!   `duration_seconds < 15.0` (`whisper_engine.rs`) — it means "short chunk",
//!   full stop. Every chunk is emitted exactly once with a fresh
//!   `sequence_id`; no later "final" ever replaces an earlier "partial". The
//!   live VAD closes a segment after 400 ms of silence (`audio/pipeline.rs`),
//!   so nearly every real utterance is well under 15 s and therefore flagged
//!   partial. A service that dropped partials would drop almost the whole
//!   meeting. Every emission is a final segment here; the flag is carried as
//!   [`Turn::short_chunk`] and decides nothing.
//! - **`source` does not name a device.** The producer hardcodes it to
//!   `"Audio"` for microphone and system audio alike. A turn's [`Channel`] is
//!   therefore [`Channel::Unknown`] today. The rule "no cue from the user's own
//!   microphone" is real and tested, but it cannot fire until the pipeline
//!   carries attribution — an `audio/` change that is not this epic's to make.
//!
//! ## Ordering
//!
//! The buffer is ordered by **audio time**, which is what "the last 180
//! seconds" means; `sequence_id` is only the identity used to replace a
//! re-emitted segment rather than duplicate it.
//!
//! Today that ordering is **defence in depth, not a response to observed
//! behaviour**: the producer runs a single worker on purpose —
//! `worker.rs`'s `const NUM_WORKERS: usize = 1; // Serial processing ensures
//! transcripts emit in chronological order` — so `sequence_id` order and audio
//! order currently coincide, and every insert is an append. (The
//! `"workers": 3` literal in the `recording-started` payload is stale and
//! describes nothing; it is `audio/`'s to fix, not this epic's.) Sorting by
//! audio time costs one `partition_point` per segment and means raising
//! `NUM_WORKERS` again cannot silently corrupt "the last three minutes".
//!
//! ## What never happens here
//!
//! No model call, no file, no database row, no network, no log line containing
//! transcript text. The buffer lives in memory for one recording session: it is
//! rebuilt at `recording-started` — which is also where the tenant is
//! re-resolved from [`AuthContext`], so a Phase-2 tenant switch between two
//! meetings can never leave a buffer stamped with the previous workspace — and
//! cleared at `recording-stopped` (the ordinary stop path,
//! `audio/recording_commands.rs`) **and** `recording-stop-complete` (the tray's
//! stop, `tray.rs`; no other code emits it). With the copilot disabled nothing
//! subscribes at all — [`should_subscribe`] is the one decision, and
//! [`apply_config`] cannot reach `listen` without it.

use super::config::CopilotConfig;
use super::cue::{self, CueKind};
use crate::audio::transcription::worker::TranscriptUpdate;
use crate::context::{self, AuthContext, TenantId};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, EventId, Listener, Runtime};

/// Event the service emits when a cue is detected.
pub const CUE_EVENT: &str = "copilot-cue";

/// Upper bound on the durable buffer, in segments. At one segment every few
/// seconds this is many hours of speech — far past any meeting, but a bound
/// nonetheless, because "durable for the session" must not mean "unbounded for
/// a session someone forgot to stop". Oldest segments go first.
///
/// **This, not the window, is what bounds memory.** The configured window
/// (`live_window_secs`) selects which turns an insight will be built from in
/// I3; it does not evict anything. A three-hour meeting is resident in full
/// while it is being recorded, whatever the window is set to — see
/// [`discard_if_idle`] and `docs/SECURITY_PRIVACY.md` for what bounds it.
pub const DURABLE_CAP: usize = 20_000;

/// A segment that ends less than this many seconds before the next one starts
/// is read as the same sentence continuing across a VAD split.
///
/// **Not derived from the VAD setting** — deliberately much larger than it. The
/// live capture path closes a segment after **400 ms** of silence
/// (`audio/pipeline.rs`'s `redemption_time`; the 2 000 ms
/// `VAD_REDEMPTION_TIME_MS` belongs to the offline import and retranscription
/// paths only). At 400 ms a single spoken question is split readily, and the
/// pause a speaker leaves mid-sentence — thinking, or waiting for the other
/// side — is far longer than the VAD's. Three seconds is a generous bound
/// chosen to cover that, not to match the segmenter.
///
/// Only the *immediately* preceding segment is ever joined, so a question
/// broken into three or more pieces is still read from its last two.
pub const CONTINUATION_GAP_SECS: f64 = 3.0;

/// Which capture device a segment came from — **as far as the producer says**.
///
/// The producer says nothing today (`source: "Audio"`), so every live turn is
/// `Unknown`. The variants exist so the rule that depends on them is written
/// once, now, and starts working the day the pipeline carries attribution.
/// Nothing here infers a speaker from voice (ADR-0034).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    /// The user's own microphone.
    Microphone,
    /// System audio — everyone else on the call.
    System,
    /// The producer did not say.
    Unknown,
}

impl Channel {
    pub fn from_source(source: &str) -> Self {
        let s = source.trim().to_ascii_lowercase();
        if s.contains("mic") {
            Channel::Microphone
        } else if s.contains("system") || s.contains("loopback") {
            Channel::System
        } else {
            Channel::Unknown
        }
    }
}

/// One transcript segment as the live context keeps it.
///
/// Deliberately **not** `Serialize`: a turn carries transcript text, and the
/// only payloads that leave this module today are counts (`LiveContextStatus`)
/// and ids (`CueEvent`). The derive comes back when I3 has a consumer for the
/// window, and that consumer's shape is reviewed then.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub sequence_id: u64,
    pub text: String,
    pub channel: Channel,
    /// Seconds from recording start.
    pub audio_start: f64,
    pub audio_end: f64,
    /// The producer's `is_partial`, which means "under 15 s of audio" and
    /// nothing more. Kept for I3's prompt assembly, never used to drop a turn.
    pub short_chunk: bool,
}

impl Turn {
    fn from_update(update: &TranscriptUpdate, text: String) -> Self {
        Turn {
            sequence_id: update.sequence_id,
            text,
            channel: Channel::from_source(&update.source),
            audio_start: update.audio_start_time,
            audio_end: update.audio_end_time,
            short_chunk: update.is_partial,
        }
    }
}

/// A detected cue, with the segments it rests on.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cue {
    pub kind: CueKind,
    pub confidence: f32,
    /// `sequence_id`s of the evidence, in audio order — one segment, or two
    /// when a question was split across a VAD boundary.
    pub evidence: Vec<u64>,
}

/// What [`LiveContext::ingest`] did with one update.
#[derive(Clone, Debug, PartialEq)]
pub struct Ingested {
    /// False when the update carried no text, or when it re-emitted a segment
    /// already held (which is replaced in place, not duplicated).
    pub added: bool,
    pub cue: Option<Cue>,
}

/// The pure core: a bounded, audio-ordered buffer with a time window over it.
///
/// No Tauri type appears here so every rule is unit-testable without a window
/// or an app handle. The Tauri glue below feeds it.
#[derive(Debug)]
pub struct LiveContext {
    /// The workspace this context belongs to, captured from [`AuthContext`]
    /// when the session started. In-memory state is still tenant state
    /// (`docs/MULTITENANCY.md` rule 2): I3 must never hand a window from one
    /// tenant to a retrieval for another, and this is what it checks against.
    tenant_id: TenantId,
    window_secs: u32,
    /// Ordered by `audio_start`, then `sequence_id`.
    turns: Vec<Turn>,
    /// The latest `audio_end` seen; the window is measured back from it.
    latest_end: f64,
    /// The highest `sequence_id` held. Ids are assigned in emission order, so
    /// a new id above this cannot be a re-emission and needs no scan.
    max_sequence_id: Option<u64>,
    /// Segments already reported inside some cue's `evidence`.
    ///
    /// One question can arrive as three VAD segments at 400 ms, and the
    /// continuation join only looks one segment back. Asking "did the
    /// predecessor fire *on its own*" was not enough: in "so can | you extend
    /// the deadline | by two weeks please" the middle segment is silent alone,
    /// so it was joined twice and the same question was reported as two cues
    /// citing overlapping evidence. Membership here subsumes that check — a
    /// segment that fired alone is in the set too.
    reported: std::collections::HashSet<u64>,
    /// Segments evicted from the front by [`DURABLE_CAP`]. Reported, never hidden.
    evicted: u64,
}

impl LiveContext {
    pub fn new(ctx: &AuthContext, window_secs: u32) -> Self {
        LiveContext {
            tenant_id: ctx.tenant_id.clone(),
            window_secs,
            turns: Vec::new(),
            latest_end: 0.0,
            max_sequence_id: None,
            reported: std::collections::HashSet::new(),
            evicted: 0,
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn window_secs(&self) -> u32 {
        self.window_secs
    }

    pub fn set_window_secs(&mut self, secs: u32) {
        self.window_secs = secs;
    }

    /// Every segment of the session, in audio order.
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// The segments that end within the last `window_secs` of speech.
    ///
    /// A *view*, not a retention policy: nothing is evicted by the window. It
    /// exists so I3 can build a prompt from recent speech without re-reading
    /// the whole meeting.
    ///
    /// Measured from the latest segment *end* seen, not from the wall clock:
    /// the window is "the last three minutes of conversation", and a pause in
    /// the meeting does not empty it. Because turns are ordered by start and a
    /// segment may run up to 25 s, "ends after the cutoff" is not a suffix of
    /// the vector — so this filters rather than slices. At most a few thousand
    /// turns, read on demand: not a hot path.
    pub fn window(&self) -> impl Iterator<Item = &Turn> {
        let cutoff = self.latest_end - f64::from(self.window_secs);
        self.turns.iter().filter(move |t| t.audio_end >= cutoff)
    }

    pub fn window_len(&self) -> usize {
        self.window().count()
    }

    /// Forget the session. Called at recording start and after stop.
    pub fn reset(&mut self) {
        self.turns.clear();
        self.latest_end = 0.0;
        self.max_sequence_id = None;
        self.reported.clear();
        self.evicted = 0;
    }

    /// Take one `transcript-update`.
    pub fn ingest(&mut self, update: &TranscriptUpdate) -> Ingested {
        let text = update.text.trim();
        if text.is_empty() {
            return Ingested {
                added: false,
                cue: None,
            };
        }
        let turn = Turn::from_update(update, text.to_string());

        // A re-emitted segment is replaced, never duplicated, and never
        // produces a second cue for the same moment. The main transcript view
        // makes the same call (`TranscriptContext.tsx`, "Duplicate transcript
        // update skipped"). Only an id at or below the highest seen can be a
        // re-emission, so the scan runs on that rare path alone.
        let may_be_duplicate = self
            .max_sequence_id
            .is_some_and(|max| turn.sequence_id <= max);
        if may_be_duplicate {
            if let Some(existing) = self
                .turns
                .iter_mut()
                .find(|t| t.sequence_id == turn.sequence_id)
            {
                *existing = turn;
                return Ingested {
                    added: false,
                    cue: None,
                };
            }
        }

        self.latest_end = self.latest_end.max(turn.audio_end);
        self.max_sequence_id = Some(
            self.max_sequence_id
                .map_or(turn.sequence_id, |max| max.max(turn.sequence_id)),
        );

        // Insert in audio order. Almost always at the end; occasionally a
        // slower worker delivers an earlier chunk late.
        let at = self.turns.partition_point(|t| {
            (t.audio_start, t.sequence_id) < (turn.audio_start, turn.sequence_id)
        });
        self.turns.insert(at, turn);

        let mut excess = 0;
        if self.turns.len() > DURABLE_CAP {
            excess = self.turns.len() - DURABLE_CAP;
            self.turns.drain(..excess);
            self.evicted += excess as u64;
        }

        // The drain shifted everything left by `excess`. A turn that landed
        // inside the drained prefix — a very late chunk arriving at a full
        // buffer — is already gone: it is not in the buffer, so it was not
        // added and it cannot be a cue.
        let surviving = at.checked_sub(excess);
        let cue = surviving.and_then(|i| self.cue_for(i));
        if let Some(cue) = &cue {
            self.reported.extend(cue.evidence.iter().copied());
        }
        Ingested {
            added: surviving.is_some(),
            cue,
        }
    }

    /// Decide whether the turn at `idx` — possibly together with the one
    /// before it — is a cue.
    fn cue_for(&self, idx: usize) -> Option<Cue> {
        let turn = &self.turns[idx];

        // The user's own question is not something to offer the user help
        // with. Inert while the producer reports `Unknown`; see the module doc.
        if turn.channel == Channel::Microphone {
            return None;
        }

        // Join with the previous segment when it reads as the same sentence
        // continuing across a VAD split: it did not end a sentence, the gap is
        // short, it came from the same side, and it has not already been
        // reported inside a cue — which would make this a second cue for one
        // question, citing a segment the user was already offered.
        let continuation = idx.checked_sub(1).map(|p| &self.turns[p]).filter(|prev| {
            prev.channel != Channel::Microphone
                && !ends_sentence(&prev.text)
                && turn.audio_start - prev.audio_end <= CONTINUATION_GAP_SECS
                && !self.reported.contains(&prev.sequence_id)
        });

        let (text, evidence) = match continuation {
            Some(prev) => (
                format!("{} {}", prev.text, turn.text),
                vec![prev.sequence_id, turn.sequence_id],
            ),
            None => (turn.text.clone(), vec![turn.sequence_id]),
        };

        cue::detect(&text).map(|d| Cue {
            kind: d.kind,
            confidence: d.confidence,
            evidence,
        })
    }
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end().ends_with(['.', '?', '!', '…', ':', ';'])
}

/// The one decision that gates every subscription. Pure so the dormant-seam
/// promise — copilot off means nothing listens — is a unit test, not a hope.
pub fn should_subscribe(config: &CopilotConfig) -> bool {
    config.enabled
}

// --- Tauri glue ---------------------------------------------------------------

/// Payload of [`CUE_EVENT`]. **Ids and numbers only, no text**: every webview
/// that can hear this already holds the transcript from `transcript-update`
/// and resolves `evidence` against it, so carrying the words here would add a
/// second copy of meeting content to the event bus for nothing. `workspace_id`
/// is carried so a consumer can refuse a cue that is not its own — I3's
/// consumer **must** compare it with `api_get_current_workspace_id()` and drop
/// a mismatch (MULTITENANCY rule 4 applied to a message).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CueEvent {
    pub workspace_id: String,
    pub kind: CueKind,
    pub confidence: f32,
    /// The segments the cue rests on, in audio order.
    pub evidence: Vec<u64>,
    /// The segment whose arrival produced the cue (always the last of `evidence`).
    pub sequence_id: u64,
}

/// What Settings and the panel are told about the service.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveContextStatus {
    /// Is the service listening to `transcript-update` right now?
    pub subscribed: bool,
    pub window_secs: u32,
    /// Segments held for this session.
    pub turns: usize,
    /// Segments inside the rolling window.
    pub window_turns: usize,
    pub evicted: u64,
}

struct Subscription {
    transcript: EventId,
    started: EventId,
    /// `recording-stopped`, the ordinary stop path.
    stopped: EventId,
    /// `recording-stop-complete`, emitted only by the tray's stop.
    stopped_from_tray: EventId,
}

static SUBSCRIPTION: Mutex<Option<Subscription>> = Mutex::new(None);
static CONTEXT: Mutex<Option<LiveContext>> = Mutex::new(None);

fn subscription() -> MutexGuard<'static, Option<Subscription>> {
    SUBSCRIPTION.lock().unwrap_or_else(|e| e.into_inner())
}

/// The session's context. Named `live` so it cannot be misread as the
/// `crate::context` (AuthContext) module used a few lines below.
fn live() -> MutexGuard<'static, Option<LiveContext>> {
    CONTEXT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Make the running service match the settings.
///
/// Off ⇒ [`stop`]. On ⇒ a context exists for the current [`AuthContext`], its
/// window matches the settings, and exactly one set of listeners is held.
/// Idempotent: calling it twice registers nothing twice.
///
/// The window is taken **clamped**: `copilot_set_config` rejects an
/// out-of-range value, but a hand-edited or corrupt `copilot.json` can still
/// hold `0`, and an always-empty window would look like a broken service.
pub fn apply_config<R: Runtime>(app: &AppHandle<R>, config: &CopilotConfig) {
    if !should_subscribe(config) {
        stop(app);
        return;
    }

    let window_secs = config.live_window_secs_clamped();
    {
        let mut ctx = live();
        match ctx.as_mut() {
            Some(existing) => existing.set_window_secs(window_secs),
            None => *ctx = Some(LiveContext::new(&context::current(), window_secs)),
        }
    }

    let mut sub = subscription();
    if sub.is_some() {
        return;
    }

    let on_transcript = {
        let app = app.clone();
        app.clone().listen("transcript-update", move |event| {
            let Ok(update) = serde_json::from_str::<TranscriptUpdate>(event.payload()) else {
                return;
            };
            let cue = {
                let mut ctx = live();
                let Some(ctx) = ctx.as_mut() else { return };
                let ingested = ctx.ingest(&update);
                ingested.cue.map(|c| CueEvent {
                    workspace_id: ctx.tenant_id().to_string(),
                    kind: c.kind,
                    confidence: c.confidence,
                    evidence: c.evidence,
                    sequence_id: update.sequence_id,
                })
            };
            if let Some(cue) = cue {
                if let Err(e) = app.emit(CUE_EVENT, &cue) {
                    // The error text is Tauri's, never the transcript's.
                    log::warn!("copilot: could not emit {CUE_EVENT}: {e}");
                }
            }
        })
    };
    // A new recording is a new session — and a fresh identity resolution. A
    // finished one leaves nothing behind, whichever of the two stop paths
    // ended it.
    let on_started = app.listen("recording-started", |_| start_session());
    let on_stopped = app.listen("recording-stopped", |_| reset_context());
    let on_stopped_from_tray = app.listen("recording-stop-complete", |_| reset_context());

    *sub = Some(Subscription {
        transcript: on_transcript,
        started: on_started,
        stopped: on_stopped,
        stopped_from_tray: on_stopped_from_tray,
    });
    log::debug!("copilot: live context subscribed to transcript-update");
}

/// Begin a session: an empty buffer stamped with **whoever is signed in now**.
///
/// Re-resolving [`AuthContext`] here rather than reusing the one captured at
/// `apply_config` is what keeps a Phase-2 tenant switch between two meetings
/// from leaving tenant A's id on tenant B's cues. In Phase 1 the resolver is
/// constant and this is indistinguishable from a reset. Does nothing while the
/// service is stopped.
fn start_session() {
    let mut ctx = live();
    if let Some(window_secs) = ctx.as_ref().map(LiveContext::window_secs) {
        *ctx = Some(LiveContext::new(&context::current(), window_secs));
    }
}

fn reset_context() {
    if let Some(ctx) = live().as_mut() {
        ctx.reset();
    }
}

/// Drop a session buffer that has outlived its recording.
///
/// The two stop events are the primary bound on how long a meeting's text stays
/// in memory, but they are emitted on best-effort paths, so this is a second,
/// independent one: [`commands::build_status`] already asks the recorder
/// whether a session is running, and a buffer with no recording behind it is
/// dropped on that answer. The panel polls status every few seconds, so in
/// practice the window is short.
///
/// **What it does not cover, stated plainly:** if `stop_recording` fails early
/// it returns before setting `IS_RECORDING` to false (`audio/recording_commands.rs`
/// stores it well after its error return) *and* before emitting
/// `recording-stopped`. The app then still believes a recording is running, and
/// nothing here can tell that apart from a live meeting. In that case the
/// buffer is released when the copilot is switched off, when the next recording
/// starts, or at exit. Fixing the recorder's own state on that path is an
/// `audio/` change and belongs to that subsystem (CLAUDE.md §4).
pub fn discard_if_idle(recording: bool) {
    if recording {
        return;
    }
    if let Some(ctx) = live().as_mut() {
        if !ctx.turns().is_empty() {
            log::debug!("copilot: dropping a live context with no recording behind it");
            ctx.reset();
        }
    }
}

/// Release every listener and forget the session.
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    if let Some(sub) = subscription().take() {
        app.unlisten(sub.transcript);
        app.unlisten(sub.started);
        app.unlisten(sub.stopped);
        app.unlisten(sub.stopped_from_tray);
        log::debug!("copilot: live context unsubscribed");
    }
    *live() = None;
}

/// Run `f` against the live context, if there is one.
///
/// The **only** way out of this module for the window's contents, and
/// deliberately a callback rather than a getter: the context lives behind a
/// process-global `std::sync::Mutex`, and handing a caller its guard would let
/// that guard be held across an `await` — blocking every `transcript-update`
/// for the length of a model call, and inviting a deadlock. `f` is synchronous
/// by type, so that cannot happen. `copilot::insight::prepare` is written
/// synchronous for exactly this reason.
pub fn with_context<T>(f: impl FnOnce(Option<&LiveContext>) -> T) -> T {
    f(live().as_ref())
}

pub fn is_subscribed() -> bool {
    subscription().is_some()
}

/// A content-free snapshot for the status command. Counts only — no text.
pub fn status() -> LiveContextStatus {
    let subscribed = is_subscribed();
    match live().as_ref() {
        Some(ctx) => LiveContextStatus {
            subscribed,
            window_secs: ctx.window_secs(),
            turns: ctx.turns().len(),
            window_turns: ctx.window_len(),
            evicted: ctx.evicted(),
        },
        None => LiveContextStatus {
            subscribed,
            window_secs: 0,
            turns: 0,
            window_turns: 0,
            evicted: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(seq: u64, start: f64, dur: f64, text: &str) -> TranscriptUpdate {
        TranscriptUpdate {
            text: text.to_string(),
            timestamp: "14:00:00".to_string(),
            // Exactly what the producer sends today.
            source: "Audio".to_string(),
            sequence_id: seq,
            chunk_start_time: start,
            // Anything under 15 s, i.e. nearly everything live.
            is_partial: dur < 15.0,
            confidence: 0.9,
            audio_start_time: start,
            audio_end_time: start + dur,
            duration: dur,
        }
    }

    fn ctx(window_secs: u32) -> LiveContext {
        LiveContext::new(&AuthContext::local(), window_secs)
    }

    // --- The dormant seam ----------------------------------------------------

    #[test]
    fn nothing_subscribes_while_the_copilot_is_off() {
        let off = CopilotConfig::default();
        assert!(
            !off.enabled,
            "default must be off for this test to mean anything"
        );
        assert!(!should_subscribe(&off));
        let on = CopilotConfig {
            enabled: true,
            ..CopilotConfig::default()
        };
        assert!(should_subscribe(&on));
    }

    // --- Tenant --------------------------------------------------------------

    #[test]
    fn the_context_carries_the_workspace_it_was_started_for() {
        let c = ctx(180);
        assert_eq!(c.tenant_id(), &AuthContext::local().tenant_id);
    }

    // --- The producer's real semantics ----------------------------------------

    #[test]
    fn short_chunks_the_producer_calls_partial_are_kept() {
        // BACKLOG I2 originally said partials never enter the buffer. The
        // producer's "partial" is "under 15 s"; dropping it drops the meeting.
        let mut c = ctx(180);
        let r = c.ingest(&update(1, 0.0, 3.2, "Bunu ne zaman teslim edebilirsiniz?"));
        assert!(r.added);
        assert_eq!(c.turns().len(), 1);
        assert!(c.turns()[0].short_chunk);
    }

    #[test]
    fn the_producer_does_not_attribute_a_channel_today() {
        let mut c = ctx(180);
        c.ingest(&update(1, 0.0, 3.0, "hello"));
        assert_eq!(c.turns()[0].channel, Channel::Unknown);
        // And the mapping is ready for the day it does.
        assert_eq!(Channel::from_source("microphone"), Channel::Microphone);
        assert_eq!(
            Channel::from_source("Microphone (Realtek)"),
            Channel::Microphone
        );
        assert_eq!(Channel::from_source("system"), Channel::System);
        assert_eq!(Channel::from_source("Audio"), Channel::Unknown);
    }

    #[test]
    fn empty_text_is_ignored() {
        let mut c = ctx(180);
        let r = c.ingest(&update(1, 0.0, 1.0, "   "));
        assert!(!r.added);
        assert!(c.turns().is_empty());
    }

    // --- Replacement, not duplication ----------------------------------------

    #[test]
    fn a_re_emitted_segment_replaces_itself_and_fires_no_second_cue() {
        let mut c = ctx(180);
        let first = c.ingest(&update(7, 10.0, 3.0, "Hazır mısınız"));
        assert!(first.cue.is_some());
        let again = c.ingest(&update(7, 10.0, 3.0, "Hazır mısınız efendim"));
        assert!(!again.added);
        assert_eq!(again.cue, None, "a replacement must not offer twice");
        assert_eq!(c.turns().len(), 1);
        assert_eq!(c.turns()[0].text, "Hazır mısınız efendim");
    }

    // --- Ordering and the window ---------------------------------------------

    #[test]
    fn turns_are_kept_in_audio_order_even_when_workers_finish_out_of_order() {
        let mut c = ctx(180);
        // Worker B finishes chunk at 40 s before worker A finishes 37 s.
        c.ingest(&update(10, 40.0, 3.0, "later chunk"));
        c.ingest(&update(11, 37.0, 2.0, "earlier chunk"));
        let starts: Vec<f64> = c.turns().iter().map(|t| t.audio_start).collect();
        assert_eq!(starts, vec![37.0, 40.0]);
    }

    #[test]
    fn the_window_evicts_by_audio_time_not_by_count() {
        let mut c = ctx(180);
        for (seq, start) in [(1, 0.0), (2, 100.0), (3, 200.0), (4, 300.0)] {
            c.ingest(&update(seq, start, 5.0, "statement."));
        }
        // Latest end is 305; cutoff is 125. Segment 1 (ends 5) and 2 (ends 105)
        // are out; 3 and 4 are in. The durable buffer keeps all four.
        let in_window: Vec<u64> = c.window().map(|t| t.sequence_id).collect();
        assert_eq!(in_window, vec![3, 4]);
        assert_eq!(c.turns().len(), 4);
        assert_eq!(c.window_len(), 2);
    }

    #[test]
    fn a_segment_that_starts_before_the_cutoff_but_ends_after_it_is_in_the_window() {
        let mut c = ctx(60);
        c.ingest(&update(1, 0.0, 25.0, "a long segment.")); // ends 25
        c.ingest(&update(2, 80.0, 2.0, "recent.")); // ends 82, cutoff 22
        let in_window: Vec<u64> = c.window().map(|t| t.sequence_id).collect();
        assert_eq!(in_window, vec![1, 2], "ends after cutoff ⇒ in window");
    }

    #[test]
    fn a_pause_in_the_meeting_does_not_empty_the_window() {
        // The window is measured from the latest speech, not the wall clock:
        // with nothing new ingested, nothing changes.
        let mut c = ctx(180);
        c.ingest(&update(1, 0.0, 3.0, "statement."));
        assert_eq!(c.window_len(), 1);
        assert_eq!(c.window_len(), 1);
    }

    #[test]
    fn changing_the_window_applies_to_what_is_already_held() {
        let mut c = ctx(180);
        c.ingest(&update(1, 0.0, 5.0, "a."));
        c.ingest(&update(2, 100.0, 5.0, "b."));
        assert_eq!(c.window_len(), 2);
        c.set_window_secs(30);
        assert_eq!(c.window_len(), 1);
    }

    #[test]
    fn the_durable_buffer_is_bounded_and_says_what_it_dropped() {
        let mut c = ctx(180);
        for i in 0..(DURABLE_CAP as u64 + 5) {
            c.ingest(&update(i, i as f64 * 2.4, 2.0, "x."));
        }
        assert_eq!(c.turns().len(), DURABLE_CAP);
        assert_eq!(c.evicted(), 5);
        // The oldest went first.
        assert_eq!(c.turns()[0].sequence_id, 5);
    }

    #[test]
    fn reset_forgets_the_session() {
        let mut c = ctx(180);
        c.ingest(&update(1, 0.0, 3.0, "Hazır mısınız"));
        c.reset();
        assert!(c.turns().is_empty());
        assert_eq!(c.window_len(), 0);
        assert_eq!(c.evicted(), 0);
    }

    // --- Cues ----------------------------------------------------------------

    #[test]
    fn a_turkish_question_produces_a_cue_with_its_segment_as_evidence() {
        let mut c = ctx(180);
        let r = c.ingest(&update(3, 12.0, 3.0, "Bunu ne zaman teslim edebilirsiniz?"));
        let cue = r.cue.expect("cue");
        assert_eq!(cue.kind, CueKind::Question);
        assert_eq!(cue.evidence, vec![3]);
    }

    #[test]
    fn an_english_request_produces_a_request_cue() {
        let mut c = ctx(180);
        let r = c.ingest(&update(
            3,
            12.0,
            3.0,
            "can you send me the retention policy",
        ));
        assert_eq!(r.cue.map(|c| c.kind), Some(CueKind::Request));
    }

    #[test]
    fn a_statement_produces_no_cue() {
        let mut c = ctx(180);
        let r = c.ingest(&update(
            3,
            12.0,
            3.0,
            "We agreed the pilot starts in Ankara.",
        ));
        assert_eq!(r.cue, None);
    }

    #[test]
    fn a_question_split_across_a_vad_boundary_is_joined_with_both_segments_as_evidence() {
        let mut c = ctx(180);
        // "Bunu ne zaman" alone: wh-word ⇒ already a cue on its own? "ne zaman"
        // fires (0.40). Use a split where the first half is silent alone.
        let first = c.ingest(&update(1, 10.0, 2.0, "Raporu akşama kadar"));
        assert_eq!(first.cue, None, "first half alone is a statement fragment");
        // 1.5 s gap, no sentence end on the first half ⇒ continuation.
        let second = c.ingest(&update(2, 13.5, 2.0, "gönderebilir misiniz?"));
        let cue = second.cue.expect("joined text is a request");
        assert_eq!(cue.evidence, vec![1, 2]);
    }

    #[test]
    fn one_question_split_into_three_segments_is_reported_once() {
        // The live VAD closes a segment after 400 ms, so this is the ordinary
        // shape of a spoken sentence, not an edge case. The guard used to ask
        // only whether the predecessor fired *standalone*: the middle segment
        // is silent alone, so it was joined twice and the same question was
        // offered as two cues citing overlapping evidence.
        let mut c = ctx(180);
        let a = c.ingest(&update(1, 10.0, 1.5, "so can"));
        assert_eq!(a.cue, None, "the opening fragment is silent alone");

        let b = c.ingest(&update(2, 12.0, 1.5, "you extend the deadline"));
        let first = b.cue.expect("the joined question fires");
        assert_eq!(first.evidence, vec![1, 2]);

        // A fresh request follows. It fires on its own — the point is *what it
        // cites*: segment 2 was already offered to the user as part of the
        // question above, and must not be handed back inside a second cue.
        let third = c.ingest(&update(3, 14.0, 1.5, "can you also send me the notes"));
        let second = third.cue.expect("the new request fires");
        assert_eq!(
            second.evidence,
            vec![3],
            "an already-reported segment must not be re-cited as evidence"
        );
    }

    #[test]
    fn a_late_chunk_evicted_on_arrival_reports_that_it_was_not_added() {
        let mut c = ctx(180);
        for i in 0..DURABLE_CAP as u64 {
            c.ingest(&update(i, i as f64 * 2.4, 2.0, "x."));
        }
        assert_eq!(c.turns().len(), DURABLE_CAP);
        // A chunk older than everything held, arriving at a full buffer: it is
        // inserted at the front and drained in the same call.
        let r = c.ingest(&update(DURABLE_CAP as u64 + 1, -5.0, 2.0, "too late."));
        assert!(
            !r.added,
            "a turn that did not survive the call was not added"
        );
        assert_eq!(r.cue, None);
        assert_eq!(c.turns().len(), DURABLE_CAP);
    }

    #[test]
    fn a_previous_segment_that_already_fired_is_not_joined_again() {
        let mut c = ctx(180);
        let first = c.ingest(&update(1, 10.0, 2.0, "Hazır mısınız"));
        assert!(first.cue.is_some());
        // Continuation would double-report the same question; the second half
        // must stand alone — and alone it is a statement.
        let second = c.ingest(&update(2, 12.5, 2.0, "hemen başlıyoruz."));
        assert_eq!(second.cue, None);
    }

    #[test]
    fn a_long_gap_or_a_finished_sentence_is_not_a_continuation() {
        let mut c = ctx(180);
        c.ingest(&update(1, 10.0, 2.0, "Raporu akşama kadar"));
        // Gap of 10 s: a new thought, so the second half stands alone — its
        // evidence names only itself.
        let r = c.ingest(&update(2, 22.0, 2.0, "gönderebilir misiniz"));
        assert_eq!(r.cue.map(|c| c.evidence), Some(vec![2]));

        let mut c = ctx(180);
        c.ingest(&update(1, 10.0, 2.0, "Raporu hazırladık."));
        let r = c.ingest(&update(2, 12.5, 2.0, "gönderebilir misiniz"));
        assert_eq!(r.cue.map(|c| c.evidence), Some(vec![2]));
    }

    #[test]
    fn no_cue_from_the_users_own_microphone() {
        // Inert until the producer attributes channels, but the rule is real:
        // the user's own question is not something to offer the user help with.
        let mut c = ctx(180);
        let mut u = update(1, 10.0, 3.0, "Bunu ne zaman teslim edebilirsiniz?");
        u.source = "microphone".to_string();
        let r = c.ingest(&u);
        assert!(r.added, "the turn is still kept — only the cue is withheld");
        assert_eq!(r.cue, None);
        // And a microphone fragment is never used as continuation evidence for
        // the other side's sentence.
        let mut sys = update(2, 14.0, 3.0, "gönderebilir misiniz?");
        sys.source = "system".to_string();
        let r = c.ingest(&sys);
        assert_eq!(r.cue.map(|c| c.evidence), Some(vec![2]));
    }

    // --- The session boundary re-resolves identity ---------------------------

    /// `start_session` and `live()` touch the process-global context, and
    /// cargo runs tests in parallel — the same guard `shortcuts` uses.
    fn serialised() -> std::sync::MutexGuard<'static, ()> {
        static TEST_LOCK: Mutex<()> = Mutex::new(());
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_new_recording_rebuilds_the_context_for_whoever_is_signed_in_now() {
        let _guard = serialised();
        // A context left over from a *different* workspace, holding text.
        let mut stale_identity = AuthContext::local();
        stale_identity.tenant_id = TenantId::new("workspace-that-signed-out");
        let mut stale = LiveContext::new(&stale_identity, 90);
        stale.ingest(&update(1, 0.0, 3.0, "left over from the last tenant"));
        *live() = Some(stale);

        start_session();

        {
            let ctx = live();
            let ctx = ctx.as_ref().expect("the service stays started");
            assert_eq!(
                ctx.tenant_id(),
                &context::current().tenant_id,
                "identity re-resolved"
            );
            assert!(
                ctx.turns().is_empty(),
                "nothing of the previous session survives"
            );
            assert_eq!(ctx.window_secs(), 90, "the user's window setting is kept");
        }
        *live() = None;
    }

    #[test]
    fn a_buffer_with_no_recording_behind_it_is_dropped_on_the_next_status_read() {
        let _guard = serialised();
        let mut ctx = LiveContext::new(&AuthContext::local(), 180);
        ctx.ingest(&update(1, 0.0, 3.0, "a meeting that already ended"));
        *live() = Some(ctx);

        // Still recording: the buffer is the live meeting and must survive.
        discard_if_idle(true);
        assert_eq!(live().as_ref().map(|c| c.turns().len()), Some(1));

        // No recording behind it: released without waiting for a stop event.
        discard_if_idle(false);
        assert_eq!(live().as_ref().map(|c| c.turns().len()), Some(0));
        *live() = None;
    }

    #[test]
    fn discarding_while_stopped_does_not_create_a_context() {
        let _guard = serialised();
        *live() = None;
        discard_if_idle(false);
        assert!(live().is_none());
    }

    #[test]
    fn starting_a_session_while_stopped_does_nothing() {
        let _guard = serialised();
        *live() = None;
        start_session();
        assert!(
            live().is_none(),
            "a stopped service must not spring to life on an event"
        );
    }

    // --- Events are content-free ---------------------------------------------

    #[test]
    fn the_cue_event_carries_ids_and_numbers_but_no_text() {
        let event = CueEvent {
            workspace_id: "local".into(),
            kind: CueKind::Question,
            confidence: 0.9,
            evidence: vec![1, 2],
            sequence_id: 2,
        };
        let value: serde_json::Value = serde_json::to_value(&event).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "confidence",
                "evidence",
                "kind",
                "sequenceId",
                "workspaceId"
            ]
        );
    }

    // --- Status is content-free -----------------------------------------------

    #[test]
    fn the_status_payload_carries_counts_and_no_text() {
        let s = LiveContextStatus {
            subscribed: true,
            window_secs: 180,
            turns: 2,
            window_turns: 1,
            evicted: 0,
        };
        let json = serde_json::to_string(&s).expect("serialises");
        assert!(json.contains("windowTurns"), "{json}");
        // The shape is closed: exactly these keys, so a future field carrying
        // text would have to be added here on purpose.
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "evicted",
                "subscribed",
                "turns",
                "windowSecs",
                "windowTurns"
            ]
        );
    }
}
