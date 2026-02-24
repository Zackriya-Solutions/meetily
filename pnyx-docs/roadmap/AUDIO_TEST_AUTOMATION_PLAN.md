# Audio + AI Test Automation Plan

**Status:** IN PROGRESS  
**Objective:** Remove manual speaking/testing for recording, transcription, diarization, Ask AI, and Catch-up flows.

## Current Implementation Progress

- Phase A started.
- Added backend pytest scaffold (`backend/tests`, `backend/pytest.ini`, `backend/requirements-dev.txt`).
- Added initial integration tests for:
  - health endpoint
  - chat streaming endpoint (`/chat-meeting`) with mocked chat service and RBAC
  - catch-up endpoint (`/catch-up`) with mocked Gemini stream
- Added additional integration tests for:
  - websocket streaming control flow (connect/ping/stop ack via router-level websocket simulation)
  - recording URL selection flow (`/meetings/{meeting_id}/recording-url`) with WAV preference assertion
  - notes generation kickoff (`/meetings/{meeting_id}/generate-notes`) with mocked background task
- Added reliability-focused integration tests for:
  - finalize idempotency (`_finalize_session` double-call safety)
  - backpressure path execution in websocket flow
  - reconciler endpoint access control (admin-only guard)
- Added live HTTP-level E2E tests (skipped by default unless enabled):
  - websocket connect/ping/stop against running backend
  - upload endpoint against running backend
- Added frontend Playwright smoke scaffold:
  - `frontend/playwright.config.ts`
  - `frontend/tests/e2e/smoke.spec.ts`
- Current backend local result: `10 passed, 2 skipped`.

## Run Commands

Backend local (fast mocked suite):

```bash
cd backend
pytest -q tests/integration
```

Backend live HTTP E2E (requires running backend + valid auth token):

```bash
cd backend
RUN_HTTP_E2E=true \
BACKEND_BASE_URL=http://localhost:5167 \
TEST_AUTH_BEARER=<google_jwt_token> \
pytest -q tests/integration/test_audio_http_e2e_live.py
```

Frontend Playwright smoke (requires frontend running on 3118):

```bash
cd frontend
npm install
npx playwright install
npm run test:e2e
```

Docker option for backend tests:

```bash
docker exec -it meeting-copilot-backend bash -lc "cd /app && pytest -q tests/integration"
```

---

## 1) Why This Is Needed

Current testing is manual and expensive:

- Requires live mic input and repeated speaking.
- Slow feedback loop for regressions.
- Hard to validate edge cases (disconnects, retries, long audio).
- Inconsistent results across runs.

Automation goals:

- Fast repeatable checks on every backend/frontend change.
- Deterministic pass/fail signal in CI.
- Separate mock-based tests (fast) and real-provider smoke tests (slow).

---

## 2) Scope of Automation

Primary flows to automate:

1. Streaming audio websocket flow
- Connect, send audio chunks, receive transcript events, stop, finalize.

2. Upload + offline processing flow
- Upload prerecorded file, process transcript, optional diarization.

3. AI response flows
- Ask AI (`/chat-meeting`)
- Catch-up (`/catch-up`)
- Notes generation (`/generate-notes`) including audio-context path.

4. Reliability controls
- Reconnect handling
- Backpressure behavior
- Stop ack and finalize idempotency
- Stale session reconciliation (Phase 2+)

---

## 3) Test Strategy

### 3.1 Pyramid

1. Unit tests (fastest)
- Pure functions and adapters (prompt builders, state transitions, parsers).

2. Integration tests (main coverage)
- API endpoints + DB + local storage + websocket chunk simulation.

3. E2E tests (critical paths only)
- Frontend user flows with mocked media/browser APIs.

4. Scheduled smoke tests (real providers)
- Nightly/weekly tests against Groq/Gemini/Deepgram keys.

### 3.2 Determinism

- Use fixed audio fixtures.
- Mock external providers in CI for stable outputs.
- Use snapshot/contract assertions on response structure (not brittle full-text equality).

---

## 4) Backend Automation Plan

## 4.1 Framework + Layout

- Use `pytest` + `pytest-asyncio`.
- Suggested structure:

`backend/tests/`
- `conftest.py`
- `fixtures/audio/`
- `unit/`
- `integration/`
- `contracts/`
- `smoke/`

### 4.2 Core Integration Tests

