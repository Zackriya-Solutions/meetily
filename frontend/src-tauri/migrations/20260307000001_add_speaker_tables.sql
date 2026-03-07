-- Speaker profiles: named speakers that can be assigned to diarization IDs
CREATE TABLE IF NOT EXISTS speaker_profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#6366f1',
    created_at TEXT NOT NULL
);

-- Speaker mappings: links a diarization speaker_id (e.g. "speaker_0") to a named
-- profile for a specific meeting.
CREATE TABLE IF NOT EXISTS speaker_mappings (
    meeting_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL,    -- "speaker_0", "speaker_1" etc.
    profile_id TEXT NOT NULL,    -- references speaker_profiles.id
    PRIMARY KEY (meeting_id, speaker_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    FOREIGN KEY (profile_id) REFERENCES speaker_profiles(id) ON DELETE CASCADE
);

