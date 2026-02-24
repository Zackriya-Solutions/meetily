-- Phase 9 enhancements: agenda context + stricter "real meeting" reminder behavior

ALTER TABLE calendar_events
    ADD COLUMN IF NOT EXISTS agenda_description TEXT;

CREATE INDEX IF NOT EXISTS idx_calendar_events_user_start_time
    ON calendar_events (user_email, start_time);
