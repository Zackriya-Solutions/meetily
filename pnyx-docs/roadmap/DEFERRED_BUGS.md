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

- Status: Fixed
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

- Status: Fixed
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

- Status: Fixed
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

## BUG-CALENDAR-WEBHOOK-001: Calendar sync and reminder eligibility should be webhook-driven and attendee-aware

- Status: Deferred
- Priority: High
- Reported: 2026-02-24
- Area: Calendar Integration / Reminder Scheduling / Reliability

### Symptoms

- Reminder timing depends on polling loop (`CALENDAR_REMINDER_LOOP_SECONDS`) and can be late under load.
- Event updates close to meeting start (reschedule, attendee changes, cancellation) can be missed until next poll cycle.
- Reminders do not yet fully account for attendee response states (`declined`, `needsAction`, etc.).

### Impact

- Increased risk of wrong-time reminders.
- Possible reminder emails to declined or non-confirmed participants.
- More API cost from frequent polling than necessary.

### Current Understanding

- Current implementation is poll-based with periodic Google Calendar `events.list`.
- Calendar metadata already includes attendees/agenda and supports reminder dedupe.
- Missing pieces are push-triggered sync and stricter recipient eligibility logic.

### Deferred Fix Plan (Webhook + Better Scheduler)

1. Add Google Calendar push channels (`events.watch`) per active integration.
2. Store webhook channel metadata in DB and auto-renew before expiry.
3. On webhook notification, enqueue incremental sync using `syncToken` instead of full polling window.
4. Keep polling as fallback safety net (lower frequency, e.g. every 5-10 min).
5. Update reminder recipient filtering using attendee `responseStatus`.
6. Add observability for webhook health, sync lag, and reminder skip reasons.

### Proposed Data Model Changes

Add table `calendar_watch_channels`:

- `user_email` (TEXT, PK part)
- `provider` (TEXT, PK part, default `google`)
- `resource_id` (TEXT)
- `channel_id` (TEXT, unique)
- `channel_token` (TEXT, secret verifier)
- `expiration_at` (TIMESTAMP)
- `sync_token` (TEXT, nullable)
- `watch_status` (TEXT: `active|expired|stopped|error`)
- `last_notification_at` (TIMESTAMP, nullable)
- `last_sync_at` (TIMESTAMP, nullable)
- `last_error` (TEXT, nullable)

Add/extend event attendee payload fields in `calendar_events`:

- Keep `attendee_emails` for fast filters.
- Add `attendee_statuses` JSONB map: `{ "person@x.com": "accepted|tentative|declined|needsAction" }`.

### Proposed Backend Endpoints

1. `POST /api/calendar/google/webhook`
- Handles `X-Goog-*` notifications.
- Validate `channel_id`, `resource_id`, and shared `channel_token`.
- Do not trust request body (Google webhook has headers-only notification).
- Enqueue user/provider incremental sync job.

2. `POST /internal/calendar/watch/renew`
- Background task endpoint for renewing expiring channels.
- Recreates channel and stops old one when needed.

3. Optional admin diagnostics:
- `GET /api/calendar/debug/watch-status` for channel state/lag.

### Incremental Sync Strategy

1. Initial sync:
- Use `events.list` with time window and persist latest `nextSyncToken`.

2. Notification-driven sync:
- On webhook, call `events.list` with stored `syncToken`.
- Apply deltas (create/update/delete/cancelled).
- Persist new `nextSyncToken`.

3. Token invalidation (`410 Gone`):
- Clear `syncToken`.
- Run bounded full resync for near-future window.
- Store fresh `nextSyncToken`.

### Reminder Eligibility Rules (Decline-aware)

Host reminder:

- Always send to host if meeting is considered a real meeting.

Attendee reminders (`attendee_reminders_enabled=true`):

- Include only attendees where `responseStatus` in `accepted|tentative`.
- Exclude `declined`.
- Exclude `needsAction` by default (can be made configurable later).
- Exclude organizer/host email and duplicates.

Real meeting rule refinement:

- Current: attendee count > 1.
- Proposed: `eligible_participant_count >= 2` where eligible excludes declined.

### Scheduler Improvements

1. Convert scheduler to hybrid mode:
- Push-first (webhook-triggered sync jobs).
- Poll fallback every N minutes for missed notifications and channel expiry checks.

2. Add lease/lock for multi-instance safety:
- Prevent duplicate reminder sends across replicas.
- Keep DB dedupe row as final safeguard.

3. Add jitter and backoff:
- Avoid thundering herd on mass notifications.

### Security Requirements

1. Verify `X-Goog-Channel-ID`, `X-Goog-Resource-ID`, and a secret channel token.
2. Use HTTPS-only webhook endpoint.
3. Restrict webhook route from standard auth middleware and verify via channel secrets instead.
4. Audit-log every notification with channel/user mapping.

### Rollout Plan

1. Phase A (non-breaking):
- Add tables/fields and webhook endpoint.
- Keep existing poll flow as primary.

2. Phase B:
- Enable webhook-triggered incremental sync behind feature flag.
- Keep poll fallback active.

3. Phase C:
- Enable attendee-status based reminder filtering.
- Track skip metrics and compare delivery quality.

4. Phase D:
- Reduce polling frequency after webhook stability is proven.

### Testing Plan

1. Unit tests:
- Header validation, channel token verification, attendee eligibility filter.

2. Integration tests:
- Simulated webhook -> incremental sync -> due reminder selection.
- `410 Gone` sync-token reset path.

3. E2E test:
- Event create/update/decline in Google Calendar and verify reminder recipients/timing.

## BUG-ASK-AI-001: Ask AI context handling and prompting issues

- Status: Fixed
- Priority: High
- Reported: 2026-03-01
- Area: AI Features / Context Awareness / Prompt Engineering

### Symptoms

- "Ask AI" feature is not working as expected.
- Context from previous meetings is not being correctly added or utilized.
- Responses often lack the necessary depth or relevance due to insufficient context.

### Impact

- Users cannot effectively query their meeting history.
- Reduced value of the "Ask AI" feature for cross-meeting insights.

### Current Understanding

- The current system prompt does not effectively incorporate or leverage context from previous meetings.
- The retrieval mechanism for previous meeting context may be flawed or limited.

### Deferred Fix Plan (When Resumed)

1. Investigate the context retrieval logic to ensure relevant previous meetings are fetched.
2. Refine the system prompt to better instruct the AI on how to use the provided context.
3. Implement a more robust context window management strategy to handle larger amounts of historical data.
4. Add evaluation cases to verify "Ask AI" performance with multi-meeting context.

## TASK-SETTINGS-CLEANUP-001: Remove unnecessary tabs from Settings

- Status: Deferred
- Priority: Medium
- Reported: 2026-03-01
- Area: Frontend / UX / Settings

### Description

- The Settings page currently contains tabs that are not needed for the current version of the application.
- These unused tabs clutter the UI and may confuse users.

### Action Items

1. Identify the specific tabs in the Settings component that are obsolete or not functional.
2. Remove the corresponding UI elements and associated routing/logic.
3. Verify that the remaining settings tabs function correctly.
