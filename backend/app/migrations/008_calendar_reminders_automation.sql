-- Phase 9 automation: synced calendar events + reminder delivery dedup

CREATE TABLE IF NOT EXISTS calendar_events (
    user_email TEXT NOT NULL,
    provider TEXT NOT NULL DEFAULT 'google',
    event_id TEXT NOT NULL,
    meeting_title TEXT NOT NULL,
    meeting_link TEXT,
    organizer_email TEXT,
    attendee_emails JSONB NOT NULL DEFAULT '[]'::jsonb,
    start_time TIMESTAMP NOT NULL,
    end_time TIMESTAMP NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_email, provider, event_id, start_time)
);

CREATE INDEX IF NOT EXISTS idx_calendar_events_start_time
    ON calendar_events (start_time);

CREATE TABLE IF NOT EXISTS calendar_reminder_deliveries (
    user_email TEXT NOT NULL,
    provider TEXT NOT NULL DEFAULT 'google',
    event_id TEXT NOT NULL,
    event_start_time TIMESTAMP NOT NULL,
    sent_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    recipients JSONB NOT NULL DEFAULT '[]'::jsonb,
    PRIMARY KEY (user_email, provider, event_id, event_start_time)
);

