-- Phase 9 foundation: Calendar integration + automation preferences

CREATE TABLE IF NOT EXISTS calendar_integrations (
    user_email TEXT NOT NULL,
    provider TEXT NOT NULL DEFAULT 'google',
    external_account_email TEXT,
    scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    access_token TEXT NOT NULL DEFAULT '',
    refresh_token TEXT NOT NULL DEFAULT '',
    token_expires_at TIMESTAMP NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    connected_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_email, provider)
);

CREATE TABLE IF NOT EXISTS calendar_oauth_states (
    state TEXT PRIMARY KEY,
    user_email TEXT NOT NULL,
    code_verifier TEXT NOT NULL,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_calendar_oauth_states_expires_at
    ON calendar_oauth_states (expires_at);

CREATE TABLE IF NOT EXISTS calendar_automation_settings (
    user_email TEXT PRIMARY KEY,
    reminders_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    attendee_reminders_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    reminder_offset_minutes INTEGER NOT NULL DEFAULT 2,
    recap_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    writeback_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    audio_summary_policy TEXT NOT NULL DEFAULT 'high_impact_only',
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

