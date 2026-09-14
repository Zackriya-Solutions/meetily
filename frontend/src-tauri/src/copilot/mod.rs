//! Live Copilot — the private in-meeting panel (BACKLOG **EPIC I**, ADR-0038).
//!
//! Mityu helps *after* a meeting: source-linked summaries, action items,
//! evidence search, export. This module is the first half of helping *during*
//! one. **I1 is the shell only** — a window, a global shortcut, an honest
//! screen-sharing posture. **I2 adds the live-context service** ([`session`]):
//! a rolling window and a per-session buffer over the existing
//! `transcript-update` stream, plus a deterministic cue when someone asks or
//! requests something ([`cue`]). **I3a adds the first model call**
//! ([`insight`]): the window plus a [`crate::modes::Mode`] become one grounded
//! answer, redacted at the prompt boundary and gated on an egress policy that
//! is off by default (ADR-0040). **I3b puts it on screen** — four actions, the
//! Art. 50 disclosure and marking, and a distinct rendering for every refusal.
//! Nothing any of it produces is written to disk: "Pin to notes" is *not* in
//! this epic's shipped surface and is filed as I3c for the reasons in
//! ADR-0041.
//!
//! ## The four invariants, and where each is actually enforced
//!
//! 1. **Private, not covert.** [`policy`] decides what the OS will really do
//!    about screen capture *and returns the sentence to show about it*, so the
//!    UI cannot promise more than the platform delivers — Windows enforces
//!    capture exclusion, macOS 15+ ignores it for ScreenCaptureKit, Linux has
//!    no mechanism. [`window`] never hides the app from the taskbar or dock,
//!    never renames the process, and there is no keyboard hook anywhere in this
//!    module. "Undetectable" is not a thing Mityu does, and a test in
//!    [`policy`] fails if that word reaches the user-facing strings.
//! 2. **The copilot never captures.** Nothing here starts, stops or reads
//!    audio, and no file in `audio/` is touched by this epic. [`session`] is a
//!    *listener* on the event the pipeline already emits, and only while a
//!    recording session — one whose consent ticket was already consumed by
//!    `recording_consent` — is running. Its buffer is rebuilt at
//!    `recording-started` (re-resolving the tenant from `AuthContext`) and
//!    cleared on **both** stop paths: `recording-stopped`, which the ordinary
//!    Stop emits, and `recording-stop-complete`, which only the tray emits.
//!    [`session::discard_if_idle`] is a second, independent bound for the case
//!    where neither event arrives.
//! 3. **Draft-only, and nothing a model made is persisted.** The only two
//!    things written to disk are the user's settings and the panel's last
//!    position ([`store`]). The live context is memory for one session, and
//!    its status payload carries counts, never text. An insight is a draft that
//!    exists only in the panel that asked for it: [`insight`] writes nothing,
//!    and neither does [`commands::copilot_request_insight`]. Persisting one
//!    ("Pin to notes") turned out not to be buildable on the existing summary
//!    repository without destroying user-authored content, so it is deferred
//!    to I3c rather than shipped — ADR-0041 records the three reasons.
//! 4. **Dormant by default.** [`config::CopilotConfig::enabled`] is `false`, and
//!    with it off [`shortcuts::apply`] registers nothing, [`window::open`]
//!    refuses and [`session::apply_config`] subscribes to nothing
//!    ([`session::should_subscribe`] is the one decision). The flag may default *on* only after the I9 gate (A5 GO plus the
//!    live smoke) — mechanics are quality-independent, claims are not.
//!
//! ## Layout
//!
//! - [`config`] — settings, the [`config::KeybindAction`] set, geometry.
//! - [`policy`] — the screen-capture verdict per platform, and its wording.
//! - [`keybind`] — shortcut grammar: parsing, validation, canonical form.
//! - [`store`] — the `copilot.json` settings/geometry file.
//! - [`window`] — the panel window's lifecycle.
//! - [`shortcuts`] — registration with the OS.
//! - [`session`] — the live context: window, buffer, cue events (I2).
//! - [`cue`] — the deterministic TR/EN question and request detector (I2).
//! - [`insight`] — the grounded live insight: prompt, redaction boundary,
//!   egress policy, grounding (I3a).
//! - [`commands`] — the Tauri surface (registered in `lib.rs`).

pub mod commands;
pub mod config;
pub mod cue;
pub mod insight;
pub mod keybind;
pub mod policy;
pub mod session;
pub mod shortcuts;
pub mod store;
pub mod window;

pub use config::{CopilotConfig, KeybindAction, Keybinds, PanelGeometry};
pub use policy::{CaptureProtection, ProtectionVerdict};
pub use window::COPILOT_WINDOW_LABEL;

use tauri::{AppHandle, Runtime};

/// Startup hook, called from `lib.rs`'s `setup`.
///
/// Reads the stored settings and registers the global shortcut and the
/// live-context listener **only** if the copilot is enabled. With the default
/// settings this does nothing observable: no window, no shortcut, no listener —
/// the build behaves exactly as one compiled without this module (the `sync/`,
/// `agents/` and `inference/` dormant-seam precedent).
pub fn init<R: Runtime>(app: &AppHandle<R>) {
    let config = store::load_config(app);
    if !config.enabled {
        log::debug!("copilot: disabled; no shortcut registered, no panel created, no listener");
        return;
    }
    if let Err(e) = shortcuts::apply_config(app, &config) {
        log::warn!("copilot: shortcuts could not be applied at startup: {e}");
    }
    session::apply_config(app, &config);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seam's whole promise in one assertion: a fresh install registers
    /// nothing, because the config it reads is the disabled one.
    #[test]
    fn a_fresh_install_has_the_copilot_off() {
        assert!(!CopilotConfig::default().enabled);
    }
}
