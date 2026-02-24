-- Migration: Durable per-chunk diarization job tracking
-- Date: 2026-02-23

CREATE TABLE IF NOT EXISTS diarization_chunk_jobs (
  id SERIAL PRIMARY KEY,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  chunk_index INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'processing', 'completed', 'failed')),
  task_id TEXT,
  start_sec REAL NOT NULL DEFAULT 0,
  end_sec REAL NOT NULL DEFAULT 0,
  duration_sec REAL NOT NULL DEFAULT 0,
  segment_count INTEGER NOT NULL DEFAULT 0,
  error_message TEXT,
  result_json JSONB,
  started_at TIMESTAMP,
  completed_at TIMESTAMP,
  created_at TIMESTAMP NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
  UNIQUE(meeting_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_diarization_chunk_jobs_meeting
  ON diarization_chunk_jobs(meeting_id);

CREATE INDEX IF NOT EXISTS idx_diarization_chunk_jobs_status
  ON diarization_chunk_jobs(meeting_id, status);
