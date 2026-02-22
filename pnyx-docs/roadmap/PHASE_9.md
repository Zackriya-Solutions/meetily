# Phase 9: Calendar Integration & Workflow Automation

**Status:** **PLANNED**
**Focus:** Calendar-driven adoption, pre-meeting context, and post-meeting distribution
**Prerequisite:** Phase 8 (Polish & Production) must be complete.

---

## 1. Goal
Make Pnyx a default part of meetings by integrating with calendars. Automatically deliver the right context before a meeting starts and ensure consistent distribution of notes after it ends.

## 2. Problems Solved
*   **Adoption friction:** Users forget to start Pnyx or add it to the meeting.
*   **Missing context:** Agenda/description/attendees are scattered and not captured in Pnyx.
*   **Inconsistent sharing:** Summaries are posted late or not at all.
*   **Weak continuity:** Recurring meetings lose history and open actions.

## 3. Scope

### A. Provider Support
*   **target:** Google Calendar (G Suite) with OAuth.

### B. Audio-Enhanced Notes (Post-Meeting)
*   **Goal:** improve note quality by summarizing from audio + meeting context.
*   **Inputs:** audio recording, agenda/description, participants + roles, meeting type.
*   **Outputs:** structured summary, decisions, action items with owners.
*   **Policy:** use audio + transcript for all meetings.

### C. Pre-Meeting Automation
*   **T-2 minute reminder email** to host (optional to attendees) with:
    *   "Start Pnyx" call-to-action
    *   Setup checklist (mic, room, permissions)
*   **Start Meeting CTA behavior:** reminder email includes a Start Meeting button that opens Pnyx in a new tab and triggers recording startup flow.
*   **Event metadata ingestion:** title, agenda/description, attendees, location, meeting link, recurring series ID.
*   **Pre-meeting brief:** generate a structured brief from agenda/description.
*   **Skeleton notes template:** headings and expected decision points shown in Pnyx before start.

### D. Post-Meeting Distribution
*   **Recap email to all attendees** with Pnyx notes link.
*   Optional **calendar writeback** (summary/decisions/actions appended to event description) when enabled.

## 4. Execution Plan

### Workstream 1: OAuth + Permissions
*   Least-privilege scopes for calendar read and optional writeback.
*   Org-level and user-level toggles for:
    *   Reminders
    *   Attendee email policy
    *   Calendar writeback

### Workstream 2: Event Sync
*   Sync upcoming events for connected users.
*   Identify meetings with conferencing links (Meet/Zoom) and recurring series.

### Workstream 3: Reminder + Brief Generation
*   Scheduler to send T-2 minute reminder emails.
*   Convert agenda/description into structured note templates.

### Workstream 4: Post-Meeting Distribution
*   After meeting end, send recap email with notes link.
*   Optional writeback to event description.

### Workstream 5: Audio-Enhanced Notes
*   Add audio summarization job using Gemini audio input.
*   Merge audio summary with agenda/participant context.
*   Keep transcript-only fallback only for hard failure cases.

## 5. Success Metrics
*   **Adoption:** % of meetings started via calendar reminder.
*   **Coverage:** % of meetings with pre-meeting brief generated.
*   **Sharing:** % of meetings where all attendees receive recap email.
*   **Engagement:** Open rate for reminder and recap emails.
*   **Notes Quality:** internal QA score uplift for audio-based summaries vs transcript-only.

## 6. Risks & Mitigations
*   **Privacy concerns:** Provide opt-out and minimal writeback by default.
*   **Calendar permissions friction:** Clear scope explanation and admin controls.
*   **Email fatigue:** Default to host-only reminders, optional attendee notifications.
*   **Audio cost/latency:** Queue-based summarization, retry policy, and observability around model time/cost.

## 7. Detailed Implementation Plan (Email Automation)

### Phase 9.1: Event Sync Foundation
*   Build `calendar_events` table for upcoming events and sync metadata (title, start/end, attendees, organizer, meeting link, recurrence ID, calendar event ID).
*   Add periodic sync worker (every 2-5 minutes) for connected users.
*   Upsert by `(user_email, provider, event_id, start_time)` to avoid duplicate rows.
*   Persist `last_synced_at` per user integration for incremental sync.
*   Skip cancelled events and events without organizer email.

