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
| `audio/pipeline.rs` (STEP 4) | Send interleaved stereo or mono mix based on recording mode |
| `audio/recording_state.rs` | Add `channels: u16` field to `AudioChunk` |
| `audio/incremental_saver.rs` | Accept `channels: u16` in constructor, adjust checkpoint math |
| `audio/recording_saver.rs` | Read recording mode from preferences, propagate channels to saver |
| `audio/recording_preferences.rs` | New field `recording_mode: "stereo" \| "mono"` (default: `"stereo"`) |
| `audio/recording_manager.rs` | Read preference, pass `RecordingMode` through to pipeline and saver |
| Frontend Settings page | Toggle in Recordings section |
| `metadata.json` schema | Add `channels` and `recording_mode` fields |

### What does NOT change

- **Audio enhancement pipeline** — per-device processing (high-pass, RNNoise, EBU R128) happens before the ring buffer, operates on mono per-device
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

### 2. AudioChunk Channel Awareness

Add `channels: u16` to the `AudioChunk` struct in `recording_state.rs`:

```rust
pub struct AudioChunk {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,     // NEW: 1=mono, 2=stereo interleaved
    pub timestamp: f64,
    pub chunk_id: u64,
    pub device_type: DeviceType,
}
```

This makes chunks self-describing. All existing code that creates `AudioChunk` with mono data passes `channels: 1`. Only the recording path in STEP 4 of the pipeline produces `channels: 2` when in stereo mode.

### 3. Pipeline Recording Path (pipeline.rs)

**RecordingMode flows into the pipeline via constructor:**

```
RecordingManager.start_recording()
  -> reads RecordingMode from preferences
  -> AudioPipelineManager.start(..., recording_mode: RecordingMode)
    -> AudioPipeline::new(..., recording_mode: RecordingMode)
      -> stored as self.recording_mode
```

**STEP 4 behavior changes:**

```rust
let (recording_data, channels) = match self.recording_mode {
    RecordingMode::Stereo => (interleave_stereo(&mic_window, &sys_window), 2),
    RecordingMode::Mono => (mixed_with_gain.clone(), 1),
};
let recording_chunk = AudioChunk {
    data: recording_data,
    channels,
    ...
};
```

**`interleave_stereo` function:**

Takes two mono buffers and produces `[L0, R0, L1, R1, ...]`. Buffers are guaranteed equal length by `extract_window()` which zero-pads incomplete buffers. An assertion guards this invariant:

```rust
fn interleave_stereo(left: &[f32], right: &[f32]) -> Vec<f32> {
    assert_eq!(left.len(), right.len(), "Stereo interleave requires equal-length buffers");
    let mut interleaved = Vec::with_capacity(left.len() * 2);
    for (&l, &r) in left.iter().zip(right.iter()) {
        interleaved.push(l);
        interleaved.push(r);
    }
    interleaved
}
```

### 4. Incremental Saver

Constructor receives `channels: u16`. Key changes:

- **Store `channels` field** for use in encoding
- **Checkpoint interval adjustment**: measured in **frames** (not samples). For stereo, a frame = 2 samples. The interval becomes `sample_rate * 30` frames = `sample_rate * 30 * channels` samples. The sample count from `chunk.data.len()` must be divided by `channels` to get frame count for threshold comparison.
- **Pass `channels` to `encode_single_audio()`** (already accepts the parameter)
- **FFmpeg concat merge** (`merge_checkpoints`): no change needed — concat demuxer works with stereo MP4 files

### 5. Audio Recovery

`recover_audio_from_checkpoints()` currently makes no assumption about channel count (uses FFmpeg concat with `-c copy`). No change needed — it preserves whatever format the checkpoints have.

### 6. Metadata

`MeetingMetadata` struct gains two fields with serde defaults for backward compatibility:

```rust
pub struct MeetingMetadata {
    // ... existing fields ...
    #[serde(default = "default_channels")]
    pub channels: u16,
    #[serde(default = "default_recording_mode")]
    pub recording_mode: String,
}

fn default_channels() -> u16 { 1 }
fn default_recording_mode() -> String { "mono".to_string() }
```

Old metadata.json files without these fields deserialize correctly as mono.

### 7. Frontend Settings Toggle

A toggle in the Recordings section of Settings:
- Label: "Separate audio tracks (stereo)"
- Description: "Record microphone and system audio on separate channels (left/right)"
- Default: ON (stereo)

Calls a Tauri command to update the preference. Setting change takes effect on next recording.

### 8. Playback

Stereo recordings play as-is — mic in left ear, system in right ear. This is expected for analysis purposes. Users who want a mixed playback experience can use external tools (Audacity) or switch to mono mode. In-app playback is not modified in this iteration.

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| Only mic, no system audio | Stereo with silent R channel |
| Only system, no mic | Stereo with silent L channel |
| Device disconnects mid-recording | Disconnected channel goes silent; other channel continues |
| User changes setting mid-recording | Takes effect on next recording (not current) |
| Old mono recordings | Metadata defaults to `channels: 1`; playback/import unaffected |
| Old metadata.json without channels field | `#[serde(default)]` deserializes as `channels: 1, recording_mode: "mono"` |

## File Size

Stereo at 48kHz doubles raw audio data vs mono. After AAC encoding, the increase is roughly 1.5-2x. At typical meeting lengths (1-2 hours), this adds ~50-100 MB — acceptable for local storage.

## Testing

### Unit tests
- `interleave_stereo()` — correct interleaving, assertion on mismatched lengths
- `IncrementalAudioSaver` with `channels: 2` — checkpoint triggers at correct intervals
- Metadata deserialization — old format (no channels) defaults correctly

### Integration tests (manual)
- Record with both devices → verify L/R separation in Audacity
- Record with one device disconnected → verify silent channel
- Toggle mono → verify mixed output matches current behavior
- Verify STT output is identical in both modes
- Verify metadata.json reflects correct channel count and recording mode
