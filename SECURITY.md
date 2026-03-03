# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.3.x   | :white_check_mark: |
| < 0.3   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly:

1. **Do NOT open a public GitHub issue** for security vulnerabilities
2. Email security concerns to: security@meetily.ai
3. Or use [GitHub Security Advisories](https://github.com/Azizim3/meetily/security/advisories/new)

We will acknowledge receipt within 48 hours and provide a detailed response within 7 days.

## Security Contact

- Email: security@meetily.ai
- PGP: Available upon request

## Data Handling

Meetily is designed as a privacy-first application:

- **Audio recording**: Stored locally on your device only
- **Transcription**: Processed locally via Whisper/Parakeet (on-device)
- **Summarization**: Configurable — can use local models (Ollama/built-in) or cloud providers (OpenAI, Claude, Groq, OpenRouter)
- **API keys**: Stored in the macOS Keychain (not in plaintext)
- **Analytics**: Completely removed — no telemetry data is collected or transmitted
- **Database**: SQLite stored locally in your app data directory

### What is sent externally (only when configured by user)

| Data | Destination | When |
|------|-------------|------|
| Transcript text | Cloud LLM provider | Only during summary generation with cloud provider |
| Model name | Cloud LLM provider | Only during summary generation |

### What stays local

- Audio recordings
- Transcriptions
- Meeting metadata
- API keys (in system keychain)
- All application settings

## Threat Model

### In Scope
- Local data protection (file permissions, encryption at rest)
- API key security (keychain storage)
- Network security (TLS for cloud API calls)
- Content Security Policy enforcement
- Dependency supply chain security

### Out of Scope
- Physical device access (if attacker has your unlocked Mac, all bets are off)
- Cloud provider security (OpenAI, Anthropic, etc. handle their own security)
- Network-level attacks (MITM on your local network)

## Known Limitations

1. **Local Ollama endpoint**: Communication with Ollama uses HTTP (not HTTPS) on localhost. This is acceptable for local-only traffic but should not be exposed to the network.
2. **FFmpeg sidecar**: Built from Homebrew source; verify checksums match expected values.
3. **ONNX Runtime**: Uses release candidate version (`ort 2.0.0-rc.10`) — pinned to specific version for reproducibility.

## Security Hardening Changelog

### v0.3.0-hardened
- Migrated API key storage from plaintext SQLite to macOS Keychain
- Completely removed PostHog analytics telemetry
- Rebuilt FFmpeg from verified Homebrew source
- Tightened Content Security Policy
- Added automated dependency scanning (cargo audit, pnpm audit)
- Added SHA256 checksums for release artifacts
- Pinned ONNX Runtime to specific version
