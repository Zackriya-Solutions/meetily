# Speaker Diarization and Editable Speaker Labels — Design

Date: 2026-09-27
Branch: `feat/speaker-diarization` (from `devtest`)

## 1. Goal

Transcripts say **who said what, per individual person**, even when several people share one
audio source (one microphone in a room, or many remote people on system audio). It works on new
recordings and on existing meetings.

### Success criteria

- A recording with 2+ people on **one mic** gives each person a distinct speaker key, and one
  person keeps the same key throughout the meeting.
- Multiple remote voices on system audio are separated.
- "Identify speakers" on an existing meeting labels it. Transcript text changes only on rows that
  are split because they contain a clear speaker change (§5.3).
- Re-transcribing or importing produces speaker-labelled rows; speakers are no longer wiped.
- Renaming a speaker updates every row in that meeting and survives reload. Other meetings are
  unaffected.
- Speakers can be merged, and a single row can be reassigned.
- Rows with a NULL speaker (legacy, or unlabelled) render exactly as today, with no chip.
- Summary input and "Copy transcript" use `[MM:SS] Name: text` lines.
- An explicit speaker count, when given, is honoured.
- Fully local inference. The only network use is the one-time model download.
- The recorded audio file is byte-for-byte unchanged.
- Wall-clock cost on a real ~30 min meeting is measured on CPU and CUDA builds. Target: ≤ 5 % of
  meeting duration on CPU.

### Out of scope (v1)

- Speaker chips during live recording (labels arrive after stop).
- Cross-meeting voice recognition. Centroids are stored so it can be added later (§4).
- A UI for the clustering threshold.
- GPU execution for diarization models (CPU is sufficient for their size).
- More than one speaker per transcript row (overlapping speech gets the dominant speaker).

## 2. Decisions

| Topic | Decision |
|---|---|
| When labels appear | Offline diarization, run automatically after each recording stops. Existing meetings via "Identify speakers", re-transcribe, or import. |
| Identity scope | Names are per meeting. Each speaker's voice centroid is stored for later cross-meeting use. |
| Re-run behaviour | New clusters are matched to the previous centroids. Confident matches keep their name, others become "Speaker N". Per-row reassignments are overwritten. |
| Corrections | Merge a speaker into another; reassign a single row. |
| Speaker count | Chosen per run (Auto or N) in the dialog. The automatic post-recording run uses Auto. |
| Models | Downloaded on first need. Setting "Identify speakers after recording" defaults on. Not bundled. |
| Mixed rows on Identify | Majority label. Rows with a clear mid-row change are split: only those pieces are re-transcribed on a fast engine (Parakeet or a GPU); otherwise the row's existing text is cut at the change (§5.2). |
| Engine | pyannote segmentation-3.0 + CAM++ embeddings + agglomerative clustering, in Rust on the existing `ort`. |
| Gate | New beta flag `speakerIdentification`, default on. |

Rejected: mic/system source attribution (one label per device, cannot separate people on one
source); `sherpa-rs` (deprecated, bundles a second onnxruntime that can clash with `ort`).

## 3. Engine (`src-tauri/src/diarization/`)

### 3.1 Models

Stored in `MODELS_DIR/diarization/`:

| File | Source | Size | Licence |
|---|---|---|---|
| `segmentation-3.0.onnx` | pyannote segmentation-3.0, ONNX export from the ungated `csukuangfj/sherpa-onnx-pyannote-segmentation-3-0` Hugging Face repo (`model.onnx`) | ~6 MB | MIT |
| `campplus-voxceleb.onnx` | `3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`, sherpa-onnx `speaker-recongition-models` release | ~28 MB | Apache-2.0 |

- Download writes to a `.part` file, verifies a pinned SHA-256, then renames. A partial or
  mismatched file never counts as installed.
- Sessions are built like `parakeet_engine/model.rs` (CPU execution provider), after
  `crate::ensure_onnx_runtime_available()`.
- The embedding model's ONNX metadata (`normalize_samples`, `feature_normalize_type`,
  `sample_rate`) is read at load and honoured.

### 3.2 Pipeline

Input: the meeting's stored audio decoded with `decode_audio_file` and converted to 16 kHz mono
(`to_whisper_format`). Output: `Diarization { turns: Vec<Turn>, speakers: Vec<SpeakerCentroid> }`
where `Turn { start_s, end_s, key }` and `SpeakerCentroid { key, embedding: Vec<f32> (the model's output size, 512 for CAM++ VoxCeleb),
speech_seconds }`.

