# Architecture

## Product Summary

MeetFree is a local-first Tauri desktop application for:

- capturing microphone and optional system audio
- transcribing speech locally with Parakeet or Whisper
- storing meetings, transcripts, summaries, and settings locally
- generating summaries with local models first, with optional external providers

The product of record in this fork is the desktop application under `desktop/`.

## Active Codebase Map

Approximate source file counts in the active app:

- `desktop/src/app`: 11
- `desktop/src/components`: 94
- `desktop/src/contexts`: 7
- `desktop/src/hooks`: 22
- `desktop/src/services`: 6
- `desktop/src-tauri/src/audio`: 56
- `desktop/src-tauri/src/database`: 11
- `desktop/src-tauri/src/summary`: 18
- `desktop/src-tauri/src/whisper_engine`: 7
- `desktop/src-tauri/src/parakeet_engine`: 4

This is a capable codebase, but it is already large relative to the current product definition.

## Runtime Layers

### 1. App Shell and Navigation

The React app shell lives in:

- `desktop/src/app/layout.tsx`
- `desktop/src/app/page.tsx`
- `desktop/src/app/meeting-details/`
- `desktop/src/app/settings/`

The shell owns:

- sidebar and meeting navigation
- onboarding gating
- import dialog mounting
- update notifications
- global providers

### 2. Frontend State Layer

The main frontend state owners are:

- `desktop/src/contexts/RecordingStateContext.tsx`
- `desktop/src/contexts/TranscriptContext.tsx`
- `desktop/src/contexts/ConfigContext.tsx`
- `desktop/src/contexts/OnboardingContext.tsx`
- `desktop/src/components/Sidebar/SidebarProvider.tsx`

These contexts coordinate most UI behavior. The current architecture relies heavily on:

- Tauri events
- React context state
- `sessionStorage`
- IndexedDB crash recovery state

### 3. Tauri Command and Event Boundary

The desktop/native boundary is centered in:

- `desktop/src-tauri/src/lib.rs`

This file registers startup behavior, system tray integration, notifications, database setup, model initialization, and a very large Tauri command surface. Functionally it works, but it is also the main concentration point for product complexity.

### 4. Native Services

The major Rust subsystems are:

- audio capture, processing, saving, recovery: `desktop/src-tauri/src/audio/`
- transcription engines: `desktop/src-tauri/src/whisper_engine/`, `desktop/src-tauri/src/parakeet_engine/`
- database access and repositories: `desktop/src-tauri/src/database/`
- summaries, templates, and summary contract: `desktop/src-tauri/src/summary/`
- provider integrations: `desktop/src-tauri/src/openai/`, `desktop/src-tauri/src/openrouter/`, `desktop/src-tauri/src/anthropic/`, `desktop/src-tauri/src/groq/`, `desktop/src-tauri/src/ollama/`
- onboarding and app status: `desktop/src-tauri/src/onboarding.rs`, `desktop/src-tauri/src/state.rs`

### 5. Storage Layers

The app currently uses multiple storage layers for different purposes:

- SQLite: meetings, transcripts, summaries, settings
- app data filesystem: recordings, models, bundled templates
- IndexedDB: crash recovery for in-progress transcripts
- Tauri store: onboarding status and some lightweight app state

## Core Product Flows

### Startup and Onboarding

Startup flow:

1. Tauri initializes plugins, tray, notifications, model directories, model managers, and database startup logic.
2. React checks onboarding state and shows either onboarding or the main app shell.
3. Onboarding downloads Parakeet and a recommended built-in summary model, initializes the database, and saves default model configuration.

Key files:

- `desktop/src-tauri/src/lib.rs`
- `desktop/src-tauri/src/onboarding.rs`
- `desktop/src/components/onboarding/`
- `desktop/src/contexts/OnboardingContext.tsx`

### Recording and Live Transcription

Recording flow:

1. UI requests start from `useRecordingStart`.
2. Tauri resolves devices and validates transcription model readiness.
3. `RecordingManager` starts capture and transcription workers.
4. Transcript events stream back to the React app.
5. Transcript state is mirrored into IndexedDB for crash recovery.

Key files:

- `desktop/src/hooks/useRecordingStart.ts`
- `desktop/src-tauri/src/audio/recording_commands.rs`
- `desktop/src-tauri/src/audio/recording_manager.rs`
- `desktop/src/contexts/TranscriptContext.tsx`

### Stop, Save, and Recovery

Current stop flow:

1. Rust stops recording and emits metadata events.
2. React waits for transcription completion, flushes transcript buffers, saves the meeting into SQLite, updates IndexedDB recovery state, and navigates to the meeting page.

This is the highest-risk architectural seam in the product because final persistence depends on coordinated frontend timing.

Key files:

- `desktop/src/hooks/useRecordingStop.ts`
- `desktop/src/services/storageService.ts`
- `desktop/src/services/indexedDBService.ts`
- `desktop/src-tauri/src/audio/incremental_saver.rs`

### Summary Generation

Summary flow:

1. Meeting details page loads transcript data and any stored summary payload.
2. User selects provider/model/template.
3. Tauri saves transcript chunks for processing state.
4. Background summary generation runs through the summary service.
5. Results are normalized through the `v0.1.0` summary contract and persisted.

Key files:

- `desktop/src/app/meeting-details/page-content.tsx`
- `desktop/src/hooks/meeting-details/useSummaryGeneration.ts`
- `desktop/src-tauri/src/summary/service.rs`
- `desktop/src-tauri/src/summary/processor.rs`
- `desktop/src-tauri/src/summary/contract.rs`
- `desktop/src/contracts/summaryContract.ts`

### Import and Retranscription

The fork already includes:

- import of existing audio files
- retranscription of saved meetings
- model/language selection for retranscription

Key files:

- `desktop/src/hooks/useImportAudio.ts`
- `desktop/src/components/ImportAudio/`
- `desktop/src/components/MeetingDetails/RetranscribeDialog.tsx`
- `desktop/src-tauri/src/audio/import.rs`
- `desktop/src-tauri/src/audio/retranscription.rs`

## Data Model

The SQLite schema centers on:

- `meetings`
- `transcripts`
- `summary_processes`
- `transcript_chunks`
- `settings`
- `transcript_settings`

Important implications:

- meetings and transcripts are the primary product records
- summary generation is treated as an asynchronous process with status and recovery
- provider configuration and API keys are stored locally as part of settings

## Strengths

- strong local-first foundation
- real desktop-native behavior instead of web-only wrappers
- explicit summary payload contract across Rust and TypeScript
- multiple resilience mechanisms: IndexedDB recovery, audio checkpoints, database WAL cleanup
- solid provider abstraction for summary generation

## Current Architectural Pressure Points

- The stop/save path is frontend-orchestrated, which is fragile for the most important workflow.
- The Tauri command surface is very broad and mostly centralized in one file.
- The settings and provider surface is wider than the current product needs.
- Secrets are stored via the platform keyring where supported, while non-secret provider settings remain in local app storage.
- The codebase already contains more optional capability than a first release should expose by default.

## Bottom Line

The architecture is directionally right for a privacy-oriented desktop meeting product:

- native app
- local persistence
- local transcription
- optional local summaries

The main problem is not lack of capability. The main problem is that the product boundary is looser than the architecture needs it to be for a dependable v0.1.0.
