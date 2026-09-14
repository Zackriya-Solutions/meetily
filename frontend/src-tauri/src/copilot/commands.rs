//! The copilot's Tauri command surface. Registered in `lib.rs`.
//!
//! TS bindings live in `frontend/src/services/copilotService.ts` — the only
//! place these names appear on the renderer side.
//!
//! ```ts
//! const status = await invoke<CopilotStatus>('copilot_get_status');
//! const status = await invoke<CopilotStatus>('copilot_set_config', { config });
//! await invoke('copilot_toggle_panel');
//! await invoke('copilot_close_panel');
//! await invoke('copilot_focus_main_window');
//! await invoke('copilot_request_insight', { action: 'recap' });
//! await invoke('copilot_cancel_insight');
//! ```
//!
//! Most commands here are local, synchronous work over a settings file and a
//! window handle: no network, no database, no model, no transcript.
//! [`copilot_request_insight`] is the exception and the reason this note is no
//! longer a blanket claim — it reads settings from SQLite and calls a model
//! (I3b). It stays thin in the same way `ask::commands` does: it resolves
//! identity, config and paths, then delegates to [`super::insight`], where
//! every rule is testable without a running app.

use super::config::{CopilotConfig, KeybindAction, MAX_LIVE_WINDOW_SECS, MIN_LIVE_WINDOW_SECS};
use super::insight::{self, InsightError, LiveInsightOutcome};
use super::policy::ProtectionVerdict;
use super::session::LiveContextStatus;
use super::{keybind, session, shortcuts, store, window};
use crate::modes::{default_mode, LiveAction};
use crate::state::AppState;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Manager, Runtime};
use tokio_util::sync::CancellationToken;

/// One row of the Settings shortcut list: what the binding is, whether it is
/// actually registered, and — when it is not — why.
///
/// Serialize only — see [`ProtectionVerdict`] for why a struct holding
/// `&'static str` cannot derive `Deserialize`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutInfo {
    /// Wire token of the action (`togglePanel`, `ask`, `captureScreen`).
    pub action: &'static str,
    pub label: &'static str,
    /// The user's binding, canonicalised.
    pub keybind: String,
    /// True only when the operating system is actually holding this combination
    /// for Mityu — never merely because the settings say it should.
    pub registered: bool,
    /// Why it is not registered, when it is not. Owned rather than `&'static`
    /// because the interesting cases come from the OS at runtime.
    pub unavailable_reason: Option<String>,
}

/// Shown while the copilot is on and shortcuts are enabled but the OS has not
/// been asked yet — the brief window between enabling and [`shortcuts::apply`].
const NOT_ASKED_YET: &str = "Not registered — Mityu has not asked the system yet.";

/// Decide one Settings row.
///
/// Pure, and separated from [`build_status`] for one reason: this is where the
/// UI previously lied. The row used to be derived from the stored config, so it
/// printed "· active" over a combination the OS had refused — and the refusal is
/// not hypothetical, Microsoft Teams holds `Ctrl+Shift+M` for in-call mute,
/// which is this feature's own default and the very meeting it is for.
///
/// Two sources, each for what it is actually good for:
///
/// - `held_by_os` — [`shortcuts::is_held`], the plugin's live map. **The
///   authority for the yes/no**, because it cannot go stale: anything that
///   clears the shortcuts empties that map too.
/// - `registration` — what [`shortcuts::apply_config`] recorded the OS saying.
///   **The authority for the reason**, which the boolean cannot carry.
///
/// `held_by_os: None` means the question could not be asked, and falls back to
/// the record rather than being read as a yes.
fn shortcut_row(
    action: KeybindAction,
    config: &CopilotConfig,
    registration: Option<shortcuts::Registration>,
    held_by_os: Option<bool>,
) -> ShortcutInfo {
    let keybind = config.keybinds.get(action).to_string();
    let (registered, unavailable_reason) = if !action.is_registerable() {
        // The most specific truth wins: an action with no implementation says so
        // whatever the switches are set to.
        (false, action.unavailable_reason().map(str::to_string))
    } else if !config.enabled {
        (false, Some("The copilot is off.".to_string()))
    } else if !config.shortcuts_enabled {
        (false, Some("Shortcuts are turned off.".to_string()))
    } else {
        let recorded_held = matches!(registration, Some(shortcuts::Registration::Held));
        // The live answer wins; the record only fills in when we could not ask.
        let held = held_by_os.unwrap_or(recorded_held);
        let reason = match registration {
            _ if held => None,
            Some(shortcuts::Registration::Refused(reason)) => Some(reason),
            // Recorded as held but the OS is not holding it: something cleared
            // the registration behind our back. Say that, rather than inventing
            // a collision that may not have happened.
            Some(shortcuts::Registration::Held) => {
                Some("Registered, then released by the system.".to_string())
            }
            None => Some(NOT_ASKED_YET.to_string()),
        };
        (held, reason)
    };

    ShortcutInfo {
        action: action.wire_name(),
        label: action.label(),
        keybind,
        registered,
        unavailable_reason,
    }
}