1. **Segmentation** (`segmentation.rs`): 10 s windows with a 2.5 s step (constant, tuned by
   measurement). Each window yields per-frame scores (~17 ms frames) over 7 powerset classes,
   decoded to activity for up to 3 local speakers, including 2-speaker overlap.
2. **Features** (`fbank.rs`): Kaldi-compatible 80-bin log-mel fbank matching sherpa-onnx's CAM++ front end: mel range 20 Hz to
   Nyquist − 400 Hz (7600 Hz at 16 kHz), 25 ms frame, 10 ms shift, Povey window, pre-emphasis 0.97, DC removal, dither 0, `snip_edges = false`, per-utterance mean
   normalisation as the model metadata requires. Written by hand on `realfft`.
3. **Embeddings** (`embedding.rs`): for each (window, local speaker) with at least 0.5 s of
   non-overlapped activity, one CAM++ embedding over those frames. Shorter local speakers are
   skipped for clustering and resolved in reconstruction.
4. **Clustering** (`cluster.rs`): agglomerative, average linkage, cosine distance over
   L2-normalised embeddings. Auto: stop merging when the closest pair's cosine similarity drops
   below τ (constant, starts at 0.5, tuned on the reference set). Clusters carrying less than
   20 s of speech (summed over the overlapping windows, so roughly 5 s of actual speech) are
   crosstalk or noise: their embeddings join the nearest larger cluster's centroid. Given N: tighten
   τ in 0.05 steps (up to 0.95) until at least N clusters survive that folding, then merge the most
   similar centroids down to N. Without the folding, a real 62-minute meeting with 7 people
   produced 110 clusters at τ = 0.5 (71 of them single embeddings), and "N = 7" produced one
   giant cluster plus crumbs.
5. **Reconstruction** (`reconstruct.rs`): map each window's local speakers to global clusters,
   average per-frame activations across overlapping windows, pick the dominant speaker per frame,
   then merge into turns. Turns shorter than 0.3 s are absorbed into neighbours; gaps under 0.5 s
   between same-speaker turns are bridged.
6. **Keys**: clusters are ordered by first speech, giving `spk_0, spk_1, …`. Each centroid is the
   speech-weighted mean of its members' embeddings.

`diarizer.rs` exposes `diarize(samples, &DiarizeOptions { num_speakers: Option<usize> },
progress, cancel) -> Result<Diarization>`. It is pure: no DB, no Tauri. That keeps it
unit-testable.

### 3.3 Assignment (`assign.rs`)

- **Row labels**: each transcript row gets the speaker with the largest time overlap. A row with
  no overlap takes the nearest turn within 1 s, otherwise NULL.
- **Mixed-row detection**: a row is "mixed" when its second-largest speaker covers ≥ 1.5 s **and**
  ≥ 30 % of the row. Its split points are the turn boundaries inside it.
- **Carry-over**: greedy one-to-one matching of new centroids to the meeting's previous centroids
  by cosine similarity, highest first, accepting pairs ≥ 0.6. Matched speakers inherit
  `display_name`.

## 4. Data model

### 4.1 Migration `meeting_speakers`

```sql
CREATE TABLE IF NOT EXISTS meeting_speakers (
    meeting_id     TEXT NOT NULL,
    speaker_key    TEXT NOT NULL,
    display_name   TEXT,
    embedding      BLOB,
    speech_seconds REAL NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    PRIMARY KEY (meeting_id, speaker_key),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);
```

- `embedding`: little-endian `f32` values, one per model output dimension (512 for CAM++ VoxCeleb). NULL for a speaker created by manual reassignment.
- `display_name` NULL renders as "Speaker N" where N = key index + 1.
- `delete_meeting_with_transaction` also deletes from `meeting_speakers` (the codebase cascades
  manually).

### 4.2 Existing column

`transcripts.speaker` (migration `20251110000001`) holds the `speaker_key`. NULL = unlabelled.
The migration's comment describes the old mic/system meaning; the column is reused as-is.

### 4.3 Plumbing

- Add `speaker: Option<String>` to `database::models::Transcript`, `api::MeetingTranscript`,
  `api::TranscriptSegment`, and to every transcript INSERT: `TranscriptsRepository::save_transcript`,
  `audio/retranscription.rs`, `audio/import.rs`.
