# Audio Pipeline Stabilization Plan (Celery)

**Status:** PLANNED  
**Branch:** `feature/audio-pipeline-celery-plan`  
**Objective:** Eliminate missing/partial meeting audio and improve reliability for back-to-back recordings and long meetings (15 min to 3 hr).

---

## 1) Why This Is Needed

Current issues observed in production/local testing:

- Meeting audio occasionally saves partially (for example 2-3 minutes from a 15-minute recording).
- Some recordings vanish after consecutive meetings.
- Chunk upload/finalize lifecycle is not fully resilient to stop/start races and retries.

Root causes are likely:

- In-process async tasks can be interrupted or overlap under rapid session transitions.
- Finalize is not fully guarded as an idempotent, single-flight operation.
- Chunk durability is not verified deterministically before finalize.

---

## 2) Target Architecture

Use **Celery + Redis** to move heavy and failure-prone operations out of request/WS lifecycle.

### 2.1 Core Components

1. API/WS layer (FastAPI)
- Handles recording stream ingress and user commands.
- Writes session/chunk metadata to DB.
- Enqueues Celery tasks; does not perform heavy long-running finalize/processing inline.

2. Celery workers
- `audio_upload_chunk` queue: durable chunk upload to storage.
- `audio_finalize_session` queue: one idempotent finalize per session.
- `audio_postprocess` queue: diarization/transcript enrichment after finalize.

3. Redis broker + result backend
- Reliable task delivery/retry handling.

4. Database state machine + manifests
- Session lifecycle and per-chunk durability tracking.

---

## 3) Session State Machine

Introduce/standardize recording states:

- `recording`
- `stopping_requested`
- `uploading_chunks`
- `finalizing`
- `postprocessing`
- `completed`
- `failed`

Rules:

- Allow only valid forward transitions.
- Enforce single active session lock per user (or per device key).
- Block new recording start while previous session is in stop/finalize critical states unless explicitly overridden.

---

## 4) Data Model Additions

### 4.1 `recording_sessions` (new)

Suggested fields:

- `session_id` (PK)
- `user_email`
- `meeting_id`
- `status`
- `started_at`, `stopped_at`, `updated_at`
- `expected_chunk_count`
- `finalized_chunk_count`
- `error_code`, `error_message`
- `idempotency_finalize_key`

### 4.2 `recording_chunks` (new)

Suggested fields:

- `session_id`
- `chunk_index`
- `byte_size`
- `checksum`
- `storage_path`
- `uploaded_at`
- `upload_status` (`pending|uploaded|failed`)
- PK: (`session_id`, `chunk_index`)

### 4.3 `recording_jobs` (optional)

If needed for auditability beyond Celery metadata:

- `job_type`, `session_id`, `state`, `attempt`, `last_error`, `created_at`, `updated_at`

---

## 5) Celery Task Design

### 5.1 `audio_upload_chunk(session_id, chunk_index, payload_ref)`

- Validates chunk ownership/session state.
- Uploads to storage (GCP/local) with checksum verification.
- Marks chunk `uploaded`.
- Safe to retry (idempotent by session+chunk index).

### 5.2 `audio_finalize_session(session_id)`

- Acquires distributed lock (`finalize:{session_id}`).
- Verifies all required chunks are uploaded or marks failure with diagnostics.
- Merges chunks in deterministic order.
- Writes final artifact + duration metadata.
- Marks session `finalizing -> postprocessing` (or `completed` if postprocessing off).
- Idempotent: repeated calls should no-op if already finalized.

### 5.3 `audio_postprocess(session_id)`

- Triggers diarization/alignment asynchronously.
- Supports chunked processing for long audio.
- Updates final transcript/notes artifacts.
- Marks `completed` or `failed`.

---

## 6) Stop/Finalize Contract (Critical)

When user clicks stop:

1. API marks session `stopping_requested`.
2. API flushes recorder and enqueues pending upload tasks.
3. API waits briefly for queueing acknowledgment (not completion).
4. API enqueues `audio_finalize_session`.
5. UI transitions to "Saving/Finalizing" with polling endpoint.

Important:

- API must not perform heavy merge inline.
- Finalize should only start after chunk upload states are consistent.

---

