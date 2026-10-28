CREATE TABLE recording_sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT,
    ended_at TEXT,
    folder_path TEXT,
    meeting_id TEXT UNIQUE REFERENCES meetings(id) ON DELETE SET NULL,
    error TEXT
);
CREATE TABLE session_preferences (
    key TEXT PRIMARY KEY,
    value TEXT
);
