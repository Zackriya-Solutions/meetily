# Multitrack Audio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add stereo recording mode (mic=L, system=R) as opt-in advanced option while keeping mono as default.

**Architecture:** Fork the recording path in the audio pipeline — STEP 4 sends either interleaved stereo or mixed mono to the incremental saver based on a user preference. The STT path (VAD -> Whisper/Parakeet) is completely untouched.

**Tech Stack:** Rust (Tauri backend), TypeScript/React (frontend settings), AAC/MP4 encoding via FFmpeg

**Spec:** `docs/superpowers/specs/2026-03-19-multitrack-audio-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `frontend/src-tauri/src/audio/recording_state.rs` | Modify | Add `channels: u16` to `AudioChunk` |
| `frontend/src-tauri/src/audio/recording_preferences.rs` | Modify | Add `RecordingMode` enum and preference field |
| `frontend/src-tauri/src/audio/pipeline.rs` | Modify | `interleave_stereo_into()`, pre-alloc buffer, STEP 4 fork |
| `frontend/src-tauri/src/audio/incremental_saver.rs` | Modify | Accept `channels`, fix frame math, pass to encoder |
| `frontend/src-tauri/src/audio/recording_saver.rs` | Modify | Propagate channels/mode to saver and metadata |
| `frontend/src-tauri/src/audio/recording_commands.rs` | Modify | Extract `RecordingMode` from preferences, pass to manager |
| `frontend/src-tauri/src/audio/recording_manager.rs` | Modify | Accept `RecordingMode` parameter, pass to pipeline+saver |
| `frontend/src-tauri/src/audio/encode.rs` | Modify | Stereo bitrate (256kbps) |
| `frontend/src/components/RecordingSettings.tsx` | Modify | Add stereo toggle |

**Note:** No new Tauri commands needed. The existing `set_recording_preferences` command already saves the full `RecordingPreferences` struct including the new `recording_mode` field.

---

## Task 1: Add `channels` field to `AudioChunk`

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_state.rs:18-25`
- Modify: `frontend/src-tauri/src/audio/pipeline.rs` (all AudioChunk construction sites)
- Modify: `frontend/src-tauri/src/audio/incremental_saver.rs:437` (test)

- [ ] **Step 1: Add `channels: u16` to AudioChunk struct**

In `recording_state.rs`, add the field after `sample_rate`:

```rust
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub timestamp: f64,
    pub chunk_id: u64,
    pub device_type: DeviceType,
}
```

- [ ] **Step 2: Fix all AudioChunk construction sites**

The Rust compiler will flag every site. Add `channels: 1` to each:

| File | Line | Context |
|------|------|---------|
| `pipeline.rs:610` | AudioCapture raw chunk | `channels: 1` |
| `pipeline.rs:844` | Transcription chunk post-VAD | `channels: 1` |
| `pipeline.rs:870` | Recording chunk (will change in Task 4) | `channels: 1` for now |
| `pipeline.rs:914` | Flush transcription | `channels: 1` |
| `pipeline.rs:1036` | Flush signal | `channels: 1` |
| `pipeline.rs:1056` | Additional flush signals | `channels: 1` |
| `incremental_saver.rs:437` | Test | `channels: 1` |

- [ ] **Step 3: Verify it compiles**

Run: `cd frontend && cargo check 2>&1 | head -30`
Expected: Compilation succeeds with no errors related to AudioChunk.

- [ ] **Step 4: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_state.rs frontend/src-tauri/src/audio/pipeline.rs frontend/src-tauri/src/audio/incremental_saver.rs
git commit -m "feat(audio): add channels field to AudioChunk struct"
```

---

## Task 2: Add `RecordingMode` to preferences

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs:14-26`

- [ ] **Step 1: Add RecordingMode enum and preference field**

In `recording_preferences.rs`, add the enum before `RecordingPreferences` and a new field:

```rust
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum RecordingMode {
    Mono,
    Stereo,
}

impl Default for RecordingMode {
    fn default() -> Self {
        RecordingMode::Mono
    }
}

impl RecordingMode {
    pub fn channels(&self) -> u16 {
        match self {
            RecordingMode::Mono => 1,
            RecordingMode::Stereo => 2,
        }
    }
}
```