## 7) Long Meeting Strategy (15 min to 3 hr)

Recommended processing:

- Up to 20 min: direct finalize + postprocess.
- 20-60 min: postprocess in 10-minute windows with overlap.
- 60-180 min: chunked postprocess with bounded parallelism (2-4 workers max), checkpoint every chunk.

Expected user experience:

- Save operation returns quickly.
- Notes/transcript refinement continues asynchronously.
- UI indicates background progress.

---

## 8) Reliability Controls

1. Idempotency keys
- `upload:{session_id}:{chunk_index}`
- `finalize:{session_id}`

2. Retry policy
- Exponential backoff with jitter.
- Bounded retries per job type.

3. Dead-letter handling
- Persist failed job payload refs and errors.
- Provide requeue endpoint for admin/debug.

4. Diagnostics endpoints
- `/sessions/{session_id}/status`
- `/sessions/{session_id}/chunks`
- `/sessions/{session_id}/reconcile`
- `/sessions/{session_id}/retry-finalize`

---

## 9) Rollout Plan

### Phase A: Foundations

- Add DB schema for `recording_sessions` + `recording_chunks`.
- Add state machine guards in API.
- Add status endpoints.

### Phase B: Celery Integration

- Introduce Redis + Celery worker stack.
- Move chunk upload + finalize into Celery tasks.
- Keep feature flag fallback to current path.

### Phase C: Postprocess Hardening

- Move diarization/alignment fully async.
- Add long-audio chunked postprocess pipeline.

### Phase D: Enforcement

- Enable strict single active session lock.
- Remove legacy inline finalize path after burn-in.

---

## 10) Success Criteria

- Zero "whole meeting vanished" incidents in monitored window.
- Partial-save incidence reduced by >95%.
- Back-to-back recording reliability stable (no overlap corruption).
- Deterministic session audit trail available for every failed save.

---

## 11) Immediate Next Steps

1. Create DB migrations for `recording_sessions` and `recording_chunks`.
2. Add session state transition utility in backend service layer.
3. Introduce Celery app configuration (worker + queue names + retry defaults).
4. Implement `audio_upload_chunk` and `audio_finalize_session` tasks with idempotency.
5. Wire stop endpoint/flow to enqueue finalize instead of inline heavy finalize.

---

## 12) Security + Reliability Hardening (Current Pipeline Gaps)

This section addresses known issues found in the current implementation and how to fix them without adding per-chunk auth latency.

### 12.1 WebSocket Auth Model (No Per-Chunk Overhead)

Use authentication once per websocket connection, not on every audio frame.

Proposed flow:

1. Frontend obtains JWT via existing session flow.
2. Frontend opens `/ws/streaming-audio` with auth token at handshake (preferred: `Sec-WebSocket-Protocol`; fallback: query token over TLS only).
3. Backend verifies token once during WS connect.
4. Backend derives `user_email` from token claims and ignores client-provided `user_email`.
5. Backend binds connection context to authenticated user and authorized meeting/session.
6. Audio chunks are accepted without additional auth checks.

Result:

- Negligible latency impact (single verification per connection).
- Removes user impersonation risk on websocket stream ingress.

### 12.2 Authorization Guards for Meeting Binding

For `meeting_id` passed during WS connect:

- Validate format (UUID/safe slug).
- Verify the authenticated user has `edit` or `ai_interact` permission for that meeting.
- Reject unknown/unauthorized `meeting_id` with close code + structured error event.
- Do not allow raw folder/object-prefix control from untrusted identifiers.

### 12.3 Backpressure + Queue Bounds

Current risk: unbounded client/server queues can grow until OOM under network/API slowdown.

Fixes:

- Add max queue size in frontend (`maxBufferedChunks`) and backend (`asyncio.Queue(maxsize=...)`).
- Define policy when full:
  - short burst: block producer briefly,
  - sustained overload: drop oldest chunk with metric increment,
  - hard overload: fail session safely with actionable error.
- Emit queue depth metrics and warning logs for early detection.

### 12.4 Stop/Disconnect Reliability Contract

Current risk: non-explicit disconnect can skip full finalize path.

Fixes:

