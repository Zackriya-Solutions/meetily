# Streaming Partial Transcription

**Date:** 2026-03-20
**Branch:** `feat/streaming-partial-transcription`
**Related:** Improves real-time transcription UX

## Problem

Currently, transcription only appears after the speaker pauses (VAD redemption_time = 400ms of silence). For long continuous speech, the user sees nothing for 5-10+ seconds until the pause, then a wall of text appears at once.

## Solution

Send **partial transcriptions** during continuous speech for immediate visual feedback, then replace them with a **final transcription** of the complete segment when the speaker pauses. This is the same pattern used by YouTube Live Captions, Google Meet, and Zoom.

## User Experience

```
Speaker talking...    → [partial, gray]  "Testando um dois"
Still talking...      → [partial, gray]  "Testando um dois três quatro"
Speaker pauses (400ms)→ [final, black]   "Testando 1, 2, 3, 4, 5, 6."
                                          (replaces all partials)
```

## Architecture

```
VAD accumulating speech
    |
    +---> Every ~1.5s of accumulated speech:
    |       Send accumulated audio so far to Whisper → emit partial transcript
    |       (displayed as gray/italic in frontend)
    |
    +---> On speech end (400ms silence):
            Send FULL segment to Whisper → emit final transcript
            (replaces all partials for this segment, displayed as normal text)
```

### Key Design Decisions

- **Partial interval**: ~1.5s of accumulated speech (not wall-clock time)
- **Partials use accumulated audio**: each partial includes ALL audio since speech start, not just the delta. This gives Whisper full context for better accuracy.
- **Final replaces partials**: frontend receives a sequence_id; partials and final share the same sequence_id. Final overwrites all partials with that ID.
- **No extra GPU cost for final**: the final transcription processes the same audio the last partial already covered, just with the complete tail end included.

### What changes

| Component | Change |
|-----------|--------|
| `audio/vad.rs` | Emit intermediate "partial" segments during ongoing speech (every ~1.5s) |
| `audio/transcription/worker.rs` | Mark chunks as partial vs final, emit with sequence_id |
| `audio/recording_saver.rs` | Only persist final segments (ignore partials) |
| Frontend `TranscriptContext` | Handle partial/final events, replace partials on final |
| Frontend transcript display | Show partials in gray/italic, finals in normal style |

### What does NOT change

- VAD redemption_time (still 400ms)
- Whisper model/engine
- Audio pipeline/mixer
- Recording/saving flow (only finals are saved)
- Multitrack recording

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| Very short speech (<1.5s) | No partial emitted, only final |
| Speech >30s without pause | Partials every ~1.5s, final when pause detected |
| Low confidence partial | Still shown (user expects to see something) |
| Final confidence < threshold | Still saved (full segment context = better accuracy) |

## Future Enhancement

Combined with multitrack stereo recording, the retranscription feature can:
1. Split stereo into mic (L) and system (R) channels
2. Transcribe each channel independently
3. Merge with speaker labels (mic = "You", system = "Other")
