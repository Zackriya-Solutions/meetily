# Privacy Policy

## Scope

This policy describes the behavior of the code in this repository.

## Local Data Handling

- Meeting recordings, transcripts, summaries, and settings are stored by the desktop app on the local machine.
- The desktop app initializes its SQLite database in the Tauri app data directory.
- Local transcription is handled by the native app through Whisper or Parakeet.

## Optional External Providers

Meetily can generate summaries with external providers if you configure and select them.

The current codebase supports:

- OpenAI
- Claude
- Groq
- OpenRouter
- Custom OpenAI-compatible endpoints

If you use one of these providers, the summary request leaves the local machine and is handled under that provider's terms.

## Local Summary Options

The current codebase also supports local summary generation through:

- Ollama
- BuiltInAI

## Analytics

- Analytics are optional.
- Analytics default to off.
- When enabled, analytics use PostHog and a generated pseudonymous user identifier.

The analytics UI and event layer describe collection in terms of:

- App usage events
- Session and feature usage data
- Platform and architecture metadata
- Model/provider selection metadata
- Anonymous device-type and meeting-performance metrics

The current analytics UI explicitly says it does not collect:

- Meeting names or titles
- Meeting transcripts or content
- Audio recordings
- Device names
- Personal information

## No Blanket Local-Only Claim

This repository should not be described as "no data ever leaves your machine" without qualification.

That statement is only true when:

- You use local transcription, and
- You use local summary providers, and
- Analytics remain disabled.
