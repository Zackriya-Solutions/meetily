-- General-purpose application settings key/value store.
-- Unrelated feature flags and preferences should live here,
-- not in speaker-specific tables such as global_speaker_defaults.
CREATE TABLE IF NOT EXISTS app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