Add to `RecordingPreferences` struct:

```rust
#[serde(default)]
pub recording_mode: RecordingMode,
```

- [ ] **Step 2: Verify it compiles**

Run: `cd frontend && cargo check 2>&1 | head -30`
Expected: Compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_preferences.rs
git commit -m "feat(audio): add RecordingMode enum to recording preferences"
```

---

## Task 3: Update IncrementalAudioSaver for multi-channel

**Files:**
- Modify: `frontend/src-tauri/src/audio/incremental_saver.rs:19-50, 54-75, 78-111`

- [ ] **Step 1: Write failing test for stereo checkpoint interval**

Add to the `tests` module at the bottom of `incremental_saver.rs`:

```rust
#[tokio::test]
async fn test_stereo_checkpoint_interval() {
    let temp_dir = tempdir().unwrap();
    let meeting_folder = temp_dir.path().join("Stereo_Test");
    std::fs::create_dir_all(&meeting_folder).unwrap();
    std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();

    // Create saver with 2 channels (stereo)
    let mut saver = IncrementalAudioSaver::new(
        meeting_folder.clone(),
        48000,
        2, // stereo
    ).unwrap();

    // Add 15 seconds of stereo audio (30s of mono-equivalent samples)
    // 15s * 48000 Hz * 2 channels = 1,440,000 samples
    // This should NOT trigger a checkpoint (15s real audio, not 30s)
    for i in 0..30 {
        let chunk = AudioChunk {
            data: vec![0.5f32; 48000],  // 0.5s of stereo data (24000 frames)
            sample_rate: 48000,
            channels: 2,
            timestamp: i as f64 * 0.5,
            chunk_id: i as u64,
            device_type: DeviceType::Microphone,
        };
        saver.add_chunk(chunk).unwrap();
    }

    // Should be 0 checkpoints -- 15s of stereo, not 30s
    assert_eq!(saver.get_checkpoint_count(), 0,
        "Stereo checkpoint should use frame count (15s), not sample count (30s)");
}

