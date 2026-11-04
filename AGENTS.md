# Agent Guide for Meetily

These instructions apply to AI coding agents working in this repository.

## Project Overview

- Meetily is a desktop application for recording meetings, transcribing audio, and generating summaries.
- The supported app lives in `frontend/`: Next.js/React provides the UI, and the Rust/Tauri core handles native behavior, audio, persistence, transcription, and summary orchestration.
- Local transcription uses Whisper or Parakeet. Summaries use the configured local or cloud provider; do not assume every provider runs locally.
- Frontend code invokes Tauri commands; Rust emits events that update frontend state. Preserve existing command contracts, event payloads, and state ownership.
- `backend/` is an unsupported legacy archive, not a runtime dependency or a place to add new application behavior. Its FastAPI/CORS behavior is not a supported production API.

See [architecture and audio pipeline](docs/architecture.md), including [frontend/native communication](docs/architecture.md#frontend-and-native-communication) and [model management](docs/architecture.md#transcription-and-model-management).

## Development Commands

Use pnpm 9.15.9. Run these commands from `frontend/`:

| Command | Purpose |
| --- | --- |
| `pnpm install --frozen-lockfile` | Install the committed dependency set |
| `pnpm run tauri:dev` | Run the full desktop app |
| `pnpm run tauri:build` | Build the desktop app for production |

Use the Tauri app for development and validation; a standalone Next.js server does not exercise native commands, audio, or permissions.

See [build and platform setup](docs/BUILDING.md), [development commands and debugging](docs/BUILDING.md#development-commands-and-debugging), and [GPU acceleration](docs/GPU_ACCELERATION.md) for prerequisites and platform-specific options. Prefer the package scripts over clean-run wrappers, which can remove dependencies and generated output.

## Working in This Repository

- Read the relevant source before editing and follow existing patterns. Keep changes focused on the requested problem.
- Implement native behavior through existing Rust services and Tauri commands/events. Check callers and listeners when changing an interface.
- Check source and configuration before repeating technical examples or assumptions from documentation.

## Engineering Guardrails

- Preserve both audio outputs: recordings receive mixed audio, while transcription receives VAD-selected speech. A change to one path must not silently break the other.
- Preserve sample-rate conversions and the channel-drain/flush behavior during recording shutdown. Read the current pipeline and VAD implementation rather than assuming timing constants.
- Follow existing shared-state synchronization and async boundaries. Do not block async tasks or audio callbacks with expensive work.
- Use `perf_debug!()` and `perf_trace!()` for hot-path logging that should disappear in release builds. Keep pipeline and worker diagnostics aggregated rather than adding per-chunk logs.
- Reuse `AudioBufferPool` where applicable instead of adding avoidable audio-buffer allocations.
- Use Tauri path APIs for application files; do not hardcode platform-specific paths. Preserve platform permission handling.
- Use "microphone" and "system" consistently for audio-device roles. Keep errors user-friendly and logs diagnostic without exposing private meeting content.

## Source Map

| Area | Start here |
| --- | --- |
| Tauri entry point and command registration | [lib.rs](frontend/src-tauri/src/lib.rs) |
| Meeting persistence | [database/](frontend/src-tauri/src/database/) |
| Recording commands and lifecycle | [recording_commands.rs](frontend/src-tauri/src/audio/recording_commands.rs), [recording_manager.rs](frontend/src-tauri/src/audio/recording_manager.rs) |
| Device discovery and platform configuration | [devices/](frontend/src-tauri/src/audio/devices/) |
| Audio capture | [capture/](frontend/src-tauri/src/audio/capture/) |
| Mixing, resampling, and VAD | [pipeline.rs](frontend/src-tauri/src/audio/pipeline.rs), [vad.rs](frontend/src-tauri/src/audio/vad.rs) |
| Recording output | [recording_saver.rs](frontend/src-tauri/src/audio/recording_saver.rs) |
| Transcription providers and workers | [transcription/](frontend/src-tauri/src/audio/transcription/) |
| Whisper model loading and acceleration | [whisper_engine/](frontend/src-tauri/src/whisper_engine/) |
| Summary providers and orchestration | [summary/](frontend/src-tauri/src/summary/) |
| Recording UI and meeting state | [page.tsx](frontend/src/app/page.tsx), [SidebarProvider.tsx](frontend/src/components/Sidebar/SidebarProvider.tsx) |

## Verification and Privacy

- Exercise the changed path in the Tauri app when application behavior changes. Check relevant platform permissions, devices, and models for native/audio work.
- Use existing frontend tests under `frontend/tests/` and Rust tests for the affected subsystem. Inspect their runner and configuration before choosing commands; do not assume a `pnpm test` script exists.
- For documentation-only changes, check links and any changed commands or configuration rather than building the app unnecessarily.
- Report exactly what ran, observed results, the platform, and what remains untested. Never invent build, test, or hardware results.
- Use synthetic or redacted reproduction data. Never share private recordings, transcripts, credentials, or unredacted logs and attachments.

## Issues, PRs, and Authorization

- When preparing an issue or PR, read and follow [CONTRIBUTING.md](CONTRIBUTING.md) for duplicate searches, coordination with existing work, branch targets, review expectations, and submission requirements. Local-only edits and investigations do not require a GitHub duplicate search.
- Do not commit, push, create issues or PRs, or post comments without explicit user authorization for that action. Permission to edit locally is not permission to publish.
- Before requesting publication authorization, provide related issue/PR links, a concise change explanation, verification results, and known limitations.
