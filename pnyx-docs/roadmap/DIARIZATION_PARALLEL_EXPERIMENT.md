# Diarization Parallel Chunk Experiment (PoC)

**Status:** Experimental  
**Goal:** Validate whether chunked + overlap + parallel Deepgram calls can reduce wall time for long recordings before changing production diarization.

---

## 1) What This Is

A standalone benchmark script:

- `backend/examples/benchmark_chunked_diarization.py`

It does:

1. Splits a long audio file into chunk windows with overlap.
2. Sends chunk WAVs to Deepgram diarization in parallel.
3. Stitches chunk-level segments into a single timeline.
4. Prints wall-time and chunk-latency stats.

This script does **not** modify the current production diarization pipeline.

---

## 2) Prerequisites

1. `ffmpeg` + `ffprobe` available in runtime.
2. `DEEPGRAM_API_KEY` set.
3. Backend dependencies installed.

---

## 3) Run Command

From repo root:

```bash
cd backend
PYTHONPATH=. python examples/benchmark_chunked_diarization.py \
  --audio /absolute/path/to/meeting.wav \
  --chunk-minutes 10 \
  --overlap-seconds 30 \
  --concurrency 8
```

Recommended test matrix:

1. `chunk=8m overlap=20s concurrency=6`
2. `chunk=10m overlap=30s concurrency=8`
3. `chunk=12m overlap=40s concurrency=10`

---

## 4) Metrics To Compare

Capture for each run:

1. Total wall time.
2. Chunk API avg/max/min latency.
3. Number of stitched segments.
4. Error/retry behavior (if any).

Target guidance:

- Good first milestone for 3h audio: under 20 minutes.
- Strong milestone: 8-15 minutes.
- 3-5 minutes likely requires very high provider throughput/concurrency allowances.

---

## 5) Interpretation Notes

1. More concurrency reduces wall time until API/account/network limits are hit.
2. Too small chunk windows can hurt speaker continuity.
3. Too large overlap increases duplicate work and cost.
4. Stitching quality (speaker consistency across chunk boundaries) should be inspected before production rollout.

---

## 6) Phase 5 Implementation Status

Implemented in production path:

1. Chunked diarization Celery workflow (group fan-out by chunk).
2. Durable per-chunk job persistence in DB (`diarization_chunk_jobs`).
3. Global speaker reconciliation across chunk boundaries.
4. Progress endpoint for UI:
   - `GET /meetings/{meeting_id}/diarization-progress`

### New Migration

- `backend/app/migrations/010_diarization_chunk_jobs.sql`

### New/Relevant Env Vars

- `DIARIZATION_CELERY_CHUNK_WORKFLOW_ENABLED` (default: `true`)
- `AUDIO_CELERY_ENABLED` (must be `true` to use chunk Celery workflow)
- `DIARIZATION_CHUNK_GROUP_TIMEOUT_SECONDS` (default: `1800`)
- `DIARIZATION_CHUNK_WORK_DIR` (default: `./data/diarization_chunks`)
- `DIARIZATION_KEEP_CHUNK_FILES` (default: `false`)
