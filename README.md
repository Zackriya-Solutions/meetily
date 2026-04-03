# Meetily

Meetily is a native Tauri desktop application for recording meetings, transcribing speech locally, storing meeting data in a local SQLite database, and generating summaries with either local or optional external models.

## Current Scope

The current codebase includes:

- A desktop app built with Tauri, Next.js, React, and Rust.
- Local audio capture for microphone and system audio.
- Local transcription engines for Whisper and Parakeet.
- Local meeting storage in SQLite via the Tauri app.
- Summary generation with Ollama, BuiltInAI, OpenAI, Claude, Groq, OpenRouter, and custom OpenAI-compatible endpoints.
- Summary template discovery from built-in, bundled, and user template directories.
- Meeting import and retranscription workflows.
- Optional product analytics, disabled by default.

## Privacy Model

- Recording, transcription, and meeting storage run locally inside the desktop app.
- Summary generation can stay local when you use Ollama or BuiltInAI.
- Summary generation sends data to an external provider only when you configure and select one.
- Analytics are optional and default to off.

## Not In This Repo

The active application code does not currently include:

- Speaker diarization in the live transcription path.
- PDF or DOCX export pipelines.
- A production FastAPI backend path for meeting storage or summarization.

## Build From Source

See [docs/BUILDING.md](docs/BUILDING.md) for verified build commands and platform notes.

The main desktop workflow lives under [`desktop/`](desktop/).

The current native build path also requires building [`llama-helper/`](llama-helper/) before Tauri packaging or `cargo check -p meetily`.

## Documentation

- [docs/architecture.md](docs/architecture.md)
- [docs/BUILDING.md](docs/BUILDING.md)
- [docs/GPU_ACCELERATION.md](docs/GPU_ACCELERATION.md)
- [PRIVACY_POLICY.md](PRIVACY_POLICY.md)
- [CONTRIBUTING.md](CONTRIBUTING.md)

## Repository Notes

- The product of record in this repository is the native Tauri application.
- Legacy backend-era documentation and code paths have been removed from this fork.
