![MeetFree Banner](docs/meetfree_banner.png)

# MeetFree

MeetFree is a local-first desktop app for meeting capture, transcription, search, and summaries.

Built with Tauri + Next.js + Rust. Data stays on-device by default.

## Why MeetFree

- Reliable backend-owned recording finalization
- Fast transcript retrieval with SQLite FTS5
- Portable markdown export (single and batch)
- First-class import and retranscribe workflows
- Flexible summary providers, including MeetFree Built-in

## What’s Included in v0.2.0

- Record -> finalize -> persist pipeline with durable metadata
- Structured transcript search with filters and ranked snippets
- Markdown export with YAML frontmatter + standard sections
- Transcript cleanup + vocabulary corrections across display, export, and summary input
- Built-in model recommendation metadata + validated file import for supported models

## Quick Start

### Prerequisites

- Node.js 20+
- pnpm
- Rust toolchain

### Run locally

```bash
pnpm --dir desktop install
pnpm --dir desktop tauri:dev
```

### Release checks

```bash
cargo check -p meetfree
cargo test -p meetfree --lib
pnpm --dir desktop lint
pnpm --dir desktop build
```

## Architecture

- Frontend: `desktop/src/`
- Native backend: `desktop/src-tauri/src/`
- Database init + migrations: `desktop/src-tauri/src/database/` and `desktop/src-tauri/migrations/`
- Audio/transcription pipeline: `desktop/src-tauri/src/audio/`
- Summary engine/providers: `desktop/src-tauri/src/summary/`

## Privacy

- Recording, transcription, and storage run locally
- Cloud providers are optional and user-configured
- Provider keys are stored in OS-backed secure storage when available

## Active Documentation

- [docs/technical-design-v0.2.0.md](docs/technical-design-v0.2.0.md)
- [docs/execution-plan-v0.2.0.md](docs/execution-plan-v0.2.0.md)
- [docs/architecture.md](docs/architecture.md)
- [docs/BUILDING.md](docs/BUILDING.md)
- [PRIVACY_POLICY.md](PRIVACY_POLICY.md)
- [CONTRIBUTING.md](CONTRIBUTING.md)

Archived historical roadmap:

- [docs/roadmap-v0.1.0.md](docs/roadmap-v0.1.0.md)
