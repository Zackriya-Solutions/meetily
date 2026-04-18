# Outbound Network Calls Audit

This document summarizes the outbound network calls identified in the current Meetily repository after telemetry removal.

## Scope

This audit covers:

- Direct outbound HTTP(S) endpoints hardcoded in the repository
- Localhost network calls between app components
- Optional third-party calls triggered by configuration or user action
- Update checks, downloads, and external-link opens

This audit does **not** claim to be a packet capture. It is a code-level inventory of observed egress paths.

## Executive Summary

Meetily is **not network-silent**, but the active application code no longer includes built-in telemetry.

The repository currently contains:

- **Automatic update checks** to GitHub release metadata
- **Local service calls** to the bundled/local backend and local model servers
- **Optional cloud AI provider calls** when those providers are configured
- **Model and dependency downloads**
- **User-initiated external link opens**

## 1. Telemetry

No built-in telemetry endpoints are present in the active frontend or Tauri runtime.

## 2. Auto-update traffic

### Tauri updater

- **Endpoint**:
  - `https://github.com/Zackriya-Solutions/meeting-minutes/releases/latest/download/latest.json`
- **Purpose**: Check for application updates
- **Source files**:
  - `frontend/src-tauri/tauri.conf.json`
  - `frontend/src/services/updateService.ts`
  - `frontend/src/hooks/useUpdateCheck.ts`
  - `frontend/src/components/UpdateCheckProvider.tsx`
- **Trigger**:
  - Automatically on app mount
  - Manually from tray / update UI
- **Important notes**:
  - Update checks are enabled through the Tauri updater plugin.
  - The configured update endpoint still points to the original upstream release feed, not a fork-specific release feed.

## 3. Localhost / local service traffic

These are network calls, but they stay on the local machine unless the configured local service is itself remote.

### Local backend API

- **Endpoint**: `http://localhost:5167`
- **Purpose**:
  - Meeting retrieval
  - Meeting persistence
  - Legacy/alternate backend endpoints
  - Backend availability tests
- **Source files**:
  - `frontend/src-tauri/src/api/api.rs`
  - `frontend/src/components/Sidebar/SidebarProvider.tsx`
  - `frontend/src-tauri/tauri.conf.json`

### Local transcription stream / whisper server

- **Endpoint**: `http://127.0.0.1:8178` / `http://127.0.0.1:8178/stream`
- **Purpose**: Local transcription service path
- **Source files**:
  - `frontend/src/components/Sidebar/SidebarProvider.tsx`
  - `frontend/src-tauri/tauri.conf.json`

### Local Ollama

- **Endpoints**:
  - `http://localhost:11434`
  - `http://127.0.0.1:11434`
  - Custom Ollama endpoint if configured
- **Purpose**:
  - List models
  - Fetch metadata
  - Run local summary generation
- **Source files**:
  - `frontend/src-tauri/src/ollama/ollama.rs`
  - `frontend/src-tauri/src/ollama/metadata.rs`
  - `frontend/src-tauri/src/summary/llm_client.rs`
  - `backend/app/transcript_processor.py`
  - `frontend/src-tauri/tauri.conf.json`

## 4. Optional cloud AI provider calls

These calls occur only when a cloud provider is selected/configured.

### OpenAI

- **Endpoints**:
  - `https://api.openai.com/v1/models`
  - `https://api.openai.com/v1/chat/completions`
- **Purpose**:
  - Model listing
  - Summary generation
- **Source files**:
  - `frontend/src-tauri/src/openai/openai.rs`
  - `frontend/src-tauri/src/summary/llm_client.rs`
  - `backend/app/transcript_processor.py`

### Anthropic

- **Endpoints**:
  - `https://api.anthropic.com/v1/models`
  - `https://api.anthropic.com/v1/messages`
- **Purpose**:
  - Model listing
  - Summary generation
- **Source files**:
  - `frontend/src-tauri/src/anthropic/anthropic.rs`
  - `frontend/src-tauri/src/summary/llm_client.rs`
  - `backend/app/transcript_processor.py`

### Groq

- **Endpoints**:
  - `https://api.groq.com/openai/v1/models`
  - `https://api.groq.com/openai/v1/chat/completions`
