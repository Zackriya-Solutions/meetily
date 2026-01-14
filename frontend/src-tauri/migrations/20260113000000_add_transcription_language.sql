-- Add transcription_language column to meetings table
-- This stores the language used for transcription (null = auto-detected)
ALTER TABLE meetings ADD COLUMN transcription_language TEXT;
