# Native recording session persistence and recovery

Rust owns one recording session through starting, recording/paused, stopping, processing, and saving. A session ID binds the transcript collector, Stop ownership, persisted meeting, completion events, and recovery checkpoints. React presents the saved result instead of resaving its own transcript-state snapshot.

## Persistence contract

The recording_sessions migration maps each session to at most one meeting. Meeting/transcript creation and mapping run in one BEGIN IMMEDIATE transaction so concurrent retry returns the existing meeting. The writer reservation also avoids upgrading a stale SQLite WAL read snapshot during finalization. Empty-transcript recordings can still create a saved meeting.

Transcript workers append to the session collector before emitting UI events and atomically replace transcript checkpoint JSON. Startup reconciliation attempts to recover readable checkpoints from interrupted native sessions. Unreadable data remains available for manual recovery. Deleting a mapped meeting marks the native session deleted so browser checkpoints cannot recreate it.

Browser recovery queries durable native session status before offering checkpoints. Completed mapped, active, and deleted sessions are excluded. Error/interrupted sessions remain candidates. IndexedDB metadata merges and saved flags commit transactionally, preventing a late write from marking an already saved meeting unsaved.

## Integration boundaries

All recording-start entry points reserve a native session, and tray Stop uses the same finalization path as the UI. Duplicate Stop callers wait for the owned result; a stale request cannot stop a newer session. Stop failures surface an error and refresh native state. Model teardown is bounded and completion analytics runs after persistence.

This proposal does not change normal Quit behavior, title formatting, save-folder selection, application-specific capture, or summary-editor loading. Saver drain/error propagation overlaps upstream PR #789 and should be coordinated before merge. Startup/monitor timeout work is in a separate contribution.

The migration adds recording_sessions and session_preferences. It does not rewrite existing meetings. Upgrade/relaunch behavior and compatibility with an older application accessing a migrated database still require native acceptance testing.

## Validation

The frontend passes TypeScript, 50 suite entries including isolated native-event/recovery/Stop/IndexedDB fixtures, and its production build. Changed Rust files pass syntax parsing, which is not native type checking.

Native tests in session/persistence_tests.rs cover empty/buffered results, concurrent saves, a file-backed WAL writer, recovery ownership/deletion rules, interrupted checkpoint reconciliation, and status queries. These tests and native GUI/audio checks have not been run on this rebased branch: Xcode SDK access is blocked until the local license is accepted. No recordings or user database were accessed during preparation.

```sh
cargo test -p meetily session:: --lib
cargo test -p meetily shutdown --lib
```
