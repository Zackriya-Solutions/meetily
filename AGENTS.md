# AGENTS.md

This repository's product of record is the native Tauri desktop application in [`desktop/`](desktop/).

## Stack

- Tauri 2
- Next.js 14
- React 18
- Rust
- SQLite via `sqlx`
- Local transcription via Whisper and Parakeet
- Summary providers via Ollama, BuiltInAI, OpenAI, Claude, Groq, OpenRouter, and custom OpenAI-compatible endpoints

## What Is Active

- Desktop UI in [`desktop/src/`](desktop/src/)
- Native commands and services in [`desktop/src-tauri/src/`](desktop/src-tauri/src/)
- Local database initialization in [`desktop/src-tauri/src/database/manager.rs`](desktop/src-tauri/src/database/manager.rs)
- Audio capture and transcription pipeline in [`desktop/src-tauri/src/audio/`](desktop/src-tauri/src/audio/)
- Summary templates in [`desktop/src-tauri/src/summary/`](desktop/src-tauri/src/summary/)

## What Is Not Active

- There is no separate FastAPI backend path to maintain in this fork.
- Speaker diarization is not implemented in the active transcription path.
- PDF and DOCX export workflows are not implemented in the active app path.

## Useful Commands

From [`desktop/`](desktop/):

```bash
pnpm install
pnpm run tauri:dev
pnpm run tauri:build
```

From the repository root:

```bash
cargo build -p llama-helper
cargo metadata --no-deps --format-version 1
cargo check -p meetily
```

## Documentation Rule

Keep documentation aligned with the current codebase. Remove stale claims instead of preserving marketing copy that no longer matches implementation.
