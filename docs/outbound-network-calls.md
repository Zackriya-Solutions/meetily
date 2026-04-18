# Outbound Network Calls Audit

This document summarizes the outbound network calls identified in the current Meetily repository after telemetry and auto-update removal.

## Scope

This audit covers:

- Direct outbound HTTP(S) endpoints hardcoded in the repository
- Localhost network calls between app components
- Optional third-party calls triggered by configuration or user action
- Model downloads, and external-link opens

This audit does **not** claim to be a packet capture. It is a code-level inventory of observed egress paths.

## Executive Summary

Meetily is **not network-silent**, but the application code no longer includes built-in telemetry or auto-update checks.

The repository currently contains:

- **Local service calls** to the bundled/local backend and local model servers
- **Optional cloud AI provider calls** when those providers are configured
- **Model and dependency downloads**
- **User-initiated external link opens**

## 1. Telemetry

No built-in telemetry endpoints are present in the active frontend or Tauri runtime.

## 2. Localhost / local service traffic

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

## 3. Optional cloud AI provider calls

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

## 4. Model and binary downloads

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

## 5. User-initiated external link opens

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

## 6. CSP / allowlisted network destinations

The Tauri CSP in `frontend/src-tauri/tauri.conf.json` explicitly allows:

- `http://localhost:11434`
- `http://localhost:5167`
- `http://localhost:8178`
- `https://api.ollama.ai`

This is an allowlist, not proof that all of these are actively used in every runtime path. It does, however, show intended network destinations.

## 7. Backend-specific note

The repository also contains a Python backend in `backend/app/` with outbound summary-provider calls:

- Anthropic
- Groq
- OpenAI
- Ollama on localhost

No backend telemetry implementation was identified during this audit.

## 8. Bottom line

The codebase currently supports or performs outbound calls in these categories:

1. **Local app/service traffic**: localhost backend, localhost transcription server, local Ollama
2. **Optional cloud AI providers**: OpenAI, Anthropic, Groq, OpenRouter, custom OpenAI-compatible endpoints
3. **Downloads**: Whisper, Parakeet, built-in models, FFmpeg
4. **User-initiated external links**

If the goal is a fully offline / zero-egress build, these areas would need to be disabled, removed, or made opt-in with safer defaults.
