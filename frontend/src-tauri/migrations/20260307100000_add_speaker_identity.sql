-- Add identity fields to speaker_profiles
ALTER TABLE speaker_profiles ADD COLUMN is_self INTEGER NOT NULL DEFAULT 0;
ALTER TABLE speaker_profiles ADD COLUMN global_auto_apply INTEGER NOT NULL DEFAULT 0;

-- Global mic profile: stores which profile maps to 'mic' across all meetings
CREATE TABLE IF NOT EXISTS global_speaker_defaults (
    key TEXT PRIMARY KEY,   -- e.g. 'mic_profile_id'
    value TEXT NOT NULL
);