- **Purpose**:
  - Model listing
  - Summary generation
- **Source files**:
  - `frontend/src-tauri/src/groq/groq.rs`
  - `frontend/src-tauri/src/summary/llm_client.rs`
  - `backend/app/transcript_processor.py`

### OpenRouter

- **Endpoints**:
  - `https://openrouter.ai/api/v1/models`
  - `https://openrouter.ai/api/v1/chat/completions`
- **Purpose**:
  - Model listing
  - Summary generation
- **Source files**:
  - `frontend/src-tauri/src/openrouter/openrouter.rs`
  - `frontend/src-tauri/src/summary/llm_client.rs`

### Custom OpenAI-compatible endpoint

- **Endpoint**: User-supplied
- **Purpose**:
  - Connection tests
  - Summary generation
- **Source files**:
  - `frontend/src-tauri/src/api/api.rs`
  - `frontend/src-tauri/src/summary/llm_client.rs`

## 5. Model and binary downloads

### Whisper model downloads

- **Host**: `https://huggingface.co`
- **Purpose**: Download Whisper model files
- **Source file**:
  - `frontend/src-tauri/src/whisper_engine/whisper_engine.rs`

### Parakeet model downloads

- **Hosts**:
  - `https://huggingface.co`
  - `https://meetily.towardsgeneralintelligence.com`
- **Purpose**: Download Parakeet model assets
- **Source file**:
  - `frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs`

### Built-in summary model downloads

- **Host**: `https://meetily.towardsgeneralintelligence.com`
- **Purpose**: Download built-in summary model files
- **Source files**:
  - `frontend/src-tauri/src/summary/summary_engine/models.rs`
  - `frontend/src-tauri/src/summary/summary_engine/model_manager.rs`

### FFmpeg auto-download

- **Endpoint/host**: Determined by the `ffmpeg_sidecar` crate at runtime
- **Purpose**: Download FFmpeg if not already available locally
- **Source file**:
  - `frontend/src-tauri/src/audio/ffmpeg.rs`
- **Important note**:
  - The repository code calls `check_latest_version`, `ffmpeg_download_url`, and `download_ffmpeg_package`, but the exact remote download URL is supplied by the dependency rather than hardcoded in this repo.

## 6. User-initiated external link opens

These are not background API calls, but they do open external destinations from the app.

Examples identified during the audit include:

- `https://github.com/Zackriya-Solutions/meeting-minutes/blob/main/PRIVACY_POLICY.md`
- `https://ollama.com/download`
- `https://meetily.zackriya.com/#about`
- `https://github.com/Zackriya-Solutions/meeting-minutes`

**Source files** include:

- `frontend/src/components/ModelSettingsModal.tsx`
- `frontend/src/components/About.tsx`
- `frontend/src/components/MeetingDetails/SummaryGeneratorButtonGroup.tsx`
- `frontend/src/hooks/meeting-details/useSummaryGeneration.ts`
- `frontend/src/components/onboarding/steps/SetupOverviewStep.tsx`

## 7. CSP / allowlisted network destinations

The Tauri CSP in `frontend/src-tauri/tauri.conf.json` explicitly allows:

- `http://localhost:11434`
- `http://localhost:5167`
- `http://localhost:8178`
- `https://api.ollama.ai`

This is an allowlist, not proof that all of these are actively used in every runtime path. It does, however, show intended network destinations.

## 8. Backend-specific note

The repository also contains a Python backend in `backend/app/` with outbound summary-provider calls:

- Anthropic
- Groq
- OpenAI
- Ollama on localhost

No backend telemetry implementation was identified during this audit.

## 9. Bottom line

The codebase currently supports or performs outbound calls in these categories:

1. **Auto-update checks**: GitHub release metadata
2. **Local app/service traffic**: localhost backend, localhost transcription server, local Ollama
3. **Optional cloud AI providers**: OpenAI, Anthropic, Groq, OpenRouter, custom OpenAI-compatible endpoints
4. **Downloads**: Whisper, Parakeet, built-in models, FFmpeg
5. **User-initiated external links**

If the goal is a fully offline / zero-egress build, these areas would need to be disabled, removed, or made opt-in with safer defaults.
