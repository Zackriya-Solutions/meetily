-- Phase 2 foundation for durable audio pipeline state tracking

CREATE TABLE IF NOT EXISTS recording_sessions (
    session_id VARCHAR(128) PRIMARY KEY,
    user_email VARCHAR(255) NOT NULL,
    meeting_id VARCHAR(128) NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'recording',
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    stop_requested_at TIMESTAMPTZ,
    stopped_at TIMESTAMPTZ,
    finalized_at TIMESTAMPTZ,
    expected_chunk_count INTEGER NOT NULL DEFAULT 0,
    finalized_chunk_count INTEGER NOT NULL DEFAULT 0,
    dropped_chunk_count INTEGER NOT NULL DEFAULT 0,
    idempotency_finalize_key VARCHAR(255),
    last_heartbeat_at TIMESTAMPTZ,
    error_code VARCHAR(128),
    error_message TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_recording_sessions_user ON recording_sessions(user_email);
CREATE INDEX IF NOT EXISTS idx_recording_sessions_meeting ON recording_sessions(meeting_id);
CREATE INDEX IF NOT EXISTS idx_recording_sessions_status ON recording_sessions(status);
CREATE INDEX IF NOT EXISTS idx_recording_sessions_updated ON recording_sessions(updated_at DESC);

CREATE TABLE IF NOT EXISTS recording_chunks (
    session_id VARCHAR(128) NOT NULL,
    chunk_index INTEGER NOT NULL,
    byte_size INTEGER NOT NULL DEFAULT 0,
    checksum VARCHAR(128),
    storage_path TEXT,
    upload_status VARCHAR(16) NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    uploaded_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (session_id, chunk_index),
    CONSTRAINT fk_recording_chunks_session
        FOREIGN KEY (session_id) REFERENCES recording_sessions(session_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_recording_chunks_session_status
    ON recording_chunks(session_id, upload_status);