#[tokio::test]
async fn test_stereo_duration_calculation() {
    let temp_dir = tempdir().unwrap();
    let meeting_folder = temp_dir.path().join("Stereo_Duration_Test");
    std::fs::create_dir_all(&meeting_folder).unwrap();
    std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();

    let mut saver = IncrementalAudioSaver::new(
        meeting_folder.clone(),
        48000,
        2, // stereo
    ).unwrap();

    // Add exactly 30 seconds of stereo audio to trigger one checkpoint
    // 30s * 48000 Hz * 2 channels = 2,880,000 samples
    for i in 0..60 {
        let chunk = AudioChunk {
            data: vec![0.5f32; 48000],  // 0.5s stereo
            sample_rate: 48000,
            channels: 2,
            timestamp: i as f64 * 0.5,
            chunk_id: i as u64,
            device_type: DeviceType::Microphone,
        };
        saver.add_chunk(chunk).unwrap();
    }

    // Should be exactly 1 checkpoint (30s of stereo audio)
    assert_eq!(saver.get_checkpoint_count(), 1,
        "30s of stereo audio should produce exactly 1 checkpoint");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && cargo test --lib test_stereo -- --nocapture 2>&1 | tail -20`
Expected: FAIL because `IncrementalAudioSaver::new` doesn't accept `channels` parameter yet.

- [ ] **Step 3: Add `channels` field to IncrementalAudioSaver**

Modify the struct (line 19-26):

```rust
pub struct IncrementalAudioSaver {
    checkpoint_buffer: Vec<AudioData>,
    /// Checkpoint threshold in raw samples (inclusive of channels).
    /// For stereo: sample_rate * 30 * 2. For mono: sample_rate * 30.
    /// Compared directly against sum of data.len() from chunks.
    checkpoint_interval_samples: usize,
    checkpoint_count: u32,
    checkpoints_dir: PathBuf,
    meeting_folder: PathBuf,
    sample_rate: u32,
    channels: u16,
}
```

Modify constructor (line 34) to accept `channels: u16`:

```rust
pub fn new(meeting_folder: PathBuf, sample_rate: u32, channels: u16) -> Result<Self> {
    let checkpoints_dir = meeting_folder.join(".checkpoints");
    if !checkpoints_dir.exists() {
        return Err(anyhow!("Checkpoints directory does not exist: {}", checkpoints_dir.display()));
    }

    Ok(Self {
        checkpoint_buffer: Vec::new(),
        checkpoint_interval_samples: sample_rate as usize * 30 * channels as usize,
        checkpoint_count: 0,
        checkpoints_dir,
        meeting_folder,
        sample_rate,
        channels,
    })
}
```

- [ ] **Step 4: Fix duration calculation in save_checkpoint**

Modify `save_checkpoint` (line 103):

```rust
let duration_seconds = audio_data.len() as f32 / (self.sample_rate as f32 * self.channels as f32);
```

- [ ] **Step 5: Pass channels to encode_single_audio**

Modify the encode call (line 96-101):

```rust
encode_single_audio(
    bytemuck::cast_slice(&audio_data),
    self.sample_rate,
    self.channels,
    &checkpoint_path
)?;
```

- [ ] **Step 6: Fix existing tests to pass channels=1**

Update `test_checkpoint_creation` (line 430) and `test_empty_recording` (line 465) — change constructor call:

```rust
let mut saver = IncrementalAudioSaver::new(
    meeting_folder.clone(),
    48000,
    1, // mono
).unwrap();
```

- [ ] **Step 7: Fix caller in recording_saver.rs (temporary hardcode)**

In `recording_saver.rs:239`, update the `IncrementalAudioSaver::new` call. Use `1` for now (will be wired to preference in Task 6):

```rust
let incremental_saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000, 1)?;
```

- [ ] **Step 8: Run tests**

Run: `cd frontend && cargo test --lib incremental_saver -- --nocapture 2>&1 | tail -30`
Expected: All 4 tests pass (existing 2 + 2 new stereo tests).

- [ ] **Step 9: Commit**

```bash
git add frontend/src-tauri/src/audio/incremental_saver.rs frontend/src-tauri/src/audio/recording_saver.rs
git commit -m "feat(audio): add multi-channel support to IncrementalAudioSaver

Frame-based checkpoint math ensures stereo checkpoints trigger at
correct 30s intervals. Includes tests for stereo checkpoint timing
and duration calculation."
```

---

## Task 4: Add interleave function and fork STEP 4 in pipeline

**Files:**
- Modify: `frontend/src-tauri/src/audio/pipeline.rs:680-700, 747-763, 868-878, 958-969`

- [ ] **Step 1: Add interleave_stereo_into function**

Add at the top of `pipeline.rs`, after the imports:

```rust
use super::recording_preferences::RecordingMode;