### Phase 9.2: Reminder Email Pipeline (T-2)
*   Add `calendar_email_jobs` table with job types: `pre_meeting_reminder`, `post_meeting_recap`.
*   Scheduler scans events and enqueues reminder jobs for `event_start - reminder_offset_minutes`.
*   Enforce idempotency key: `reminder:{event_id}:{start_time}`.
*   Recipients:
    *   Host always when reminders enabled.
    *   Attendees only if `attendee_reminders_enabled = true`.
*   Email content:
    *   Meeting title/time
    *   "Start Pnyx" CTA link
    *   Basic setup checklist
*   Mark sent/failed state with retry count and last error.
*   Current implementation slice:
    *   API endpoint: `POST /api/calendar/reminders/send` sends a reminder immediately.
    *   Email template includes `Start Meeting in Pnyx` button.
    *   Calendar settings page includes `Send Test Reminder Email` action.
    *   Background automation worker syncs upcoming Google events and sends due reminders automatically.

### SMTP + Launch URL Config
*   `SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`, `SMTP_FROM_EMAIL`
*   Optional: `SMTP_USE_TLS=true|false`
*   Optional: `CALENDAR_EMAIL_START_MEETING_URL`
    *   Default: `http://localhost:3118/?autoStart=true&source=calendar_email`
*   Optional automation toggles:
    *   `CALENDAR_REMINDER_AUTOMATION_ENABLED=true|false` (default `true`)
    *   `CALENDAR_REMINDER_LOOP_SECONDS=60` (default `60`)

### Phase 9.3: Post-Meeting Recap Pipeline
*   On meeting completion (or finalized notes ready), enqueue recap job.
*   Recipients default: all attendees + organizer (respect org/user toggles).
*   Include notes URL, summary snippet, decisions, and action items.
*   Idempotency key: `recap:{event_id}:{meeting_id}`.
*   Optional calendar writeback after recap send when enabled and write scope granted.

### Phase 9.4: Email Delivery Service
*   Introduce `EmailService` abstraction with provider-backed implementation (SES/SendGrid/Postmark).
*   Add HTML + plain text templates in backend for reminder and recap.
*   Validate recipient domains and de-duplicate addresses before send.
*   Add exponential backoff retries (e.g., 3 attempts) for transient failures.
*   Add dead-letter state for manual inspection of permanent failures.

### Phase 9.5: Observability + Controls
*   Metrics:
    *   jobs_enqueued, jobs_sent, jobs_failed, retry_count
    *   reminder send latency vs scheduled time
    *   recap delivery coverage by attendee count
*   Structured logs include `user_email`, `event_id`, `job_type`, `provider_message_id`.
*   Add admin/debug endpoint for inspecting job status by event/meeting.
*   Add per-user pause switch for all calendar emails.

### Phase 9.6: Rollout Strategy
*   Stage 1: internal users only, host reminders only, no attendee mail.
*   Stage 2: enable attendee reminders for pilot group.
*   Stage 3: enable recap-by-default for all connected users.
*   Stage 4: enable optional writeback for users with write scope.

### Phase 9.7: Meeting Launch UX (Email CTA)
*   Add `Start Meeting` button in reminder email template that opens a new tab with launch params.
*   New tab auto-triggers recording start flow and clears launch params after activation.
*   Do not add extra start button to main UI; keep launch via email CTA.
*   If microphone permission is blocked, show permission error and keep user in launch tab for retry.
*   Track analytics for:
    *   `start_meeting_email_click`
    *   `start_meeting_email_autostart_success`
    *   `start_meeting_email_autostart_error`

### Engineering Checklist
*   [ ] DB migrations: `calendar_events`, `calendar_email_jobs`, sync checkpoints.
*   [ ] Sync worker + incremental Google Calendar fetch.
*   [ ] Reminder scheduler + idempotent enqueue.
*   [ ] Recap trigger path from meeting completion.
*   [ ] Email templates + provider integration.
*   [ ] Retry/dead-letter handling.
*   [ ] Metrics + logging + debug endpoints.
*   [ ] End-to-end tests: connect -> sync -> reminder -> meeting end -> recap.
*   [x] Reminder email supports `Start Meeting` CTA that opens new tab and triggers recording startup.