- `common::create_transcript_segments` takes the speaker per segment.
  `common::write_transcripts_json` writes `speaker` per segment and a top-level
  `speakers: { key: display_name }` map.

### 4.4 Operations (`database/repositories/speaker.rs`)

- `list(meeting_id)`, `rename(meeting_id, key, name)` (empty name resets to NULL).
- `merge(meeting_id, from, into)`: one transaction. Move rows from `from` to `into`, set
  `into`'s centroid to the speech-weighted mean of both, add speech seconds, delete `from`.
- `reassign_row(transcript_id, key | new)`: "new" creates the next free `spk_N` with a NULL
  embedding.
- `replace_for_meeting(tx, meeting_id, speakers, row_labels)`: used by every diarization write.

### 4.5 Preference

`identify_speakers_after_recording: bool` (serde default `true`) is added to the Rust-side
`RecordingPreferences`, so the backend decides on its own when a recording is saved.

## 5. Entry points

A single **job runner** runs one diarization job at a time, queued, cancellable. Events:
`diarization-progress { meeting_id, stage, percent, message }`, `diarization-complete
{ meeting_id, speaker_count }`, `diarization-error { meeting_id, error }`. A meeting that is being
re-transcribed cannot be diarized at the same time.

### 5.1 Automatic after recording

`api_save_transcript` is where both normal stop and crash recovery persist. After its commit,
if the preference is on and the meeting folder has audio, it enqueues an Identify job
(Auto count). If the models are missing, the download runs first. Failures are logged and
the meeting stays unlabelled. While a new recording is live, the automatic job waits before
decoding, diarizing and splitting rows (stage `waiting`), and diarization models use half the
cores, so identification does not compete with live transcription.

**Summary race**: the recording-stop flow auto-generates a summary. That generation waits for a
running diarization job on the same meeting (cap 120 s), then proceeds either way.

### 5.2 Identify speakers (manual)

1. Decode audio → `diarize` → assign rows.
2. For mixed rows (pieces under 1 s join a neighbour first):
   - **Fast engine** (Parakeet, or Whisper built with a GPU backend): cut the row's audio at the
     split points and re-transcribe each piece with the configured local engine (via the existing
     `get_or_init_*` helpers). whisper.cpp returns no text for input under 1 s, hence the 1 s
     floor.
   - **Otherwise, or when the engine cannot load or a piece fails or comes back empty**: split the
     row's existing text. Each cut goes where the speaker changes, estimated from the row's average
     speaking rate, moved to the nearest sentence end within 2 s, else the nearest clause mark
     within 1 s, else the nearest word gap (Chinese and Japanese text may be cut between
     characters). Whisper large-v3 on CPU pays a full 30 s window per piece: a 62-minute meeting
     with 26 mixed rows (85 pieces) would take tens of minutes, against milliseconds for the text
     split.
   - A row whose text cannot be cut into non-empty pieces stays whole with its majority label and
     counts toward the warning.
3. One transaction: update row speakers, replace split rows with their pieces, replace
   `meeting_speakers` (names carried over).
4. Rewrite `transcripts.json`, emit `diarization-complete`. The frontend refetches.

### 5.3 Re-transcribe and import

`start_retranscription_command` and the import command gain `identify_speakers: bool` and
`num_speakers: Option<u32>`. When on: diarize **before** transcription, then cut the VAD
segments at turn boundaries (pieces under 1 s join a neighbour), so each piece carries one
speaker into the INSERT. `meeting_speakers` is replaced in the same transaction as the rows,
with names carried over.

### 5.4 Commands

- Models: `diarization_models_status`, `diarization_download_models`,
  `diarization_delete_models`, plus `diarization-model-download-progress` events.
- Jobs: `start_speaker_identification(meeting_id, meeting_folder_path, num_speakers)`,
  `cancel_speaker_identification`, `get_speaker_identification_status(meeting_id)`.
- Speakers: `api_list_meeting_speakers`, `api_rename_meeting_speaker`,
  `api_merge_meeting_speakers`, `api_set_transcript_speaker`.

All are registered in `lib.rs`'s `invoke_handler!`.

## 6. UI

- **Transcript row**: a colour-coded chip (stable palette by key index) before the text, shown
  only when the speaker differs from the previous row. No chip when `speaker` is NULL.
