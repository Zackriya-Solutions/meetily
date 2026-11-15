-- Migration: speaker identification
-- transcripts.speaker (added in 20251110000001) now stores a stable speaker id:
--   'me'        - the local user's microphone voice
--   'others'    - system audio when no voiceprint model is installed
--   'speaker_N' - a voiceprint cluster
-- meeting_speakers holds user-chosen display names so renames never rewrite transcripts.

CREATE TABLE IF NOT EXISTS meeting_speakers (
    meeting_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (meeting_id, speaker_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);
