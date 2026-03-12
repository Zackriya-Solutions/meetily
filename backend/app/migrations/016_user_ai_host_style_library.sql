-- Migration 016: User AI Host Style Library + default selector

CREATE TABLE IF NOT EXISTS user_ai_host_styles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_email VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    skill_markdown TEXT NOT NULL DEFAULT '',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_user_ai_host_styles_user
    ON user_ai_host_styles (user_email);

CREATE TABLE IF NOT EXISTS user_ai_host_style_defaults (
    user_email VARCHAR(255) PRIMARY KEY,
    default_style_id VARCHAR(255) NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);
