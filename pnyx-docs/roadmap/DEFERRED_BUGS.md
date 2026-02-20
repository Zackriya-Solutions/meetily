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

