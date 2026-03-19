# Multitrack Audio Recording

**Date:** 2026-03-19
**Branch:** `feat/multitrack-audio`
**Related:** Issue #241 (multitrack audio), Issue #230 (speaker diarization)

## Problem

All audio sources (microphone + system) are mixed into a single mono track before saving. This makes it impossible to separate speakers from system audio in post-processing, limiting diarization and transcription accuracy.

## Solution

Save audio as **stereo** (mic = left, system = right) by default, with a user toggle to revert to mono (mixed) for compatibility. The STT pipeline is unaffected — it always receives mono mixed audio at 16kHz.

## Architecture

```
Ring Buffer
    |
extract_window() -> (mic_window, sys_window)
    |
    +---> mix_window() -> VAD -> Whisper/Parakeet (unchanged, always mono 16kHz)
    |
    +---> Recording path (CHANGED):
          |
          if stereo: interleave(mic, sys) -> encode(channels=2)
          if mono:   mix_window()         -> encode(channels=1)
```

### What changes

| File | Change |
|------|--------|
| `audio/pipeline.rs` (~L868, STEP 4) | Send interleaved stereo or mono mix based on recording mode |
| `audio/incremental_saver.rs` | Accept `channels: u16` in constructor, pass to encoder |
| `audio/recording_saver.rs` | Read recording mode from preferences, propagate channels to saver |
| `audio/recording_preferences.rs` | New field `recording_mode: "stereo" \| "mono"` (default: `"stereo"`) |
| `audio/recording_manager.rs` | Read preference and pass to pipeline/saver |
| Frontend Settings page | Toggle in Recordings section |
| `metadata.json` schema | Add `channels: u16` field |

### What does NOT change

- **Audio enhancement pipeline** — per-device processing (high-pass, RNNoise, EBU R128) happens before the ring buffer
- **STT/Whisper/Parakeet** — always receives mono mixed audio at 16kHz from VAD
- **Device monitor / reconnection** — independent of recording format
- **Import/retranscribe** — processes any audio input format

## Detailed Changes

### 1. Recording Preferences

New field in `recording_preferences.rs`:

```rust
pub enum RecordingMode {
    Stereo, // mic=L, system=R (default)
    Mono,   // mixed, current behavior
}
```

Persisted in the app's preferences file. Default: `Stereo`.

### 2. Pipeline Recording Path (pipeline.rs)

Current code (STEP 4, ~L868):
```rust
// Sends mixed mono audio to recording saver
let recording_chunk = AudioChunk {
    data: mixed_with_gain.clone(),
    ...
};
```

New behavior:
```rust
let recording_data = match recording_mode {
    RecordingMode::Stereo => interleave_stereo(&mic_window, &sys_window),
    RecordingMode::Mono => mixed_with_gain.clone(),
};
let recording_chunk = AudioChunk {
    data: recording_data,
    ...
};
```

The `interleave_stereo` function takes two mono buffers of equal length and produces `[L0, R0, L1, R1, ...]`.

### 3. Incremental Saver

Constructor receives `channels: u16`. Passes it to `encode_single_audio()` which already accepts a channels parameter.

### 4. Metadata

`metadata.json` gains a `channels` field:
```json
{
  "version": "1.0",
  "sample_rate": 48000,
  "channels": 2,
  "recording_mode": "stereo",
  ...
}
```

### 5. Frontend Settings Toggle

A toggle in the Recordings section of Settings:
- Label: "Separate audio tracks (stereo)"
- Description: "Record microphone and system audio on separate channels (left/right)"
- Default: ON (stereo)

Calls a Tauri command to update the preference.

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| Only mic, no system audio | Stereo with silent R channel |
| Only system, no mic | Stereo with silent L channel |
| Device disconnects mid-recording | Disconnected channel goes silent; other channel continues |
| User changes setting mid-recording | Takes effect on next recording (not current) |
| Old mono recordings | Metadata has `channels: 1`; playback/import unaffected |

## Testing

- Record with both devices → verify L/R separation in Audacity
- Record with one device disconnected → verify silent channel
- Toggle mono → verify mixed output matches current behavior
- Verify STT output is identical in both modes
- Verify metadata.json reflects correct channel count