/// Decide what a submitted live-window value becomes: accepted, repaired, or
/// refused.
///
/// Pure, and separated from [`copilot_set_config`] for the same reason
/// [`shortcut_row`] is — this is where the command locked the user out.
///
/// Rejecting an out-of-range value outright looked like the keybind rule (tell
/// the user rather than silently change what they typed), but it has a
/// consequence keybinds do not: **no UI exposes this field**, so every Settings
/// switch round-trips whatever is stored. One out-of-range number in
/// `copilot.json` — hand-edited, pre-seeded by an admin, or written by a future
/// build — therefore made *every* save fail, **including switching the copilot
/// off**: the user was locked out of the one control that makes this feature
/// safe, by a field they cannot see or edit.
///
/// So the refusal is now scoped to a value the caller actually changed. A
/// number they did not touch is repaired to the nearest bound and the save
/// proceeds.
fn resolve_live_window(submitted: u32, stored: u32) -> Result<u32, String> {
    let changed = submitted != stored;
    let config = CopilotConfig {
        live_window_secs: submitted,
        ..CopilotConfig::default()
    };
    if changed && !config.live_window_is_valid() {
        return Err(format!(
            "Live context window — must be between {MIN_LIVE_WINDOW_SECS} and {MAX_LIVE_WINDOW_SECS} seconds."
        ));
    }
    Ok(config.live_window_secs_clamped())
}

/// Everything the panel and the Settings tab need in one read.
///
/// Serialize only, because it contains [`ProtectionVerdict`] and
/// [`ShortcutInfo`]. The config *inside* it stays round-trippable: the UI sends
/// a `CopilotConfig` back to `copilot_set_config`, and that type owns its
/// strings.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotStatus {
    pub config: CopilotConfig,
    /// What the OS will do about screen capture here, with the exact wording.
    pub protection: ProtectionVerdict,
    /// Is the panel window open right now?
    pub panel_open: bool,
    /// Is a recording session active? The panel shows live content only while
    /// this is true — the copilot has no capture of its own, it follows a
    /// session the user consented to (ADR-0038 invariant 2).
    pub recording: bool,
    pub shortcuts: Vec<ShortcutInfo>,
    /// The I2 live-context service: subscribed or not, and how much it holds.
    /// Counts only — the payload never carries transcript text.
    pub live_context: LiveContextStatus,
    /// The actions the active mode offers (I3b).
    ///
    /// Sent so the panel renders only what the mode allows, rather than showing
    /// four buttons and letting Rust reject one of them after the click. Until
    /// I4b lets the user pick a mode this is always the default mode's set —
    /// the resolution is one line here, and the panel does not change when it
    /// starts varying.
    pub live_actions: Vec<LiveAction>,
}

/// Read the copilot's state. Local-only and cheap enough to poll.
#[tauri::command]
pub async fn copilot_get_status<R: Runtime>(app: AppHandle<R>) -> CopilotStatus {
    let config = store::load_config(&app);
    build_status(&app, config).await
}

