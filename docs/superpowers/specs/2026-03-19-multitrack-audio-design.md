# Multitrack Audio Recording

**Date:** 2026-03-19
**Branch:** `feat/multitrack-audio`
**Related:** Issue #241 (multitrack audio), Issue #230 (speaker diarization)

## Problem

All audio sources (microphone + system) are mixed into a single mono track before saving. This makes it impossible to separate speakers from system audio in post-processing, limiting diarization and transcription accuracy.

## Solution

Add a **stereo recording mode** (mic = left, system = right) as an opt-in advanced option. **Default remains mono** (current behavior) for maximum compatibility and least-surprise UX. The STT pipeline is unaffected — it always receives mono mixed audio at 16kHz.

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
          if mono:   mix_window()         -> encode(channels=1)  [default]
```

### What changes

| File | Change |
|------|--------|
| `audio/pipeline.rs` (STEP 4) | Send interleaved stereo or mono mix based on recording mode; pre-allocated interleave buffer |
| `audio/recording_state.rs` | Add `channels: u16` field to `AudioChunk` |
| `audio/incremental_saver.rs` | Accept `channels: u16` in constructor, adjust checkpoint math (frames not samples) |
| `audio/recording_saver.rs` | Read recording mode from preferences, propagate channels to saver |
| `audio/recording_preferences.rs` | New field `recording_mode: "stereo" \| "mono"` (default: `"mono"`) |
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
    Mono,   // mixed, current behavior (DEFAULT)
    Stereo, // mic=L, system=R
}
```

Persisted in the app's preferences file. Default: `Mono`.

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
    RecordingMode::Stereo => {
        self.interleave_buffer.clear();
        interleave_stereo_into(&mic_window, &sys_window, &mut self.interleave_buffer);
        (self.interleave_buffer.as_slice(), 2u16)
    },
    RecordingMode::Mono => (mixed_with_gain.as_slice(), 1u16),
};
```

**`interleave_stereo_into` function (pre-allocated buffer):**

Takes two mono buffers and writes `[L0, R0, L1, R1, ...]` into a reusable buffer. Buffers are guaranteed equal length by `extract_window()` which zero-pads incomplete buffers:

```rust
fn interleave_stereo_into(left: &[f32], right: &[f32], out: &mut Vec<f32>) {
    debug_assert_eq!(left.len(), right.len(), "Stereo interleave requires equal-length buffers");
    out.reserve(left.len() * 2);
    for (&l, &r) in left.iter().zip(right.iter()) {
        out.push(l);
        out.push(r);
    }
}
```

**Performance notes:**
- Uses `debug_assert_eq!` (zero cost in release builds) instead of `assert_eq!`
- Buffer `self.interleave_buffer: Vec<f32>` is pre-allocated in `AudioPipeline::new()` and reused each window to avoid per-window allocation (~1.7 allocs/sec eliminated)

### 4. Incremental Saver

Constructor receives `channels: u16`. Key changes:

- **Store `channels` field** for use in encoding and math
- **Checkpoint interval adjustment**: measured in **frames** (not samples). For stereo, a frame = 2 samples. All duration/threshold calculations use `data.len() / channels as usize`:
  - Checkpoint threshold: `sample_rate * 30` frames → compare against `total_samples / channels`
  - Duration calculation: `audio_data.len() / channels / sample_rate` (not `audio_data.len() / sample_rate`)
- **Pass `channels` to `encode_single_audio()`** (already accepts the parameter)
- **FFmpeg concat merge** (`merge_checkpoints`): no change needed — concat demuxer works with stereo MP4 files
- **AAC bitrate**: consider 256kbps for stereo mode (vs current bitrate for mono) to maintain per-channel quality

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
- Label: "Separate audio tracks (advanced)"
- Description: "Record microphone and system audio on separate stereo channels (left/right). Useful for speaker diarization and post-processing."
- Default: OFF (mono)

Calls a Tauri command to update the preference. Setting change takes effect on next recording.

### 8. Playback

Stereo recordings play as-is — mic in left ear, system in right ear. This is expected for analysis/diarization purposes. Users who want a mixed playback experience should use mono mode (default) or external tools like Audacity.

**Future iterations (not in scope):**
- Downmix to mono for in-app playback
- "Export as mono" button per meeting
- Visual badge (Stereo/Mono) in meetings list

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| Only mic, no system audio | Stereo with silent R channel |
| Only system, no mic | Stereo with silent L channel |
| Device disconnects mid-recording | Disconnected channel goes silent; other channel continues |
| User changes setting mid-recording | Takes effect on next recording (not current) |
| Old mono recordings | Metadata defaults to `channels: 1`; playback/import unaffected |
| Old metadata.json without channels field | `#[serde(default)]` deserializes as `channels: 1, recording_mode: "mono"` |
| Retranscribe a stereo file from disk | STT path always receives live mono stream; retranscribe uses FFmpeg which handles stereo input |

## File Size

Stereo at 48kHz doubles raw audio data vs mono. After AAC encoding at 256kbps, the increase is roughly 1.5-2x. At typical meeting lengths (1-2 hours), this adds ~50-100 MB — acceptable for local storage.

## Testing

### Unit tests (required before merge)
- `interleave_stereo_into()` — correct interleaving, debug_assert on mismatched lengths
- `IncrementalAudioSaver` with `channels: 2` — checkpoint triggers at correct frame intervals (30s, not 15s)
- Duration calculation with `channels: 2` — reports correct seconds
- Metadata deserialization — old format (no channels) defaults correctly

### Integration tests (manual)
- Record with both devices → verify L/R separation in Audacity
- Record with one device disconnected → verify silent channel
- Toggle mono → verify mixed output matches current behavior
- Verify STT output is identical in both modes
- Verify metadata.json reflects correct channel count and recording mode

## Design Decisions Log

| Decision | Rationale |
|----------|-----------|
| Default mono, stereo opt-in | Least surprise for casual users; stereo L/R playback is confusing without context |
| Pre-allocated interleave buffer | Eliminates ~1.7 heap allocations/sec in hot path |
| `debug_assert_eq!` over `assert_eq!` | Zero cost in release; `extract_window()` guarantees equal lengths |
| Stereo interleaved over 2 separate files | Temporal alignment guaranteed by construction; simpler recovery; single file management |
| Frames-based checkpoint math | Prevents stereo checkpoints at half the intended duration |
| AAC 256kbps for stereo | Maintains per-channel quality (128kbps effective per channel) |
