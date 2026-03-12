-- Migration 014: User AI Host Skills

CREATE TABLE IF NOT EXISTS user_ai_host_skills (
    user_email VARCHAR(255) PRIMARY KEY,
    skill_markdown TEXT NOT NULL DEFAULT '',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_user_ai_host_skills_active
    ON user_ai_host_skills (is_active);
