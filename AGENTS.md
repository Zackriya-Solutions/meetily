# Meetily — Agent Guidance

These instructions apply to AI coding agents working in this repository.

## Project

**Meetily** — Privacy-first AI meeting assistant. Tauri 2.x desktop app (Rust + Next.js 14 + React 18 + Tailwind CSS). All local: no cloud dependency.

- Meetily is a desktop application for recording meetings, transcribing audio, and generating summaries.
- The supported app lives in `frontend/`: Next.js/React provides the UI, and the Rust/Tauri core handles native behavior, audio, persistence, transcription, and summary orchestration.
- Local transcription uses Whisper or Parakeet. Summaries use the configured local or cloud provider; do not assume every provider runs locally.
- Frontend code invokes Tauri commands; Rust emits events that update frontend state. Preserve existing command contracts, event payloads, and state ownership.
- `backend/` is an unsupported legacy archive, not a runtime dependency or a place to add new application behavior. Its FastAPI/CORS behavior is not a supported production API.

See [architecture and audio pipeline](docs/architecture.md), including [frontend/native communication](docs/architecture.md#frontend-and-native-communication) and [model management](docs/architecture.md#transcription-and-model-management).

## Workspace & Entrypoints

- Cargo workspace: `frontend/src-tauri` (main app, lib crate `app_lib`) + `llama-helper` (sidecar binary for LLM inference).
- Rust entry: `frontend/src-tauri/src/main.rs` → `app_lib::run()` (defined in `lib.rs`). **Do NOT edit `main.rs`.**
- Frontend entry: `frontend/src/app/page.tsx` (Next.js App Router).
- Tauri config: `frontend/src-tauri/tauri.conf.json` — version, CSP, bundle targets, updater.
- Version source of truth: `tauri.conf.json`, **not** `package.json` or `Cargo.toml`.

## Development Commands

Use pnpm (NOT npm/yarn); upstream pins pnpm 9.15.9. Run these commands from `frontend/`:

Before running desktop dev/build commands, complete the [first-run sidecar setup](docs/BUILDING.md#first-run-sidecar-setup). Installing frontend dependencies alone does not stage `llama-helper`.

```bash
pnpm install --frozen-lockfile  # Install the committed dependency set
pnpm run dev                    # Next.js dev server only (port 3118)
pnpm run tauri:dev              # Full Tauri dev (auto-detects GPU via scripts/tauri-auto.js)
pnpm run tauri:build            # Production build
pnpm run tauri:dev:cpu          # Tauri dev without GPU acceleration
pnpm run tauri:dev:metal        # macOS Metal
pnpm run tauri:dev:cuda         # NVIDIA CUDA
pnpm run tauri:dev:vulkan       # AMD/Intel Vulkan
pnpm run lint                   # Next.js lint (ESLint, no config file in repo)

# Helper scripts (macOS/Linux):
./clean_run.sh [info|debug|trace]   # Full clean + install + dev (removes node_modules!)
./clean_build.sh [info|debug|trace] # Full clean + production build
./build-gpu.sh                      # Auto-detect GPU + production build
./dev-gpu.sh                        # Auto-detect GPU + dev mode

# Cargo from workspace root:
cargo build -p meetily              # Rust only (no frontend)
cargo build -p llama-helper         # Sidecar binary only
cargo check                         # Rust type-check only
```

Use the Tauri app for development and validation; a standalone Next.js server does not exercise native commands, audio, or permissions. After sidecar preparation, prefer the package scripts over clean-run wrappers, which can remove dependencies and generated output.

Rust features for GPU: `metal`, `coreml`, `cuda`, `vulkan`, `hipblas`, `openblas`, `openmp`. macOS defaults to Metal+CoreML; Windows/Linux default to CPU.

See [build and platform setup](docs/BUILDING.md), [development commands and debugging](docs/BUILDING.md#development-commands-and-debugging), and [GPU acceleration](docs/GPU_ACCELERATION.md) for prerequisites and platform-specific options.

## Testing

```bash
# Rust tests
cargo test -p meetily               # Unit/integration tests for the Tauri backend

# Frontend tests (uses bun:test — NOT Jest or Vitest)
bun test                            # Run from frontend/ directory
# Tests located in: frontend/tests/lib/*.test.ts/.mjs/.js
```

No test runner is configured in `package.json`. Tests use `bun:test` independently. Inspect the runner and configuration before choosing commands; do not assume a `pnpm test` script exists.

## Key Architecture

- **Communication**: Tauri commands (frontend → Rust via `invoke()`) + events (Rust → frontend via `app.emit()`).
- **State**: React context (`SidebarProvider`) + Rust atomics (`RecordingState`).
- **Audio pipeline** (`audio/pipeline.rs`): dual path — recording (mixed mic+system) and transcription (VAD-filtered → Whisper).
- **Two audio directories**: `audio/` (primary, modularized) and `audio_v2/` (experimental, 9 files — `lib.rs` is the entrypoint). Prefer `audio/` for new work unless v2 migration is explicit.
- **Database**: SQLite via `sqlx`. Schema is embedded in `database/` module.
- **Whisper**: `whisper_engine/whisper_engine.rs`. Models: dev in `frontend/models/`, prod in app data dir (`$APPDATA/Meetily/models/` or `~/Library/Application Support/Meetily/models/`).
- **llama-helper**: Built separately, placed in `frontend/src-tauri/binaries/`, declared as Tauri `externalBin` sidecar.
- **FFmpeg**: Downloaded and bundled at build time (`build/ffmpeg.rs`). Cached in `binaries/` directory.
- **Naming convention**: Audio devices are "microphone" and "system" (never "input"/"output").

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

## Constraints & Gotchas

1. **Next.js `output: 'export'`** (static export) — no SSR, no API routes. `reactStrictMode: false` (BlockNote/TipTap compatibility).
2. **ProseMirror version-pinned** via pnpm overrides (`prosemirror-model@1.25.3`, etc.) — do NOT bump these independently. Webpack aliases keep BlockNote/TipTap on the same singleton (`next.config.js`).
3. **CSP** in `tauri.conf.json` allows `connect-src` to `localhost:11434` (Ollama), `localhost:5167`, `localhost:8178`. Add new LLM endpoints here.
4. **macOS entitlements** (`entitlements.plist`): microphone, audio output, screen capture required.
5. **Linux build dependencies**: `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `libopenblas-dev`, `cmake`, `llvm`. WebKit version is pinned in CI (`2.44.0-2`).
6. **App identifier**: `com.meetily.ai` — used in path resolution, do not change.
7. **Crate name vs lib name**: Package is `meetily`, lib crate is `app_lib`. Import as `use app_lib::...` in Rust code.
8. **Logging macros**: `perf_debug!()` and `perf_trace!()` compile to no-ops in release builds. Use `log::debug!()` for persistent debug output.
9. **Backend directory** `backend/` is archived Python/FastAPI legacy. Do not create or modify files there. All features go through Tauri Rust core.

## Source Map

| Area | Start here |
| --- | --- |
| Tauri entry point and command registration | [lib.rs](frontend/src-tauri/src/lib.rs) |
| Meeting persistence | [database/](frontend/src-tauri/src/database/) |
| Recording commands and lifecycle | [recording_commands.rs](frontend/src-tauri/src/audio/recording_commands.rs), [recording_manager.rs](frontend/src-tauri/src/audio/recording_manager.rs) |
| Device discovery and platform configuration | [devices/](frontend/src-tauri/src/audio/devices/) |
| Audio permissions | [permissions.rs](frontend/src-tauri/src/audio/permissions.rs) |
| Audio capture | [capture/](frontend/src-tauri/src/audio/capture/) |
| Mixing, resampling, and VAD | [pipeline.rs](frontend/src-tauri/src/audio/pipeline.rs), [vad.rs](frontend/src-tauri/src/audio/vad.rs) |
| Recording output | [recording_saver.rs](frontend/src-tauri/src/audio/recording_saver.rs) |
| Transcription providers and workers | [transcription/](frontend/src-tauri/src/audio/transcription/) |
| Whisper model loading and acceleration | [whisper_engine/](frontend/src-tauri/src/whisper_engine/) |
| Parakeet model loading and inference | [parakeet_engine/](frontend/src-tauri/src/parakeet_engine/) |
| Summary providers and orchestration | [summary/](frontend/src-tauri/src/summary/) |
| Recording UI and shared frontend state | [page.tsx](frontend/src/app/page.tsx), [SidebarProvider.tsx](frontend/src/components/Sidebar/SidebarProvider.tsx), [RecordingStateContext.tsx](frontend/src/contexts/RecordingStateContext.tsx) |

## Verification and Privacy

- Exercise the changed path in the Tauri app when application behavior changes. Check relevant platform permissions, devices, and models for native/audio work.
- Use existing frontend tests under `frontend/tests/` and Rust tests for the affected subsystem. Inspect their runner and configuration before choosing commands; do not assume a `pnpm test` script exists.
- For documentation-only changes, check links and any changed commands or configuration rather than building the app unnecessarily.
- Report exactly what ran, observed results, the platform, and what remains untested. Never invent build, test, or hardware results.
- Use synthetic or redacted reproduction data. Never share private recordings, transcripts, credentials, or unredacted logs and attachments.

## Git Workflow

- `main` — production; `devtest` — integration/testing; feature branches from `devtest`.
- PRs target `devtest`, not `main`.
- Commit format: conventional commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`).

## Issues, PRs, and Authorization

- When preparing an issue or PR, read and follow [CONTRIBUTING.md](CONTRIBUTING.md) for duplicate searches, coordination with existing work, branch targets, review expectations, and submission requirements. Local-only edits and investigations do not require a GitHub duplicate search.
- Do not commit, push, create issues or PRs, or post comments without explicit user authorization for that action. Permission to edit locally is not permission to publish.
- Before requesting publication authorization, provide related issue/PR links, a concise change explanation, verification results, and known limitations.

## CI Artifacts

- macOS: `.dmg` + `.app.tar.gz` + `.sig`
- Windows: `.msi` + `.msi.sig` + `.exe` (NSIS) + `.exe.sig`
- Linux (22.04): `.deb` — Linux (24.04): `.AppImage` + `.rpm`
- Updater signatures always generated. Code signing optional (macOS Apple ID, Windows DigiCert).
