-- Migration: Add speaker field for speaker identification
-- This adds a speaker column to transcripts table to store which audio source the transcript came from
-- Values: 'mic' for microphone, 'system' for system audio

ALTER TABLE transcripts ADD COLUMN speaker TEXT;