/// Interleave two mono buffers into stereo [L0, R0, L1, R1, ...]
/// Writes into a pre-allocated buffer to avoid per-window allocation.
fn interleave_stereo_into(left: &[f32], right: &[f32], out: &mut Vec<f32>) {
    debug_assert_eq!(left.len(), right.len(), "Stereo interleave requires equal-length buffers");
    out.reserve(left.len() * 2);
    for (&l, &r) in left.iter().zip(right.iter()) {
        out.push(l);
        out.push(r);
    }
}
```

- [ ] **Step 2: Add RecordingMode and interleave buffer to AudioPipeline struct**

Add fields to `AudioPipeline` struct (after `recording_sender_for_mixed` ~line 696):

```rust
recording_mode: RecordingMode,
interleave_buffer: Vec<f32>,
```

- [ ] **Step 3: Update AudioPipeline::new() to accept RecordingMode**

Add `recording_mode: RecordingMode` parameter to `AudioPipeline::new()` (line 700). Initialize the new fields in the constructor return (line 747-763):

```rust
recording_mode,
interleave_buffer: Vec::with_capacity(48000),  // Pre-allocate ~1s
```

- [ ] **Step 4: Modify STEP 4 to fork based on RecordingMode**

Replace lines 868-878:

```rust
// STEP 4: Send audio for recording
if let Some(ref sender) = self.recording_sender_for_mixed {
    let (recording_data, rec_channels) = match self.recording_mode {
        RecordingMode::Stereo => {
            self.interleave_buffer.clear();
            interleave_stereo_into(&mic_window, &sys_window, &mut self.interleave_buffer);
            (self.interleave_buffer.clone(), 2u16)
        },
        RecordingMode::Mono => (mixed_with_gain.clone(), 1u16),
    };
    let recording_chunk = AudioChunk {
        data: recording_data,
        sample_rate: self.sample_rate,
        channels: rec_channels,
        timestamp: chunk.timestamp,
        chunk_id: self.chunk_id_counter,
        device_type: DeviceType::Microphone,
    };
    let _ = sender.send(recording_chunk);
}
```

Note: `recording_sender_for_mixed` continues to be set externally at line 996 (`pipeline.recording_sender_for_mixed = recording_sender;`). This is unchanged.

- [ ] **Step 5: Update AudioPipelineManager::start() signature**

Add `recording_mode: RecordingMode` parameter to `AudioPipelineManager::start()` (line 958-969). Pass it to `AudioPipeline::new()`:

```rust
pub fn start(
    &mut self,
    state: Arc<RecordingState>,
    transcription_sender: mpsc::UnboundedSender<AudioChunk>,
    target_chunk_duration_ms: u32,
    sample_rate: u32,
    recording_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
    mic_device_name: String,
    mic_device_kind: super::device_detection::InputDeviceKind,
    system_device_name: String,
    system_device_kind: super::device_detection::InputDeviceKind,
    recording_mode: RecordingMode,
) -> Result<()> {
```

And in the `AudioPipeline::new()` call (~line 982), pass `recording_mode`.

- [ ] **Step 6: Add temporary default at call site to keep it compiling**

In `recording_manager.rs`, where `pipeline_manager.start()` is called (~line 109-119), add `RecordingMode::Mono` as the last argument temporarily:

```rust
use super::recording_preferences::RecordingMode;

// ... in start_recording():
self.pipeline_manager.start(
    self.state.clone(),
    transcription_sender,
    0,
    48000,
    Some(recording_sender),
    mic_name,
    mic_kind,
    sys_name,
    sys_kind,
    RecordingMode::Mono,  // Temporary default, wired to preferences in Task 6
)?;
```

- [ ] **Step 7: Verify it compiles**

Run: `cd frontend && cargo check 2>&1 | head -30`
Expected: Compiles successfully.

- [ ] **Step 8: Commit**

```bash
git add frontend/src-tauri/src/audio/pipeline.rs frontend/src-tauri/src/audio/recording_manager.rs
git commit -m "feat(audio): add stereo interleave and recording mode fork in pipeline

STEP 4 now produces interleaved stereo or mixed mono based on
RecordingMode. Uses pre-allocated buffer for zero-alloc hot path."
```

---

## Task 5: Update MeetingMetadata with channel fields

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs:28-41, 247-262`

- [ ] **Step 1: Add channels and recording_mode to MeetingMetadata struct**

In `recording_saver.rs`, update `MeetingMetadata` struct (line 28-41):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMetadata {
    pub version: String,
    pub meeting_id: Option<String>,
    pub meeting_name: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<f64>,
    pub devices: DeviceInfo,
    pub audio_file: String,
    pub transcript_file: String,
    pub sample_rate: u32,
    #[serde(default = "default_channels")]
    pub channels: u16,
    #[serde(default = "default_recording_mode")]
    pub recording_mode: String,
    pub status: String,
}

fn default_channels() -> u16 { 1 }
fn default_recording_mode() -> String { "mono".to_string() }
```

- [ ] **Step 2: Update metadata creation in initialize_meeting_folder**

Where `MeetingMetadata` is constructed (~line 247-262), add the new fields:

```rust
channels: 1,  // Will be updated in Task 6 when wired to preference
recording_mode: "mono".to_string(),
```

- [ ] **Step 3: Write backward compatibility tests**

Add a test module to `recording_saver.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_old_metadata_deserializes_with_defaults() {
        let old_json = r#"{
            "version": "1.0",
            "meeting_id": null,
            "meeting_name": "Test",
            "created_at": "2025-01-01T00:00:00Z",
            "completed_at": null,
            "duration_seconds": null,
            "devices": { "microphone": null, "system_audio": null },
            "audio_file": "audio.mp4",
            "transcript_file": "transcripts.json",
            "sample_rate": 48000,
            "status": "completed"
        }"#;

        let metadata: MeetingMetadata = serde_json::from_str(old_json).unwrap();
        assert_eq!(metadata.channels, 1);
        assert_eq!(metadata.recording_mode, "mono");
    }

    #[test]
    fn test_new_metadata_deserializes_stereo() {
        let new_json = r#"{
            "version": "1.0",
            "meeting_id": null,
            "meeting_name": "Test",
            "created_at": "2025-01-01T00:00:00Z",
            "completed_at": null,
            "duration_seconds": null,
            "devices": { "microphone": null, "system_audio": null },
            "audio_file": "audio.mp4",
            "transcript_file": "transcripts.json",
            "sample_rate": 48000,
            "channels": 2,
            "recording_mode": "stereo",
            "status": "completed"
        }"#;

        let metadata: MeetingMetadata = serde_json::from_str(new_json).unwrap();
        assert_eq!(metadata.channels, 2);
        assert_eq!(metadata.recording_mode, "stereo");
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd frontend && cargo test --lib recording_saver::tests -- --nocapture 2>&1 | tail -20`
Expected: Both tests pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_saver.rs
git commit -m "feat(audio): add channels and recording_mode to MeetingMetadata

Serde defaults ensure old metadata.json files without these fields
deserialize correctly as mono/1-channel."
```

