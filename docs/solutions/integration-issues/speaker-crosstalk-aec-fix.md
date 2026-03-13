---
title: "Speaker Cross-talk Fixed with Acoustic Echo Cancellation (AEC)"
category: integration-issues
component: audio-pipeline
tags: [speaker-separation, aec, voice-processing, macos, avfoundation, cidre]
severity: high
date_solved: 2026-03-07
symptoms:
  - Microphone picks up system audio played through speakers
  - Dual-VAD misattributes system audio as local speaker ("Me")
  - Speaker labels confused when both mic and system audio active simultaneously
  - Works correctly only when speakers are muted
root_cause: Missing acoustic echo cancellation on microphone capture
---

# Speaker Cross-talk Fixed with Acoustic Echo Cancellation (AEC)

## Problem

After implementing dual-VAD speaker separation (mic = "Me", system = "Others"), transcription labels were frequently wrong when speakers were unmuted. The microphone picked up system audio playing through the speakers, causing the mic VAD to trigger and label that audio as "Me" instead of "Others".

### Symptoms

- Speaker labels switched unpredictably during meetings
- `DeviceType::Microphone` VAD triggered on system audio bleed-through
- Problem disappeared when speakers were muted (confirming acoustic coupling)
- Granola (competitor app) did not exhibit this issue at all

## Investigation

### What didn't work

1. **Adjusting VAD thresholds** - Raising the mic VAD threshold would also miss quiet speech
2. **Audio level comparison** - System audio through speakers has comparable energy to speech at the mic
3. **Timing-based heuristics** - Can't reliably distinguish echoed audio from simultaneous speech

### Key insight: Granola's approach

Extracted and analyzed Granola's audio architecture from `/Applications/Granola.app`:

- Uses a native `AudioCapture` module with built-in AEC
- `startAudioCapture(useCoreAudio, disableEchoCancellationOnHeadphones, ...)`
- AEC enabled by default, only disabled when headphones are detected
- Two separate WebSocket connections to Deepgram (one per source)
- `diarize: false` - device-based separation, not diarization

This confirmed AEC is essential for device-based speaker separation, not optional.

### macOS AEC options evaluated

| Option | Mechanism | Effort | Risk |
|--------|-----------|--------|------|
| AVAudioEngine Voice Processing | `kAudioUnitSubType_VoiceProcessingIO` via cidre | Low | Low |
| Raw AudioUnit (coreaudio-rs) | Direct AudioUnit API | Medium | Medium |
| AVAudioEngine via cidre (no blocks) | Manual rendering mode | High | High |
| Software AEC (WebRTC-style) | Cross-correlation subtraction | Medium | High |

## Root Cause

CPAL's `build_input_stream` captures raw microphone audio without any signal processing. When system audio plays through speakers, the mic picks up that audio. Without AEC subtracting the known system audio signal from the mic input, there's no way to distinguish the user's voice from echoed system audio.

## Solution

Replaced CPAL with `AVAudioEngine` + Voice Processing for microphone capture on macOS. Apple's `kAudioUnitSubType_VoiceProcessingIO` provides hardware-accelerated AEC that subtracts the system audio reference signal from the microphone input.

### Files changed

1. **`frontend/src-tauri/Cargo.toml`** - Added `"blocks"` feature to cidre (required for `install_tap_on_bus`)
2. **`frontend/src-tauri/src/audio/capture/voice_processing.rs`** (new) - AVAudioEngine mic capture with AEC (~100 lines)
3. **`frontend/src-tauri/src/audio/capture/mod.rs`** - Export new module
4. **`frontend/src-tauri/src/audio/stream.rs`** - Use VP for mic on macOS, CPAL fallback
5. **`frontend/src-tauri/src/audio/pipeline.rs`** - Removed debug audio level logging

### Core implementation

```rust
// voice_processing.rs - key excerpt
let mut engine = av::audio::Engine::new();
let mut input_node = engine.input_node();

// Enable Voice Processing (AEC + noise suppression)
input_node.set_vp_enabled(true)?;
input_node.set_vp_agc_enabled(true);

// Request 48kHz mono f32 (matches pipeline expectations)
let tap_format = av::audio::Format::with_common_format_sample_rate_channels_interleaved(
    av::audio::CommonFormat::PcmF32, 48000.0, 1, true,
)?;

// Tap receives AEC-processed audio
input_node.install_tap_on_bus(0, 4800, Some(&tap_format), move |buffer, _time| {
    if let Some(data) = buffer.data_f32_at(0) {
        capture.process_audio_data(&data[..buffer.frame_len() as usize]);
    }
})?;

engine.prepare();
engine.start()?;
```

### Architecture

```
Before (CPAL - no AEC):
  Mic hardware -> CPAL -> AudioCapture -> Pipeline (mic VAD)
  Speaker output -> mic picks up echo -> mic VAD triggers -> wrong "Me" label

After (AVAudioEngine VP):
  Mic hardware -> VoiceProcessingIO (AEC subtracts speaker signal) -> Tap -> AudioCapture -> Pipeline
  Speaker output -> AEC reference -> subtracted from mic -> clean speech only
```

### Fallback behavior

If Voice Processing fails to initialize (e.g., permissions, hardware issues), the system falls back to CPAL automatically with a warning log. This ensures recording always works, even without AEC.

## Prevention

- **Device-based speaker separation requires AEC** - this is not optional when speakers are active. Document this as a hard requirement.
- **Test with speakers unmuted** - always verify speaker separation with system audio playing through speakers, not just on mute.
- **Cross-platform consideration** - Windows/Linux will need their own AEC solution (WASAPI voice capture, PulseAudio echo cancellation) when speaker separation is ported.

## Related

- Granola's native AudioCapture module analysis (session research)
- `todos/001-pending-p1-speaker-dropped-in-sqlx-save-path.md` - speaker field storage
- Apple docs: [AVAudioEngine Voice Processing](https://developer.apple.com/documentation/avfaudio/avaudioinputnode/1390585-voiceprocessingenabled)
