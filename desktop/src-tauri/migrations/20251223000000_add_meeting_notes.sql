-- Add meeting_notes table for storing user notes during meetings
CREATE TABLE IF NOT EXISTS meeting_notes (
    meeting_id TEXT PRIMARY KEY NOT NULL,
    notes_markdown TEXT,
    notes_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

-- Create index for faster lookups
CREATE INDEX IF NOT EXISTS idx_meeting_notes_meeting_id ON meeting_notes(meeting_id);