---

## Task 6: Wire RecordingMode through commands, manager, and saver

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs:113-119, 233-236`
- Modify: `frontend/src-tauri/src/audio/recording_manager.rs:63-77, 109-119`
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs:50-58, 140-149, 230-244`

- [ ] **Step 1: Extract RecordingMode in recording_commands.rs**

In `recording_commands.rs`, where preferences are loaded (~line 113-119), also extract `recording_mode`:

```rust
let (auto_save, preferred_mic_name, preferred_system_name, recording_mode) =
    match super::recording_preferences::load_recording_preferences(&app).await {
        Ok(prefs) => {
            info!("📋 Loaded recording preferences: auto_save={}, recording_mode={:?}",
                  prefs.auto_save, prefs.recording_mode);
            (prefs.auto_save, prefs.preferred_mic_device, prefs.preferred_system_device, prefs.recording_mode)
        }
        Err(e) => {
            warn!("Failed to load recording preferences, using defaults: {}", e);
            (true, None, None, RecordingMode::Mono)
        }
    };
```

Add import at top: `use super::recording_preferences::RecordingMode;`

- [ ] **Step 2: Pass RecordingMode to start_recording**

In `recording_commands.rs` (~line 233-236), pass `recording_mode`:

```rust
let transcription_receiver = manager
    .start_recording(microphone_device, system_device, auto_save, recording_mode)
    .await
    .map_err(|e| format!("Failed to start recording: {}", e))?;
```

- [ ] **Step 3: Update RecordingManager::start_recording signature**

In `recording_manager.rs`, add `recording_mode: RecordingMode` parameter to `start_recording()`:

```rust
pub async fn start_recording(
    &mut self,
    microphone_device: Option<Arc<AudioDevice>>,
    system_device: Option<Arc<AudioDevice>>,
    auto_save: bool,
    recording_mode: RecordingMode,
) -> Result<mpsc::UnboundedReceiver<AudioChunk>> {
```

Calculate channels and log:
```rust
let channels = recording_mode.channels();
info!("📼 Recording mode: {:?} ({} channels)", recording_mode, channels);
```