- Persist `stop_requested_at` and enqueue `audio_finalize_session` regardless of disconnect shape.
- Make finalize idempotent and lock-protected (`finalize:{session_id}`).
- Add watchdog task that reconciles sessions stuck in `stopping_requested|uploading_chunks|finalizing`.
- Client stop should send explicit stop and wait for server ack (or timeout fallback) before close.

### 12.5 Durability of Post-Recording Tasks

Current risk: in-process fire-and-forget tasks can be lost on restart.

Fixes:

- Move finalize/postprocess to Celery only.
- Store task IDs and state transitions in DB.
- On API boot, run reconciler for in-flight sessions without completed artifacts.

### 12.6 Upload Endpoint Guardrails

For `/upload-meeting-recording`:

- Enforce file size limit (configurable hard cap).
- Restrict accepted MIME/extensions.
- Reject unsupported or suspicious files early.
- Keep ffmpeg sandboxing/resource limits where possible.

### 12.7 Data Exposure Controls

- Remove transcript text snippets from info logs.
- Keep PII/audio metadata minimal in logs.
- Use redaction helper for provider errors and auth failures.

---

## 13) Issue-to-Fix Matrix

1. Unauthenticated WS + email spoofing
- Fix: one-time WS auth on connect, derive user from token, ignore query `user_email`.

2. Unbounded buffering (frontend/backend)
- Fix: bounded queues + backpressure policy + queue metrics.

3. Partial/vanished recordings on disconnect/rapid consecutive meetings
- Fix: idempotent finalize in Celery, acked stop flow, reconciliation watchdog.

4. Unsafe meeting/session identifiers controlling storage paths
- Fix: strict ID validation + RBAC check before recorder binding.

5. Lost finalize/postprocess on process restart
- Fix: durable Celery tasks with persisted task/session state.

6. Upload path DoS risk from large/invalid files
- Fix: size/type limits and early rejection.

7. Transcript leakage in logs
- Fix: remove content logs; log only IDs, durations, states, and error codes.

---

## 14) Implementation Order (Recommended)

1. WS auth-on-connect + meeting RBAC validation.
2. Bounded queues/backpressure on both client and server.
3. Stop ack + finalize idempotency lock + session reconciler.
4. Shift finalize/postprocess to Celery durable tasks.
5. Upload guardrails and logging redaction cleanup.

---

## 15) Phase 2 Foundation Status

Implemented in branch:

1. DB migration scaffold
- Added `009_audio_pipeline_sessions.sql` for:
  - `recording_sessions`
  - `recording_chunks`

2. State-management service
- Added centralized transition logic service for recording lifecycle states.

3. Celery foundation
- Added Celery app config and audio pipeline task module.
- Added durable finalize task scaffold (`audio.finalize_session`).

4. Startup reconciler
- Added background reconciler that scans stale transitional sessions and requeues finalize when Celery is enabled.

5. Container foundation
- Added Redis and Celery worker services in Docker Compose under `celery` profile.

### New/Relevant Env Vars

- `AUDIO_CELERY_ENABLED` (`false` by default)
- `CELERY_BROKER_URL` (default `redis://redis:6379/0`)
- `CELERY_RESULT_BACKEND` (default `redis://redis:6379/0`)

---

## 16) Phase 4 Progress (Chunk Orchestration)

Implemented now:

1. Chunk durability task
- Added `audio.upload_chunk` Celery task to validate persisted chunk availability and mark chunk status (`uploaded`/`failed`).

2. Chunk status tracking from streaming path
- On each saved chunk, backend records a `recording_chunks` row.
- If `AUDIO_CHUNK_UPLOAD_VIA_CELERY=true`, chunk upload task is enqueued and tracked in session metadata.

3. Final chunk tracking at stop
- Final flush chunk from recorder stop metadata is also written into `recording_chunks`, so finalize gating includes it.

4. Finalize gating on chunk readiness
- Finalize task checks chunk stats before merge:
  - waits/retries while pending
  - fails if chunk status indicates failure

5. Observability
- `GET /sessions/{session_id}/pipeline-status` now includes `chunk_stats`.

### New Env Var

- `AUDIO_CHUNK_UPLOAD_VIA_CELERY` (default `false`)
  - `false`: chunk path stays mostly inline behavior.
  - `true`: chunk durability status is orchestrated through Celery upload task before finalize.
