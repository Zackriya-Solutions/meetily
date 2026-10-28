# Native recording session persistence and recovery

Rust owns one recording session through starting, recording/paused, stopping, processing, and saving. A session ID binds the transcript collector, Stop ownership, persisted meeting, completion events, and recovery checkpoints. React presents the saved result instead of resaving its own transcript-state snapshot.

## Persistence contract

The recording_sessions migration maps each session to at most one meeting. Meeting/transcript creation and mapping run in one BEGIN IMMEDIATE transaction so concurrent retry returns the existing meeting. The writer reservation also avoids upgrading a stale SQLite WAL read snapshot during finalization. Empty-transcript recordings can still create a saved meeting.

Transcript workers append to the session collector before emitting UI events and atomically replace transcript checkpoint JSON. Startup reconciliation attempts to recover readable checkpoints from interrupted native sessions. Unreadable data remains available for manual recovery. Deleting a mapped meeting marks the native session deleted so browser checkpoints cannot recreate it.

Browser recovery queries durable native session status before offering checkpoints. Completed mapped, active, and deleted sessions are excluded. Error/interrupted sessions remain candidates. IndexedDB metadata merges and saved flags commit transactionally, preventing a late write from marking an already saved meeting unsaved.

## Integration boundaries

All recording-start entry points reserve a native session, preserve saved devices for default starts, and tray Stop uses the same finalization path as the UI. Duplicate Stop callers wait for the owned result; a stale request cannot stop a newer session. Stop failures surface an error and refresh native state. Model teardown is bounded and completion analytics runs after persistence.

This proposal does not change normal Quit behavior, title formatting, save-folder selection, application-specific capture, or summary-editor loading. Saver drain/error propagation overlaps upstream PR #789 and should be coordinated before merge. Startup/monitor timeout work is in a separate contribution.

The migration adds recording_sessions and session_preferences. It does not rewrite existing meetings. An upgrade regression applies the production migrations to a database containing an existing meeting/transcript, repeats migration safely, and verifies that those records and foreign-key enforcement remain intact. The original SQLx migrator rejects the newer database with VersionMissing; downgrading the application against an already migrated database is therefore unsupported. Native GUI relaunch/recovery still needs live acceptance testing.

## Validation

The frontend passes TypeScript, 50 suite entries including isolated native-event/recovery/Stop/IndexedDB fixtures, and its production build. The macOS desktop executable builds with Xcode 27.1.

Native tests in session/persistence_tests.rs cover empty/buffered results, concurrent saves, a file-backed WAL writer, recovery ownership/deletion rules, interrupted checkpoint reconciliation, and status queries. The revised native suite passes 263 tests with three ignored, excluding only two unchanged device-detection failures reproduced on upstream devtest. The new saved-device startup and existing-data upgrade regressions pass. Native GUI/audio checks are pending. Builds exclude installer-only sidecars/resources through TAURI_CONFIG, so installer signing/packaging is not covered. No user recordings or database were accessed.

```sh
cargo test -p meetily session:: --lib
cargo test -p meetily shutdown --lib
```