1. Websocket stream test
- Connect `/ws/streaming-audio` with auth token.
- Stream fixture PCM chunks with timestamps.
- Assert:
  - `connected` event arrives
  - at least one transcript event (`partial/final`)
  - `stop_ack` on stop
  - finalize state transition and output artifact created.

2. Upload endpoint test
- POST `/upload-meeting-recording` with fixture.
- Poll for processing completion.
- Assert transcript data exists and recording URL endpoint works.

3. Ask AI test
- Seed meeting transcript context.
- Call `/chat-meeting` stream.
- Assert non-empty streamed response and expected JSON/text contract.

4. Catch-up test
- POST `/catch-up` with transcript payload.
- Assert concise streamed bullets/text emitted.

5. Notes generation test
- POST `/generate-notes` with transcript-only mode.
- Assert required fields in output and markdown present.

### 4.3 Reliability Tests

1. Disconnect-resume scenario
- Drop websocket mid-stream, reconnect with same session id.
- Assert session can continue and finalize successfully.

2. Backpressure scenario
- Push chunks faster than processing rate.
- Assert no crash, bounded queue behavior, and expected warning/error handling.

3. Idempotent finalize
- Trigger finalize path twice.
- Assert no duplicate artifacts and stable completed status.

4. Reconciler scenario (Phase 2+)
- Insert stale transitional session.
- Run reconciler once.
- Assert task requeue or expected transition.

---

## 5) Provider Mocking Plan

Use mocks/stubs for CI:

1. Groq transcription mocks
- Return deterministic text/segments for known fixture inputs.

2. Gemini generation mocks
- Return deterministic summary/query-classifier outputs.

3. Deepgram diarization mocks
- Return fixed speaker segments.

4. HTTP-layer mocking
- Use `respx` or monkeypatch wrappers to avoid real network calls in default CI.

Policy:

- PR/CI default: mocked providers only.
- Nightly job: optional real-provider smoke suite (guarded by secrets).

---

## 6) Frontend Automation Plan

Use Playwright for key flows:

1. Recording controls smoke flow
- Start recording, verify UI state transitions, stop recording.
- Use mocked websocket and mocked `getUserMedia`.

2. Ask AI and Catch-up rendering
- Seed test data and verify response stream renders in UI.

3. Error-state UX
- Simulate auth expiry, websocket disconnect, backpressure errors.
- Assert user-facing error/notification behavior.

Note:

- Browser-level real mic testing is optional and should not block CI.

---

## 7) Audio Fixture Plan

Create reusable fixtures:

1. `short_clean_speech.wav` (30-60s)
2. `multi_speaker_short.wav` (2-5 min)
3. `noisy_background.wav` (1-2 min)
4. `long_meeting_sample.wav` (15+ min)
5. `edge_silence_heavy.wav` (speech gaps)

Also store pre-converted chunk fixtures:

- PCM frames matching websocket chunk format (timestamp + PCM payload).

---

## 8) CI/CD Plan

1. On every PR:
- Run unit + integration tests with mocked providers.
- Run lint + type checks.

2. On merge to main:
- Run extended integration suite.

3. Nightly:
- Optional real-provider smoke tests (if keys configured).
- Alert only on sustained failures.

---

## 9) Acceptance Criteria

Automation considered successful when:

1. No manual mic/speaking needed for standard regression checks.
2. Websocket stream + stop/finalize covered in CI.
3. Ask AI + Catch-up + Notes generation covered by integration tests.
4. Mocked suite runtime stays practical (target under 10-15 min total).
5. Real-provider smoke suite runs separately and is non-blocking for PRs.

---

## 10) Implementation Phases

### Phase A (Quick Win)

1. Add backend integration harness + fixtures.
2. Add websocket simulation test.
3. Add ask-ai and catch-up integration tests.

### Phase B

1. Add provider mocks and contract snapshots.
2. Add reliability tests (disconnect/backpressure/finalize idempotency).

### Phase C

1. Add Playwright smoke tests for frontend critical flows.
2. Add nightly real-provider smoke pipeline.

---

## 11) Immediate Next Steps

1. Create `backend/tests` scaffold and base fixtures.
2. Implement websocket streaming integration test with prerecorded PCM chunks.
3. Implement `/chat-meeting` and `/catch-up` API tests with seeded transcript context.
4. Add CI job for mocked integration suite.
