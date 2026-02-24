# Calendar Integration Notes

## Current behavior

- OAuth connect/disconnect for Google Calendar is enabled.
- Calendar sync worker pulls upcoming primary calendar events and stores:
  - title
  - link
  - attendees
  - agenda/description
  - start/end time
- Pre-meeting reminder automation is enabled when `CALENDAR_REMINDER_AUTOMATION_ENABLED=true`.

## Mail sender identity

- Outgoing email sender is controlled by SMTP env configuration, especially:
  - `SMTP_FROM_EMAIL`
  - `SMTP_USERNAME`
- Reminder/recap mail does **not** send as the meeting host automatically.
- If `SMTP_FROM_EMAIL` is set to `gagan@appointy.com`, recipients will see mail from that address.

## "Real meeting" rule

- Automated reminder and recap sending only applies to events with more than 1 attendee in calendar attendee list.
- Technical rule in backend: `jsonb_array_length(attendee_emails) > 1`.

## Notes quality improvements

- During notes generation, calendar agenda/description is injected into prompt context when a matching event is found.
- Context includes:
  - calendar title
  - scheduled start
  - meeting link
  - attendee count
  - agenda/description text

## Post-notes automation

- If `recap_enabled=true` and meeting passes real-meeting rule, recap email can be sent after notes generation.
- If `writeback_enabled=true` and Google write scope (`calendar.events`) is granted, notes are written back to event description with a stable Pnyx marker block.

## Deferred improvement plan

- Webhook-based calendar sync (`events.watch`) and decline-aware reminder scheduling plan is documented in:
  - `pnyx-docs/roadmap/DEFERRED_BUGS.md` under `BUG-CALENDAR-WEBHOOK-001`
