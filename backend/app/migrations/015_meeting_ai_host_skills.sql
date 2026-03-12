-- Migration 015: Meeting-level AI Host Skills

CREATE TABLE IF NOT EXISTS meeting_ai_host_skills (
    meeting_id VARCHAR(255) PRIMARY KEY,
    skill_markdown TEXT NOT NULL DEFAULT '',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    updated_by VARCHAR(255),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_meeting_ai_host_skills_active
    ON meeting_ai_host_skills (is_active);