- [ ] **Step 4: Replace temporary RecordingMode::Mono in pipeline call**

Update the `pipeline_manager.start()` call (~line 109-119) to use the actual `recording_mode`:

```rust
self.pipeline_manager.start(
    self.state.clone(),
    transcription_sender,
    0,
    48000,
    Some(recording_sender),
    mic_name,
    mic_kind,
    sys_name,
    sys_kind,
    recording_mode.clone(),
)?;
```

- [ ] **Step 5: Add channels field to RecordingSaver and wire through**

Add `channels: u16` field to `RecordingSaver` struct (~line 50-58):

```rust
pub struct RecordingSaver {
    incremental_saver: Option<Arc<AsyncMutex<IncrementalAudioSaver>>>,
    meeting_folder: Option<PathBuf>,
    meeting_name: Option<String>,
    metadata: Option<MeetingMetadata>,
    transcript_segments: Arc<Mutex<Vec<TranscriptSegment>>>,
    chunk_receiver: Option<mpsc::UnboundedReceiver<AudioChunk>>,
    is_saving: Arc<Mutex<bool>>,
    channels: u16,
}
```

Initialize in `new()`: `channels: 1,`

Add `channels: u16` parameter to `start_accumulation()` (~line 140):

```rust
pub fn start_accumulation(&mut self, auto_save: bool, channels: u16) -> mpsc::UnboundedSender<AudioChunk> {
    self.channels = channels;
```

Pass to `initialize_meeting_folder`:

```rust
fn initialize_meeting_folder(&mut self, meeting_name: &str, create_checkpoints: bool) -> Result<()> {
```

Update `IncrementalAudioSaver::new` call (line 239):

```rust
let incremental_saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000, self.channels)?;
```

Update metadata creation to use `self.channels`:

```rust
channels: self.channels,
recording_mode: if self.channels == 2 { "stereo".to_string() } else { "mono".to_string() },
```

- [ ] **Step 6: Update recording_manager to pass channels to start_accumulation**

In `recording_manager.rs` (~line 77):

```rust
let recording_sender = self.recording_saver.start_accumulation(auto_save, channels);
```

- [ ] **Step 7: Verify it compiles**

Run: `cd frontend && cargo check 2>&1 | head -30`
Expected: Compiles successfully.

- [ ] **Step 8: Run all tests**

Run: `cd frontend && cargo test --lib 2>&1 | tail -30`
Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_commands.rs frontend/src-tauri/src/audio/recording_manager.rs frontend/src-tauri/src/audio/recording_saver.rs
git commit -m "feat(audio): wire RecordingMode from preferences through full recording path

RecordingMode flows: preferences -> recording_commands -> recording_manager
-> pipeline + saver -> incremental_saver -> encoder. Replaces temporary
Mono default with actual user preference."
```

---

## Task 7: Adjust AAC bitrate for stereo

**Files:**
- Modify: `frontend/src-tauri/src/audio/encode.rs:50`

- [ ] **Step 1: Accept dynamic bitrate based on channels**

The `encode_single_audio` function already receives `channels`. Update the bitrate line (line 50):

```rust
let bitrate = if channels >= 2 { "256k" } else { "192k" };
```

Then use `bitrate` variable in the FFmpeg args instead of the hardcoded `"192k"`.

- [ ] **Step 2: Verify it compiles**

Run: `cd frontend && cargo check 2>&1 | head -30`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/audio/encode.rs
git commit -m "feat(audio): increase AAC bitrate to 256kbps for stereo recording"
```

---

## Task 8: Add frontend settings toggle

**Files:**
- Modify: `frontend/src/components/RecordingSettings.tsx:9-15, 71-80, 165-177`

- [ ] **Step 1: Add recording_mode to RecordingPreferences interface**

In `RecordingSettings.tsx`, update the TypeScript interface (line 9-15):

```typescript
export interface RecordingPreferences {
  save_folder: string;
  auto_save: boolean;
  file_format: string;
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
  recording_mode: 'mono' | 'stereo';
}
```

