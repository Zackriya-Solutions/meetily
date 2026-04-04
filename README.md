![MeetFree Banner](docs/meetfree_banner.png)

# MeetFree

MeetFree is a native Tauri desktop application for recording meetings, transcribing speech locally, storing meeting data in a local SQLite database, and generating summaries with either local or optional external models.

## Current Scope

The current codebase includes:

- A desktop app built with Tauri, Next.js, React, and Rust.
- Local audio capture for microphone and system audio.
- Local transcription engines for Whisper and Parakeet.
- Local meeting storage in SQLite via the Tauri app.
- Summary generation with Ollama, BuiltInAI, OpenAI, Claude, Groq, OpenRouter, and custom OpenAI-compatible endpoints.
- Summary template discovery from built-in, bundled, and user template directories.
- Meeting import and retranscription workflows.
- Analytics code is currently disabled in this fork.

## Privacy Model

- Recording, transcription, and meeting storage run locally inside the desktop app.
- Summary generation can stay local when you use Ollama or BuiltInAI.
- Summary generation sends data to an external provider only when you configure and select one.
- Provider API keys are stored in the operating system credential store when available.
- Analytics are optional and default to off.

## Not In This Repo

The active application code does not currently include:

- PDF or DOCX export pipelines.
- A production FastAPI backend path for meeting storage or summarization.

## Build From Source

See [docs/BUILDING.md](docs/BUILDING.md) for verified build commands and platform notes.

The main desktop workflow lives under [`desktop/`](desktop/).

The current native build path also requires building [`llama-helper/`](llama-helper/) before Tauri packaging or `cargo check -p meetfree`.

## Documentation

- [docs/architecture.md](docs/architecture.md)
- [docs/technical-design.md](docs/technical-design.md)
- [docs/technical-design-v0.2.0.md](docs/technical-design-v0.2.0.md)
- [docs/execution-plan-v0.2.0.md](docs/execution-plan-v0.2.0.md)
- [docs/roadmap-v0.1.0.md](docs/roadmap-v0.1.0.md) (archived)
- [docs/BUILDING.md](docs/BUILDING.md)
- [PRIVACY_POLICY.md](PRIVACY_POLICY.md)
- [CONTRIBUTING.md](CONTRIBUTING.md)

## Repository Notes

- The product of record in this repository is the native Tauri application.
- Legacy backend-era documentation and code paths have been removed from this fork.
