-- Phase: Shared meeting notes tracking & collaboration

CREATE TABLE IF NOT EXISTS shared_meeting_notes (
    id SERIAL PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    owner_email TEXT NOT NULL,
    shared_with_email TEXT NOT NULL,
    share_token TEXT UNIQUE,
    shared_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_viewed_at TIMESTAMP,
    notes_updated_at TIMESTAMP,
    share_config JSONB NOT NULL DEFAULT '{"summary": true, "transcript": false}'::jsonb,
    UNIQUE (meeting_id, shared_with_email)
);

CREATE INDEX IF NOT EXISTS idx_shared_notes_shared_with
    ON shared_meeting_notes (shared_with_email);

CREATE INDEX IF NOT EXISTS idx_shared_notes_meeting
    ON shared_meeting_notes (meeting_id);

CREATE INDEX IF NOT EXISTS idx_shared_notes_token
    ON shared_meeting_notes (share_token);

-- Extend calendar_automation_settings with sharing toggles
ALTER TABLE calendar_automation_settings
    ADD COLUMN IF NOT EXISTS share_summary BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS share_transcript BOOLEAN NOT NULL DEFAULT FALSE;