- [ ] **Step 2: Add default value in preference loading**

Where preferences are loaded/defaulted, ensure `recording_mode` defaults to `'mono'`.

- [ ] **Step 3: Add toggle handler**

Add after the `handleAutoSaveToggle` function:

```typescript
const handleRecordingModeToggle = async (stereo: boolean) => {
    const mode = stereo ? 'stereo' : 'mono';
    const newPreferences = { ...preferences, recording_mode: mode };
    setPreferences(newPreferences);
    await savePreferences(newPreferences);
};
```

- [ ] **Step 4: Add toggle UI**

Add after the auto-save toggle section (~line 177):

```tsx
<div className="flex items-center justify-between">
    <div className="space-y-0.5">
        <Label>Separate audio tracks (advanced)</Label>
        <p className="text-sm text-muted-foreground">
            Record microphone and system audio on separate stereo channels (left/right).
            Useful for speaker diarization and post-processing.
        </p>
    </div>
    <Switch
        checked={preferences.recording_mode === 'stereo'}
        onCheckedChange={handleRecordingModeToggle}
    />
</div>
```

- [ ] **Step 5: Verify frontend compiles**

Run: `cd frontend && pnpm run lint 2>&1 | tail -20`
Expected: No errors.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/RecordingSettings.tsx
git commit -m "feat(ui): add stereo recording toggle in settings

Advanced option to record mic and system audio on separate stereo
channels. Default: OFF (mono)."
```

---

## Task 9: Add unit tests for interleave function

**Files:**
- Modify: `frontend/src-tauri/src/audio/pipeline.rs` (add tests module)

- [ ] **Step 1: Write tests for interleave_stereo_into**

Add at the bottom of `pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interleave_stereo_basic() {
        let left = vec![1.0, 2.0, 3.0];
        let right = vec![4.0, 5.0, 6.0];
        let mut out = Vec::new();
        interleave_stereo_into(&left, &right, &mut out);
        assert_eq!(out, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn test_interleave_stereo_empty() {
        let left: Vec<f32> = vec![];
        let right: Vec<f32> = vec![];
        let mut out = Vec::new();
        interleave_stereo_into(&left, &right, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn test_interleave_stereo_reuse_buffer() {
        let mut out = Vec::with_capacity(100);
        let left = vec![1.0, 2.0];
        let right = vec![3.0, 4.0];

        interleave_stereo_into(&left, &right, &mut out);
        assert_eq!(out, vec![1.0, 3.0, 2.0, 4.0]);

        // Reuse: clear and interleave again
        out.clear();
        let left2 = vec![5.0];
        let right2 = vec![6.0];
        interleave_stereo_into(&left2, &right2, &mut out);
        assert_eq!(out, vec![5.0, 6.0]);
    }

    #[test]
    #[should_panic(expected = "equal-length")]
    #[cfg(debug_assertions)]
    fn test_interleave_stereo_mismatched_panics_in_debug() {
        let left = vec![1.0, 2.0];
        let right = vec![3.0];
        let mut out = Vec::new();
        interleave_stereo_into(&left, &right, &mut out);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && cargo test --lib pipeline::tests -- --nocapture 2>&1 | tail -20`
Expected: All 4 tests pass (3 normal + 1 should_panic in debug mode).

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/audio/pipeline.rs
git commit -m "test(audio): add unit tests for stereo interleave function"
```

---

## Task 10: Final integration check

- [ ] **Step 1: Full cargo check**

Run: `cd frontend && cargo check 2>&1 | tail -30`
Expected: Clean compilation.

- [ ] **Step 2: Run all Rust tests**

Run: `cd frontend && cargo test --lib 2>&1 | tail -30`
Expected: All tests pass.

- [ ] **Step 3: Verify no regressions in frontend lint**

Run: `cd frontend && pnpm run lint 2>&1 | tail -20`
Expected: No new lint errors.

- [ ] **Step 4: Final commit (if any fixes needed)**

```bash
git commit -m "chore: fix integration issues from multitrack audio feature"
```
