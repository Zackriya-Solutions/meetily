# Implementation Plan: Audio + Transcript + Template Notes with Gemini 2.5

**Date:** Feb 13, 2026  
**Status:** Proposed  
**Owner:** Backend + AI Pipeline

## 1. Objective
Improve post-meeting notes quality by generating notes from:
- Meeting audio file (primary signal for meaning and intent)
- Full meeting transcript (secondary structured aid, especially for speaker mapping)
- Selected meeting template prompt (structure + output format)

Target outcome: higher factual accuracy and better action-item extraction without large latency or cost regressions.

## 2. Current Architecture Review (What Exists Today)

### Current generation path
- Notes endpoint: `POST /meetings/{meeting_id}/generate-notes`
- Current backend implementation: `backend/app/api/routers/transcripts.py`
- Current behavior: transcript-only summarization using Gemini (`gemini-2.5-flash`) with template prompt.

### Current audio path
- Recording stored as `recording.wav` at path convention: `{meeting_id}/recording.wav`
- Signed URL endpoint exists: `GET /meetings/{meeting_id}/recording-url`
- Storage abstraction exists for local/GCP: `backend/app/services/storage.py`

### Gap
No pipeline currently passes audio into notes generation. Audio exists but is not used in final notes synthesis.

## 3. Proposed Target Design

### 3.1 Generation strategy
Use a two-source synthesis strategy in Gemini:
1. Audio as primary factual/semantic backbone
2. Transcript as secondary support for:
- speaker identification and segmentation support
- quick lookup of entities (names, dates, terms)
- recovery when audio segments are unclear

Template prompt remains mandatory and controls output schema.

### 3.2 API contract changes
Extend `GenerateNotesRequest` in `backend/app/schemas/meeting.py` with:
- `use_audio_context: bool = true`
- `audio_mode: str = "auto"` (`auto | compressed | wav | transcript_only`)
- `audio_url: str = ""` (optional override)
- `max_audio_minutes: int = 120` (safety cap)

Keep backward compatibility: existing clients should continue working with transcript-only if new fields are absent.

### 3.3 Backend flow changes
Update `generate_notes_with_gemini_background(...)` in `backend/app/api/routers/transcripts.py`:
1. Resolve transcript text (existing behavior)
2. Resolve audio source:
- If `audio_url` provided, use it
- Else fetch from storage path `{meeting_id}/recording.wav`
3. Build multimodal Gemini request with:
- system instruction + template prompt
- transcript text
- audio file reference
4. Parse strict JSON response into existing template structure
5. Preserve current markdown conversion and DB update behavior

### 3.4 Prompting strategy (important)
Prompt should explicitly rank source reliability:
- audio is primary for decisions, commitments, and action-item extraction
- transcript is secondary and mainly assists with speaker mapping / entities
- never invent details missing in both

Add mandatory output constraints:
- valid JSON only
- no markdown in model raw response
- keep existing section keys to avoid frontend regressions

## 4. Audio Format Recommendation (Compressed vs WAV)

### Decision
Use **compressed derivative for model ingestion by default**, keep **full WAV as canonical archive**.

### Rationale
- WAV is best for archival + reprocessing, but expensive for network transfer and model upload time.
- For notes generation, intelligibility matters more than lossless fidelity.
- A well-encoded mono speech file is usually enough for summarization quality.

### Recommended standard
- Canonical storage: `recording.wav` (existing)
- Derivative for Gemini: `recording.notes.opus` (or AAC/MP3 if Opus not available)
- Speech-focused encode settings:
- mono
- 16 kHz sample rate
- 24-32 kbps bitrate

### Fallback rules
- If compressed file missing or rejected by provider, fallback to WAV.
- If both unavailable, run transcript-only and mark `audio_used=false` in process metadata.

## 5. Implementation Steps

### Phase A: Contracts + Feature Flag
1. Add request fields in `backend/app/schemas/meeting.py`.
2. Add env flags:
- `NOTES_AUDIO_ENABLED=true`
- `NOTES_AUDIO_DEFAULT_MODE=auto`
3. Add response/process metadata fields (`audio_used`, `audio_mode`, `audio_duration_sec`, `audio_source`).

### Phase B: Audio Asset Resolution
1. Add helper in `backend/app/services/storage.py` (or new helper module) to resolve preferred audio asset by mode.
2. Add optional creation path for compressed derivative if not present.
3. Add signed URL generation for derivative file.

### Phase C: Gemini Multimodal Notes Call
1. Add multimodal builder in `backend/app/api/routers/transcripts.py`.
2. Keep existing template prompt function (`get_template_prompt`) and structure merge logic.
3. Add robust JSON parse + retry (one retry with stricter parser instruction).

### Phase D: Reliability + Observability
1. Persist processing metadata via `db.update_process(..., metadata=...)`.
2. Log structured metrics:
- notes generation latency
- model token usage (if exposed)
- audio mode selected
- fallback reason
- parse failures

### Phase E: Frontend Controls (Optional, minimal)
1. Add toggle in notes generation UI:
- "Use recording audio to improve notes"
2. Advanced selector (hidden behind dev flag): `Auto / Compressed / WAV / Transcript only`.

## 6. Best Practices for This Feature
- Keep audio-first for core meaning extraction; use transcript as secondary guardrail.
- Use one canonical template schema across all modes.
- Do not block note generation if audio retrieval fails.
- Set strict timeouts and file size caps before model invocation.
- Store deterministic run metadata for QA and cost audits.
- Add explicit privacy notice when audio is sent to model provider.
- Redact/trim transcript noise before sending to model.

## 7. QA and Evaluation Plan

### Functional tests
- Transcript-only still works unchanged.
- Audio+transcript path returns valid schema for all templates.
- Missing audio gracefully falls back.
- Template switch still regenerates correctly.

### Quality evaluation
Create a 30-meeting benchmark:
- 10 standups
- 10 planning/review
- 10 high-stakes decisions

Scorecards:
- action-item precision/recall
- decision extraction correctness
- owner/date correctness
- human readability score

Ship default audio mode only if quality gain is clear and latency/cost are acceptable.

## 8. Rollout Plan
1. Hour 1-2 (Feb 13, 2026): backend contract and feature-flag wiring.
2. Hour 2-4 (Feb 13, 2026): multimodal notes path + compressed audio fallback chain.
3. Hour 4-6 (Feb 13, 2026): QA on template outputs + strict JSON parsing regressions.
4. Hour 6-8 (Feb 13, 2026): enable `auto` mode for internal traffic and monitor metadata.

## 9. Risks and Mitigations
- Large audio causes latency spikes: use compressed derivative + duration cap.
- Provider MIME incompatibilities: fallback chain (`compressed -> wav -> transcript_only`).
- JSON drift from multimodal prompts: enforce strict schema prompt + parse retry.
- Cost creep: log per-run metadata and review weekly.

## 10. Definition of Done
- New request fields are live and backward compatible.
- Notes pipeline can consume transcript + audio + template prompt.
- Default `auto` mode chooses compressed audio when available.
- End-to-end fallback works with no user-visible failures.
- Benchmark shows measurable quality improvement vs transcript-only baseline.
