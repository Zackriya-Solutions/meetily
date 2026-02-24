# Deferred Bugs

This file tracks high-impact bugs that are confirmed but intentionally deferred.

## BUG-RESUME-001: Resume/Recovery may save only second-half audio

- Status: Deferred
- Priority: High
- Reported: 2026-02-20
- Area: Recording / Recovery / Save pipeline

### Symptoms

- After reload + resume, final meeting audio sometimes contains only post-resume audio.
- In some runs, duplicate meeting entries appear temporarily in sidebar/meeting list.
- In some runs, resumed transcript chunks appear out of order before stabilizing.

### Repro (Observed)

1. Start recording and speak for ~1+ minute.
2. Reload page.
3. Recover/resume meeting.
4. Speak again.
5. Stop and save.
6. Open recording: first segment may be missing; only second segment present.

### Impact

- Data integrity risk for recorded meetings.
- User trust impact due to missing first-half audio.
- Potential duplicate UI entries and confusing timeline ordering.

### Current Understanding

- This is a resume/finalization/chunk continuity edge case across frontend state + backend recorder/finalize lifecycle.
- Multiple mitigations are already in place, but issue is still intermittently reproducible.

### Temporary Workaround

- Avoid reload during active meeting for critical recordings.
- If reload is required, validate full audio duration immediately after save.

### Deferred Fix Plan (When Resumed)

1. Add deterministic end-to-end test for record -> reload -> resume -> save.
2. Add per-session chunk manifest verification before finalize.
3. Add strict single-meeting session lock and finalize idempotency guard.
4. Add backend diagnostics endpoint to inspect chunk inventory vs merged output.

## BUG-NOTES-CTX-001: Notes generation not using calendar context deeply enough

- Status: Deferred
- Priority: High
- Reported: 2026-02-20
- Area: Notes Generation / Calendar Integration / Prompt Quality

### Symptoms

- Generated notes often ignore useful calendar context (agenda, attendee roles, meeting purpose, recurrence history).
- Notes quality is weaker for planning/decision meetings where pre-meeting context exists in calendar metadata.
- Action items/decisions can be less grounded because expected outcomes are not inferred from event context.

### Impact

- Lower notes quality and weaker structure for context-heavy meetings.
- Missed opportunity to improve summary relevance despite available calendar integration.
- Reduced confidence in AI-generated notes for stakeholders.

### Current Understanding

- Calendar sync/reminder pipeline is active, but notes generation prompt/context assembly does not yet consistently include enriched event metadata.
- No dedicated ranking/selection logic exists to decide which calendar fields should influence note generation most.

### Temporary Workaround

- Add manual context in meeting title/custom prompt before generating notes.
- Use stronger templates for meetings with known agenda/decision focus.

### Deferred Fix Plan (When Resumed)

1. Build a calendar context pack for notes generation (title, agenda/description, organizer, attendees, meeting link, recurrence info, prior meeting references).
2. Add context-aware prompt builder that injects meeting-type expectations (planning, interview, status, decision review).
3. Add role-aware extraction for actions/decisions (map likely owners from organizer/attendees).
4. Add evaluation set comparing notes quality with/without calendar context.
5. Add feature flags and fallback behavior when calendar data is missing/low quality.

## BUG-TEST-AUTO-001: Audio + AI regression coverage is too manual

- Status: Deferred
- Priority: High
- Reported: 2026-02-22
- Area: QA / Audio Pipeline / AI Endpoints / Reliability

### Symptoms

- End-to-end validation depends on manual speaking and repetitive UI testing.
- Critical flows (recording, websocket streaming, stop/finalize, ask-ai, catch-up, notes) are not covered by deterministic automated suites.
- Regressions are discovered late and inconsistently.

### Impact

- Slow release velocity and high QA effort for each audio/AI change.
- Increased chance of shipping regressions in recording durability/finalization.
- Poor confidence in pipeline changes across phases.

### Current Understanding

- Architecture now supports better automation (session state machine, celery tasks, pipeline status endpoints), but a full automated harness is still not implemented.
- A plan exists in `pnyx-docs/roadmap/AUDIO_TEST_AUTOMATION_PLAN.md`.

### Temporary Workaround

- Use one short smoke scenario per branch (record -> stop -> finalize -> ask-ai -> catch-up).
- Inspect worker/backend logs and `/sessions/{session_id}/pipeline-status` for each test run.

### Deferred Fix Plan (When Resumed)

1. Build backend integration harness using prerecorded audio fixtures.
2. Add websocket chunk simulation tests for stop/finalize/reconnect paths.
3. Add mocked-provider tests for Groq/Gemini/Deepgram behavior in CI.
4. Add Playwright smoke tests for start/stop + ask-ai + catch-up UI flows.
5. Add nightly real-provider smoke suite and regression snapshots.

## BUG-DIARIZATION-COST-001: Groq parallel baseline can exhaust free-tier ASPH limits

- Status: Deferred
- Priority: High
- Reported: 2026-02-23
- Area: Diarization / Transcription / Cost Control

### Symptoms

- Diarization jobs fail with Groq `429 rate_limit_exceeded` even when Deepgram succeeds.
- Failures increase for long meetings when Groq baseline and diarization are run concurrently.

### Impact

- User-facing diarization failures on free/on-demand Groq tier.
- Unpredictable completion time due to retry windows.

### Current Understanding

- Groq free-tier ASPH quota is the bottleneck.
- Parallel orchestration consumes hourly audio budget faster and triggers 429s.
- Sequential mode is now the default to reduce burst quota usage.

### Current Behavior

- `GROQ_PARALLEL_WITH_DIARIZATION_ENABLED=false` by default (sequential).
- Parallel mode remains available behind this flag for paid/higher-limit tiers.

### Deferred Fix Plan (When Resumed)

1. Add dynamic concurrency/rate-limit controller based on recent 429s.
2. Auto-fallback to sequential mode after quota warnings.
3. Add provider-budget aware queueing and retry scheduling.
4. Re-enable parallel mode as default only for paid quota profiles.
