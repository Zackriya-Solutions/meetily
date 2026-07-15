-- Add expected_speakers column to diarization_settings.
-- Stores user-configured maximum number of speakers per recording.
ALTER TABLE diarization_settings ADD COLUMN expected_speakers INTEGER NOT NULL DEFAULT 2;
