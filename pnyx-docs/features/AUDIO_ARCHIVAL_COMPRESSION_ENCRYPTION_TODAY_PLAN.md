# Plan: PCM-Safe Parallel Compression + Encrypted GCP Audio (Today Execution)

**Date:** Feb 13, 2026  
**Status:** Approved for same-day execution  
**Scope:** Keep live PCM transcription pipeline unchanged; reduce save-time latency; secure audio at rest and in retrieval.

## 1. Decision Summary
We will implement **Option 1**:
- Keep current PCM stream and chunk pipeline for live STT reliability.
- Run a parallel compressed audio path during recording.
- Store compressed audio in GCP as primary recording artifact.
- Encrypt recordings in GCP and enforce decrypt-on-arrival access flow.

## 2. Why this approach
- No disruption to existing real-time transcription behavior.
- Save-button latency drops drastically because heavy compression work is moved to recording time.
- Storage footprint and transfer cost drop significantly vs WAV-only archival.
- Security posture improves via key-managed encryption and controlled decryption path.

## 3. Time Savings Estimate (3-hour meeting)

### Current model (compress after Save)
- Typical post-save compression time: ~8 to 25 minutes on 2 vCPU backend.

### New model (parallel encode during recording)
- Post-save time mostly finalize + upload metadata: ~10 to 60 seconds.

### Net impact
- Save-time reduction: ~95% to 99%.

## 4. Current PCM Dependency Audit (keep untouched)
- Browser audio worklet sends PCM frames: `frontend/public/audio-processor.worklet.js`
- WebSocket stream ingestion: `backend/app/api/routers/audio.py`
- Live transcription manager consumes PCM: `backend/app/services/audio/manager.py`
- Groq client expects PCM and wraps into WAV in-memory: `backend/app/services/audio/groq_client.py`
- PCM chunk recorder persists chunks: `backend/app/services/audio/recorder.py`

## 5. New Architecture (incremental)

### 5.1 Parallel compression path
- Add a long-lived encoder process per active recording.
- Feed incoming PCM bytes to encoder continuously.
- Produce rolling output file: `recording.opus` (preferred) or `recording.m4a` fallback.
- On stop: close stdin, finalize container, upload final compressed object.

### 5.2 Storage layout (target)
- Primary archival: `{meeting_id}/recording.opus`
- Optional compatibility artifact: `{meeting_id}/recording.wav` (temporary during migration)
- Keep PCM chunks only as short-lived technical artifacts until pipeline stabilizes.

### 5.3 Retrieval strategy
- Preferred playback/inference source: compressed artifact.
- Fallback chain: `recording.opus -> recording.wav -> transcript_only`.
- Diarization can continue using WAV until provider path is updated.

## 6. Encryption Plan

### Stage A (today): GCS CMEK (mandatory)
- Configure bucket/object encryption with Cloud KMS key.
- Restrict KMS decrypt IAM to backend service account only.
- Ensure all new uploads for meeting recordings use CMEK.

### Stage B (today, if time permits): App-layer envelope encryption
- Generate per-meeting DEK.
- Encrypt audio payload with AES-256-GCM before upload.
- Encrypt DEK via Cloud KMS KEK; store wrapped DEK + nonce/auth tag metadata.
- On authorized read, backend decrypts and streams audio.

### Stage C (if Stage B deferred): harden access immediately
- Disable broad signed URL sharing for sensitive recordings.
- Route download/play through authenticated backend endpoint with RBAC.

## 7. Browser Input Format Notes
- Keep AudioWorklet PCM for low-latency STT stability.
- Optional future enhancement: add parallel MediaRecorder compressed stream from browser.
- Do not replace PCM websocket path until latency parity is proven.

## 8. Merge/Finalize Notes for Compressed Chunks
- If producing one continuous encoder output, no expensive merge is needed at stop.
- If chunked compressed files are produced, prefer concat/remux with `-c copy` (fast) over re-encode.
- Fast concat/remux for 3h recording is usually I/O bound (~1 to 4 minutes), much faster than full re-encode.

## 9. Today-Only Execution Timeline (Feb 13, 2026)

1. **Hour 1-2**: Implement parallel encoder service hooks in recorder lifecycle.
2. **Hour 2-3**: Persist/upload compressed artifact (`recording.opus`) and metadata.
3. **Hour 3-4**: Update recording retrieval endpoint to prefer compressed asset.
4. **Hour 4-5**: Enable CMEK + IAM policy checks for recording uploads.
5. **Hour 5-6**: Add RBAC-guarded decrypt/read endpoint (or signed URL hardening if envelope encryption is deferred).
6. **Hour 6-7**: Compatibility tests for notes generation + diarization fallback paths.
7. **Hour 7-8**: Load-test save flow (long recording simulation), finalize rollout toggles.

## 10. Config and Flags
- `AUDIO_ARCHIVE_FORMAT=opus`
- `AUDIO_PARALLEL_ENCODING_ENABLED=true`
- `AUDIO_PREFER_COMPRESSED_READ=true`
- `AUDIO_CMEK_ENABLED=true`
- `AUDIO_ENVELOPE_ENCRYPTION_ENABLED` (optional, stage-dependent)

## 11. Definition of Done (Today)
- Save action no longer blocks on long post-recording compression.
- New meetings store compressed recording artifact in GCP.
- Recording reads prefer compressed asset and preserve fallback behavior.
- Encryption controls are active (minimum CMEK + IAM).
- Notes generation and existing PCM transcription continue without regressions.