/// Save settings, then make the running app match them: shortcuts are
/// re-applied, the panel's content protection is refreshed, and switching the
/// copilot off closes the panel and releases every shortcut.
///
/// Returns the same shape as `copilot_get_status`, so the UI never has to guess
/// what the backend did with what it sent.
#[tauri::command]
pub async fn copilot_set_config<R: Runtime>(
    app: AppHandle<R>,
    config: CopilotConfig,
) -> Result<CopilotStatus, String> {
    // Validate and canonicalise every binding before anything is stored: a
    // rejected string must not reach the store, or the next startup would try
    // to register it and fail silently.
    let mut config = config;
    for action in KeybindAction::ALL {
        let spec = config.keybinds.get(action);
        let parsed = keybind::parse(spec).map_err(|e| format!("{} — {e}", action.label()))?;
        config.keybinds.set(action, parsed.canonical());
    }
    let stored_window = store::load_config(&app).live_window_secs;
    config.live_window_secs = resolve_live_window(config.live_window_secs, stored_window)?;

    store::save_config(&app, &config)?;

    if config.enabled {
        shortcuts::apply_config(&app, &config)?;
        window::refresh_content_protection(&app, config.content_protection);
        session::apply_config(&app, &config);
    } else {
        // Off means off: no shortcut held, no panel on screen, no listener.
        shortcuts::clear(&app);
        session::stop(&app);
        window::close(&app)?;
    }

    Ok(build_status(&app, config).await)
}

/// Show or hide the panel — the same action as the global shortcut and the tray
/// entry.
#[tauri::command]
pub async fn copilot_toggle_panel<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    window::toggle(&app)
}

/// Close the panel (used by the panel's own close button).
#[tauri::command]
pub async fn copilot_close_panel<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    window::close(&app)
}

/// Bring the main window forward.
///
/// The panel deliberately does not duplicate stopping a recording — that path
/// needs a save location plus the post-processing chain the main window owns,
/// and `stop_recording` already returns a boolean meaning "another caller owns
/// shutdown". Rather than become a second owner of it, the panel sends the user
/// to the window that is one.
#[tauri::command]
pub async fn copilot_focus_main_window<R: Runtime>(app: AppHandle<R>) {
    crate::tray::focus_main_window(&app);
}

async fn build_status<R: Runtime>(app: &AppHandle<R>, config: CopilotConfig) -> CopilotStatus {
    let protection = super::policy::current_verdict();
    let recording = crate::audio::recording_commands::is_recording().await;
    // Second bound on how long a finished meeting stays in memory, independent
    // of the stop events. See `session::discard_if_idle`.
    session::discard_if_idle(recording);

    let shortcuts = KeybindAction::ALL
        .into_iter()
        .map(|action| {
            shortcut_row(
                action,
                &config,
                shortcuts::registration(action),
                shortcuts::is_held(app, config.keybinds.get(action)),
            )
        })
        .collect();

    CopilotStatus {
        panel_open: window::is_open(app),
        recording,
        protection,
        shortcuts,
        live_context: session::status(),
        live_actions: default_mode().live.allowed_actions,
        config,
    }
}

#[cfg(test)]
mod live_window_tests {
    use super::*;
    use crate::copilot::config::DEFAULT_LIVE_WINDOW_SECS;

    #[test]
    fn an_untouched_out_of_range_value_never_blocks_a_save() {
        // The lockout this fixes: `copilot.json` holds 0 (or 10, or u32::MAX),
        // no UI can edit it, and the user flips the enable switch off. Every
        // save used to fail with a message about a field they cannot see.
        for stored in [0, 10, u32::MAX] {
            assert_eq!(
                resolve_live_window(stored, stored),
                Ok(MIN_LIVE_WINDOW_SECS.max(stored.min(MAX_LIVE_WINDOW_SECS))),
                "stored {stored} must be repaired, not refused"
            );
        }
    }

