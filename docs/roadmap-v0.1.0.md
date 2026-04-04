# Roadmap to v0.1.0

> Archived document for historical context only. Active release planning and technical behavior live in the `v0.2.0` docs:
> - `docs/technical-design-v0.2.0.md`
> - `docs/execution-plan-v0.2.0.md`

## Release Definition

v0.1.0 should be the first dependable release of this fork, not the first release with the most features.

Success means a new user can:

1. install the desktop app
2. complete onboarding
3. record a meeting successfully
4. get a saved transcript without manual recovery
5. open and search past meetings
6. generate a summary with a clearly available model

## Must-Have User-Facing Features

### Onboarding

- first-run onboarding
- database initialization
- transcription model setup
- summary model readiness or explicit summary-not-ready state
- permission guidance, especially on macOS

### Recording

- start/stop recording
- microphone capture
- optional system audio capture where supported
- pause/resume
- device selection
- clear recording state feedback

### Transcription

- local transcription with Parakeet as the default path
- transcript streaming during recording
- accurate saved transcript ordering
- searchable transcript history

### Meeting Library

- sidebar meeting history
- rename/delete meetings
- meeting detail view
- open meeting folder

### Summary

- summary generation from saved transcripts
- built-in local summary path
- optional provider-based summary path
- summary templates
- summary regeneration and save

### Reliability

- crash/interruption recovery
- no silent data loss on stop
- update check and install path
- clear failure states for missing models, permissions, and provider config

### Settings

- recording settings
- transcription settings
- summary settings
- storage locations
- privacy-relevant provider configuration

## Must-Fix Engineering Work Before Release

### 1. Move Final Save Ownership Out Of The Frontend

Current behavior depends on the frontend coordinating:

- transcription completion polling
- transcript buffer flushing
- session storage handoff
- SQLite save timing

That is too fragile for the core workflow.

### 2. Fix The Failing Rust Tests

Current `cargo test -p meetfree --lib` is not green. At minimum:

- stabilize floating-point duration assertions
- fix or redesign the VAD large-file segmentation test

Release CI should be green on the deterministic test suite.

### 3. Decide The Summary Readiness Story

If onboarding completes before the built-in summary model is available, the app must clearly say so and behave accordingly.

### 4. Keep Provider Secrets In Secure Storage

Provider API keys should stay in OS-backed credential storage for v0.1.0.

- validate the keyring-backed path on supported platforms
- keep non-secret provider settings in local app storage
- keep release messaging explicit about what is secret storage vs local config

### 5. Rationalize Beta Features

Import/retranscribe must either:

- be supported, documented, and tested, or
- be hidden by default for v0.1.0

### 6. Reduce Default Complexity

The primary UX should bias toward:

- Parakeet
- Built-in AI
- local storage

Everything else should be secondary.

### 7. Align Documentation

The repo must ship with documentation that matches the code:

- architecture
- technical design
- release roadmap
- build instructions
- privacy behavior

## Nice-To-Have If Time Allows

- import audio polished enough to graduate from beta
- retranscription quality tuning
- better device reconnection UX
- summary provider capability matrix in settings
- explicit health/diagnostics panel

## Explicitly Deferred

- speaker diarization
- PDF export
- DOCX export
- cloud sync
- collaboration
- calendar integrations
- team reporting

## Suggested Execution Order

### Phase 1: Stabilize Core Workflow

- backend-owned recording finalization
- green deterministic tests
- confirm meeting save and recovery path

### Phase 2: Simplify Product Surface

- tighten onboarding
- narrow default providers
- decide beta feature exposure

### Phase 3: Release Hardening

- docs
- privacy disclosures
- update flow
- packaging validation

## Recommended v0.1.0 Scope Statement

MeetFree v0.1.0 should ship as:

"A local-first desktop app for recording meetings, generating local transcripts, keeping a searchable meeting history, and producing summaries with local models by default."

That is a coherent first release. The codebase can support more than that, but the release should not promise more than it can support reliably.
