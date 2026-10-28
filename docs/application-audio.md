# Record audio from one application

On macOS 14.2 or later, open **Audio Device Settings** or **Settings → Recording**, choose **Specific application**, then select a running app. Idle apps appear too; they do not need to be playing audio when you select them. Use search or **Refresh** to find an app opened after the list loaded.

**Include microphone** controls whether your own microphone is recorded. Turn it off for application audio only. A recording started with the microphone off does not open or request access to a microphone. Your source and microphone choices are saved for the next recording.

The selected application continues playing through your speakers or headphones. Other applications' digital output is excluded. An enabled physical microphone can still pick up sound from speakers; use headphones when you need acoustic separation.

Selecting Chrome, Safari, or another browser may capture audio from multiple tabs. This feature selects applications, not individual meetings or tabs. Zoom and Teams helper processes are included when native process ownership identifies them as belonging to the selected application.

If the app closes or its audio process restarts, Meetily keeps the recording open and displays **Waiting for [app]**. It continues recording the microphone if enabled and reconnects to the same application automatically. Gaps remain silent in the saved recording so transcript timing remains aligned. Stop remains available while waiting. Source selection is locked until the recording ends.

If capture reports an error, check macOS **System Settings → Privacy & Security** for system audio recording permission for Meetily. Silence by itself does not prove permission was denied: the app may simply be idle. App-specific capture uses native Core Audio taps and does not require BlackHole.

Windows and Linux retain their existing system/device capture. App-specific capture is not available there in this version. **All system audio** remains the default for existing installations.

## Implementation notes

- Saved `source_options` contains `source` (`system` or `application`) and `microphoneEnabled`. Application identity contains `bundleId`, `bundlePath`, and a display name. PIDs and HAL object IDs are never persisted.
- All start commands accept optional `sourceOptions`. Omitted options load saved preferences; explicit options take precedence even when both hardware device arguments are null.
- `get_audio_capture_capabilities` and `list_audio_applications` drive the picker. `get_recording_state.source_status` and `recording-source-changed` expose session identity, selection, capture/wait/error state, and a user-facing message.
- The capture supervisor resolves a fresh process snapshot every two seconds, matching bundle identity and ownership rather than application names. It also reopens the tap after output-device/format changes. The allowlist contains HAL object IDs, not PIDs. Empty lists wait; they never create a global tap.
- The recording owns its supervisor and discovery worker. Stopping aborts both, preventing an older session from attaching to a later recording. Startup errors tear down streams and the pipeline before a recording folder is created.
- Application capture and mic-off recording use one 50ms mixing clock, with 100ms input buffering. Missing sources contribute silence. Paused time is excluded and the final duration freezes on stop. Existing all-system-plus-microphone mixing is unchanged.
- A missing saved app stays selected. Unreadable settings fail startup rather than silently replacing an application selection with all-system capture.

## Release verification checklist

### Contribution branch verification — October 28, 2026

This branch is based on the current upstream devtest. Its logging conflict resolution preserves upstream diagnostic aggregation and privacy-conscious logging.

| Check | Result |
| --- | --- |
| TypeScript (pnpm exec tsc --noEmit --incremental false) | Passed. |
| Production frontend (pnpm build) | Passed. |
| Frontend tests (Bun 1.4.4, bun test tests) | 54 passed, 0 failed, including isolated recording-start and reload fixtures. |
| Rust syntax parsing (rustfmt --emit stdout) | Passed for changed Rust sources; this is not a type check or native build. |
| Native build and Rust tests | Pending: the local Xcode toolchain refuses SDK access until its license is accepted. |
| Windows/Linux compilation | Pending platform CI. |
| Live application isolation and reconnection | Pending; no microphone, meeting, or application audio was recorded during PR preparation. |

Automated coverage includes saved selection compatibility, source propagation, ownership matching, mic-off selection, waiting without a tap, worker cancellation, shared timing, and empty-allowlist rejection. The Rust cases still need execution on this rebased branch.

The following are interactive acceptance checks, not claims established by unit tests:

| Scenario | Expected result |
| --- | --- |
| Zoom meeting plus music in another app | Saved audio/transcript includes Zoom; unrelated digital music is excluded. |
| Teams meeting plus music in another app | Same isolation, including Teams audio helpers. |
| Chrome with two audible tabs | Both may be present; the picker accurately describes browser-wide capture. |
| Microphone off | No mic permission prompt or microphone stream; local speech is absent. |
| App initially idle | Recording waits or captures silence until audio begins. |
| App exit and restart | Persistent warning, preserved silent gap, automatic recovery to that app only. |
| Helper restart or output/Bluetooth change | Capture recovers without a global tap, duplicate audio, or an extra session. |
| Denied/revoked system audio permission | Native errors surface; silence alone is not misclassified as denial. |
| Pause/resume and stop while paused | Paused time is excluded from saved audio and transcript timestamps. |
| Repeated start/stop; stop during reconnection | No retained mic, tap, aggregate device, or discovery task. |
| Navigation/frontend reload while waiting | The backend source and waiting state remain visible. |

Use headphones, a consenting test call or controlled audio fixture, and distinguishable background audio for the isolation tests. Inspect the saved recording as well as the live transcript. Record the OS version, tested app versions, source choice, and results before releasing the feature.