    #[test]
    fn a_value_the_caller_changed_is_still_refused_with_the_bounds() {
        let refused = resolve_live_window(5, DEFAULT_LIVE_WINDOW_SECS)
            .expect_err("a deliberate 5 is out of range");
        assert!(
            refused.contains(&MIN_LIVE_WINDOW_SECS.to_string()),
            "{refused}"
        );
        assert!(
            refused.contains(&MAX_LIVE_WINDOW_SECS.to_string()),
            "{refused}"
        );
        assert!(resolve_live_window(5_000, DEFAULT_LIVE_WINDOW_SECS).is_err());
    }

    #[test]
    fn a_valid_change_is_accepted_verbatim() {
        assert_eq!(resolve_live_window(90, DEFAULT_LIVE_WINDOW_SECS), Ok(90));
        assert_eq!(
            resolve_live_window(MIN_LIVE_WINDOW_SECS, DEFAULT_LIVE_WINDOW_SECS),
            Ok(MIN_LIVE_WINDOW_SECS)
        );
        assert_eq!(
            resolve_live_window(MAX_LIVE_WINDOW_SECS, DEFAULT_LIVE_WINDOW_SECS),
            Ok(MAX_LIVE_WINDOW_SECS)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copilot::shortcuts::Registration;

    fn enabled_config() -> CopilotConfig {
        CopilotConfig {
            enabled: true,
            ..CopilotConfig::default()
        }
    }

    /// The defect this function was extracted to fix: Settings printed "active"
    /// over a shortcut the OS had refused.
    #[test]
    fn a_refused_shortcut_is_not_reported_as_registered() {
        let row = shortcut_row(
            KeybindAction::TogglePanel,
            &enabled_config(),
            Some(Registration::Refused(
                "'CommandOrControl+Shift+M' was refused by the system.".into(),
            )),
            Some(false),
        );
        assert!(!row.registered);
        assert!(row
            .unavailable_reason
            .expect("a refusal explains itself")
            .contains("refused"));
    }

    #[test]
    fn a_held_shortcut_is_reported_as_registered() {
        let row = shortcut_row(
            KeybindAction::TogglePanel,
            &enabled_config(),
            Some(Registration::Held),
            Some(true),
        );
        assert!(row.registered);
        assert_eq!(row.unavailable_reason, None);
    }

    /// The live plugin map is the authority. A record saying "held" cannot
    /// outvote an OS that is no longer holding it — that staleness is the whole
    /// reason the live query exists.
    #[test]
    fn the_live_answer_outranks_a_stale_record() {
        let row = shortcut_row(
            KeybindAction::TogglePanel,
            &enabled_config(),
            Some(Registration::Held),
            Some(false),
        );
        assert!(!row.registered);
        assert_eq!(
            row.unavailable_reason.as_deref(),
            Some("Registered, then released by the system.")
        );
    }

    /// …and in the other direction: the OS holding it settles the question even
    /// if the record never recorded anything.
    #[test]
    fn the_live_answer_also_outranks_a_missing_record() {
        let row = shortcut_row(
            KeybindAction::TogglePanel,
            &enabled_config(),
            None,
            Some(true),
        );
        assert!(row.registered);
        assert_eq!(row.unavailable_reason, None);
    }

    /// When the question could not be asked, fall back to the record — never to
    /// an optimistic "yes".
    #[test]
    fn an_unanswerable_query_falls_back_to_the_record() {
        let refused = shortcut_row(
            KeybindAction::TogglePanel,
            &enabled_config(),
            Some(Registration::Refused("nope".into())),
            None,
        );
        assert!(!refused.registered);

        let held = shortcut_row(
            KeybindAction::TogglePanel,
            &enabled_config(),
            Some(Registration::Held),
            None,
        );
        assert!(held.registered);
    }

    /// Before `apply` has run there is no outcome, and "no outcome" must not
    /// read as success.
    #[test]
    fn an_unasked_shortcut_is_not_reported_as_registered() {
        let row = shortcut_row(KeybindAction::TogglePanel, &enabled_config(), None, None);
        assert!(!row.registered);
        assert_eq!(row.unavailable_reason.as_deref(), Some(NOT_ASKED_YET));
    }

    /// A stale `Held` must never survive the copilot being switched off — and
    /// even if one did, the row refuses it.
    #[test]
    fn a_disabled_copilot_registers_nothing_whatever_the_record_says() {
        let row = shortcut_row(
            KeybindAction::TogglePanel,
            &CopilotConfig::default(),
            Some(Registration::Held),
            Some(true),
        );
        assert!(!row.registered);
        assert_eq!(
            row.unavailable_reason.as_deref(),
            Some("The copilot is off.")
        );
    }

    #[test]
    fn turning_shortcuts_off_is_reported_as_its_own_reason() {
        let config = CopilotConfig {
            enabled: true,
            shortcuts_enabled: false,
            ..CopilotConfig::default()
        };
        let row = shortcut_row(
            KeybindAction::TogglePanel,
            &config,
            Some(Registration::Held),
            Some(true),
        );
        assert!(!row.registered);
        assert_eq!(
            row.unavailable_reason.as_deref(),
            Some("Shortcuts are turned off.")
        );
    }

    /// An action with no implementation says so, whatever the switches and
    /// whatever the OS might be holding.
    #[test]
    fn an_unimplemented_action_says_so_first() {
        for action in [KeybindAction::Ask, KeybindAction::CaptureScreen] {
            let row = shortcut_row(
                action,
                &enabled_config(),
                Some(Registration::Held),
                Some(true),
            );
            assert!(!row.registered, "{} claimed to be registered", row.action);
            assert_eq!(
                row.unavailable_reason.as_deref(),
                action.unavailable_reason()
            );
        }
    }
}

// --- Live insights (I3b) -----------------------------------------------------

/// The in-flight insight's cancellation token.
///
/// One at a time, deliberately: there is one panel and one visible answer, so a
/// second request means the user changed their mind. Starting one cancels the
/// previous rather than racing it to the card.
/// Paired with a generation number, because `CancellationToken` is not `Eq`
/// and "is the token in the slot still mine?" has to be answerable without
/// comparing tokens.
static INSIGHT_CANCEL: Mutex<Option<(u64, CancellationToken)>> = Mutex::new(None);
static INSIGHT_GENERATION: AtomicU64 = AtomicU64::new(0);

fn insight_cancel() -> MutexGuard<'static, Option<(u64, CancellationToken)>> {
    INSIGHT_CANCEL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Why an insight could not be produced, in a shape the panel can branch on.
///
/// A plain string would force the UI to match on prose. `kind` is the stable
/// token; `message` is the sentence to show, written by the error itself so the
/// backend and the panel cannot drift apart on what a refusal means.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightFailure {
    pub kind: &'static str,
    pub message: String,
}

impl From<&InsightError> for InsightFailure {
    fn from(e: &InsightError) -> Self {
        let kind = match e {
            InsightError::ActionNotAllowed(_) => "actionNotAllowed",
            InsightError::NoUsableSource => "noUsableSource",
            InsightError::TenantMismatch => "tenantMismatch",
            InsightError::CloudNotAllowed => "cloudNotAllowed",
            InsightError::Provider(_) => "provider",
            InsightError::Unparsable => "unparsable",
            InsightError::Timeout => "timeout",
            InsightError::Cancelled => "cancelled",
        };
        InsightFailure {
            kind,
            message: e.to_string(),
        }
    }
}

/// The provider a live insight uses, and where it came from.
///
/// The workspace's configured summary provider, because a live insight is the
/// same BYOK arrangement as a summary and silently substituting a *different*
/// model would change the answer's quality without telling anyone. When no
/// settings row exists yet the built-in local model answers — the local-first
/// default, and the reason a fresh install can use this offline.
///
/// Note what this does **not** do: it never downgrades a cloud provider to the
/// local one to slip past the egress policy. If the workspace has a cloud
/// provider configured and has not allowed live insights to leave the device,
/// `insight::complete` refuses and the panel says so. Being told is better than
/// being quietly answered by a model you did not choose.
async fn resolve_live_model(
    pool: &sqlx::SqlitePool,
    ctx: &crate::context::AuthContext,
) -> (String, String) {
    match crate::database::repositories::setting::SettingsRepository::get_model_config(pool, ctx)
        .await
    {
        Ok(Some(setting)) if !setting.provider.trim().is_empty() => {
            (setting.provider, setting.model)
        }
        _ => ("builtin-ai".to_string(), String::new()),
    }
}

/// Ask the copilot for one insight about the last few minutes of speech.
///
/// The whole of I3a's ordering is preserved here and none of it is re-decided:
/// the live window is prepared **synchronously, under the context lock**, the
/// lock is dropped, and only then is a model awaited. Holding that guard across
/// the call would stall every `transcript-update` for its duration
/// (`session::with_context` exists to make that impossible by type).
#[tauri::command]
pub async fn copilot_request_insight<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    action: LiveAction,
) -> Result<LiveInsightOutcome, InsightFailure> {
    let config = store::load_config(&app);
    let pool = state.db_manager.pool();
    let ctx = crate::context::current();
    let mode = default_mode();

    let redaction =
        crate::database::repositories::setting::SettingsRepository::get_redaction_config(
            pool, &ctx,
        )
        .await
        .unwrap_or_default();

    // Prepared under the lock, awaited outside it.
    let prepared = session::with_context(|live| match live {
        Some(live) => insight::prepare(live, &ctx, &mode, action, &redaction),
        // No recording, so no window — the same answer as an empty one, and the
        // model is not called either way.
        None => Ok(None),
    })
    .map_err(|e| InsightFailure::from(&e))?;

    let Some(prepared) = prepared else {
        return Ok(LiveInsightOutcome::NoContext);
    };

    // A second request means the user changed their mind about the first.
    let cancel = CancellationToken::new();
    let generation = INSIGHT_GENERATION.fetch_add(1, Ordering::Relaxed);
    if let Some((_, previous)) = insight_cancel().replace((generation, cancel.clone())) {
        previous.cancel();
    }

    let (provider, model) = resolve_live_model(pool, &ctx).await;
    let app_data_dir = app.path().app_data_dir().ok();

    let outcome = insight::complete(
        pool,
        &ctx,
        app_data_dir.as_ref(),
        &prepared,
        &mode,
        action,
        &provider,
        &model,
        config.allow_cloud_insights,
        insight::DEFAULT_TIMEOUT,
        &cancel,
    )
    .await;

    // Only clear the slot if it is still ours: a newer request has already
    // replaced it, and stealing that token would leave the newer call
    // uncancellable.
    {
        let mut slot = insight_cancel();
        if slot.as_ref().is_some_and(|(g, _)| *g == generation) {
            *slot = None;
        }
    }

    match outcome {
        // Shape only. Claim text is conversation content and never reaches a log.
        Ok(o) => {
            let shape = match &o {
                LiveInsightOutcome::NoContext => "no_context".to_string(),
                LiveInsightOutcome::Answered {
                    claims, dropped, ..
                } => format!("answered kept={} dropped={}", claims.len(), dropped.len()),
                LiveInsightOutcome::Refused { dropped, .. } => {
                    format!("refused dropped={}", dropped.len())
                }
            };
            log::info!(
                "copilot_request_insight completed (action={}, {shape})",
                action.wire_name()
            );
            Ok(o)
        }
        Err(e) => {
            let failure = InsightFailure::from(&e);
            log::warn!(
                "copilot_request_insight failed (action={}, kind={})",
                action.wire_name(),
                failure.kind
            );
            Err(failure)
        }
    }
}

/// Abandon the in-flight insight, if there is one.
///
/// Idempotent and always safe: with nothing running this does nothing, and a
/// cancelled call resolves as `InsightError::Cancelled` rather than an error the
/// user has to read.
#[tauri::command]
pub async fn copilot_cancel_insight() {
    if let Some((_, token)) = insight_cancel().take() {
        token.cancel();
        log::debug!("copilot: in-flight insight cancelled");
    }
}