- **Chip popover**: Rename (whole meeting); "Same person as…" (merge); "This line was said by…"
  (reassign this row to an existing speaker or a new one).
- **Speaker bar** above the transcript: each speaker with its share of speech time. Clicking a
  name renames it.
- **Job banner**: "Identifying speakers… N %" with Cancel while a job runs for this meeting.
- **Speakers button** in the transcript toolbar opens a dialog. It offers a count (Auto, 2–10) and
  Run. If the models are missing it offers an inline download (34 MB) with progress. If the
  meeting already has speakers it notes that names are kept where voices match.
- **Enhance and Import dialogs**: "Identify speakers" checkbox (default on) and count.
- **Settings → Recording**: toggle "Identify speakers after recording"; model status with size
  and Delete.
- **Beta flag** `speakerIdentification` (default on) gates the button, dialog options, chips'
  edit actions and the setting.
- **Text output**: `buildSummaryTranscriptPayload` and Copy transcript emit
  `[MM:SS] Name: text`. Rows without a speaker keep `[MM:SS] text`.
- Frontend pieces: `types` (`speaker` on `Transcript` / `TranscriptSegmentData`,
  `MeetingSpeaker`), `useMeetingSpeakers` hook, `useSpeakerIdentification` hook (job state from
  events), `SpeakerChip`, `SpeakerBar`, `IdentifySpeakersDialog`.

## 7. Error handling

- **Model download fails or is cancelled**: nothing is installed. The automatic run logs and
  skips; the dialog shows the error with Retry.
- **No audio / no speech / engine error**: `diarization-error` and a toast. The DB is untouched,
  because all writes happen in one final transaction.
- **Cancel**: checked between stages and between windows. Nothing is written.
- **ONNX Runtime unavailable**: clear error through `ensure_onnx_runtime_available`.
- **One speaker detected**: every row gets `spk_0`. Valid output, not an error.

## 8. Testing

- **Unit (no models)**: clustering (threshold and N), powerset decoding, reconstruction and
  smoothing, row assignment and the mixed-row rule, carry-over matching, merge centroid math,
  speaker repository operations against an in-memory SQLite with the real migrations.
- **Reference (`#[ignore]`, need `DIARIZATION_REF_DIR` with the models and sample wavs)**:
  - fbank matches Python `kaldi-native-fbank` output within 1e-3, configured with sherpa-onnx's
    options (low 20 Hz, high −400 Hz).
  - CAM++ embeddings match sherpa-onnx's (cosine ≥ 0.999).
  - Same-speaker cosine is high and cross-speaker cosine is low on the sherpa `sr-data` wavs.
  - End-to-end: a synthetic wav concatenated from several speakers gives the right speaker count
    and one key per voice.
- **Real meeting**: wall-clock time on a ~30 min meeting, on CPU and CUDA builds.
- **Gates**: `cargo test`, `cargo check`, `cargo build --release --features cuda`,
  `npx tsc --noEmit`.

Reference assets (models, wavs, a Python venv with `kaldi-native-fbank` and `sherpa-onnx`) live
outside the repository and are never committed.

## 9. Delivery

- Conventional commits on `feat/speaker-diarization`, pushed to the `fork` remote only. No PR
  until the feature is tested by the maintainer of this fork. The PR, when opened, targets
  `devtest`.
- `frontend/pnpm-lock.yaml` is not committed.
- Manual testing uses a build that also includes the local Linux fixes (system audio, CUDA libs)
  and an **isolated data directory**, so the new migration never reaches the installed app's
  database.

## 10. Known limits

- Overlapping speech gets one label per row.
- Turns under ~1 s are unreliable.
- Auto count can split or merge speakers; the explicit count and merge tool are the remedies.
- CAM++ voxceleb is English-centric.
- Labels are per meeting; the same person in two meetings has two unrelated keys.
- Peak memory while identifying speakers is roughly 1.7× the decoded 48 kHz audio (about 1.9 GB for a 2 h and 3.7 GB for a 4 h recording, the same cost retranscription already has); streaming decode is future work.
- The recording time map assumes live recordings are joined from 30 s AAC checkpoints; a 48 kHz AAC file of whole frames without import/retranscription metadata is treated as checkpoint-joined, so audio from app versions before checkpoints could have labels shifted by about 37 ms per 30 s.
