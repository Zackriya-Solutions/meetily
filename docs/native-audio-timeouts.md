# Bounded native audio startup and monitoring shutdown

Core Audio startup now waits up to ten seconds on a dedicated worker thread. A shared one-permit gate prevents repeated retries from accumulating blocked native workers. Caller cancellation or timeout marks startup cancelled, and callbacks reject late delivery. A late stream result is disposed on its worker rather than being handed to a later recording.

The operating system call itself cannot be interrupted. A worker can retain native resources until macOS returns; if it never returns, restarting Meetily is required before another Core Audio start.

The device monitor now waits up to two seconds when stopping, then cancels its async task. This bounds the caller wait; it does not guarantee interruption of synchronous OS enumeration already in flight.

The existing Core Audio system-capture entry points use the bounded startup. This branch is independent of application-specific audio capture and introduces no selection, session-schema, title, or quit-policy changes.

## Validation

The production startup helper includes regressions for a timed-out worker, bounded retries, disposal of late resources, caller cancellation, native rejection, and successful retry. The monitor includes a stalled-future regression.

Run from the workspace root:

```sh
cargo test -p meetily audio_startup --lib
cargo test -p meetily stalled_enumeration_cannot_block_recording_shutdown --lib
```

Validation on October 28, 2026 with Xcode 27.1: the macOS desktop executable builds, and all three startup-helper regressions plus the stalled-monitor regression pass. The complete library suite reports 255 passed, two failed, three ignored. The two failures (test_builtin_mic_detection and test_calculate_buffer_timeout_bluetooth) reproduce on unchanged upstream devtest.

Native validation excludes installer-only sidecars/resources through TAURI_CONFIG. Release packaging, code signing, and real permission/driver behavior remain outside these automated checks. No audio was captured during validation.
