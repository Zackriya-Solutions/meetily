-- v0.2.0 core schema/search foundation
-- Adds meeting/source metadata, transcript raw fields, and FTS5 indexing.

-- Meetings metadata for source/export/search readiness.
ALTER TABLE meetings ADD COLUMN source_type TEXT NOT NULL DEFAULT 'recorded';
ALTER TABLE meetings ADD COLUMN language TEXT;
ALTER TABLE meetings ADD COLUMN duration_seconds REAL;
ALTER TABLE meetings ADD COLUMN recording_started_at TEXT;
ALTER TABLE meetings ADD COLUMN recording_ended_at TEXT;
ALTER TABLE meetings ADD COLUMN markdown_export_path TEXT;

-- Transcript persistence enhancements.
ALTER TABLE transcripts ADD COLUMN raw_transcript TEXT;
ALTER TABLE transcripts ADD COLUMN processing_version TEXT NOT NULL DEFAULT 'v0.2.0';

-- External-content FTS table over cleaned transcript text.
CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts
USING fts5(
  transcript,
  content='transcripts',
  content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS transcripts_ai
AFTER INSERT ON transcripts
BEGIN
  INSERT INTO transcripts_fts(rowid, transcript)
  VALUES (new.rowid, new.transcript);
END;

CREATE TRIGGER IF NOT EXISTS transcripts_ad
AFTER DELETE ON transcripts
BEGIN
  INSERT INTO transcripts_fts(transcripts_fts, rowid, transcript)
  VALUES('delete', old.rowid, old.transcript);
END;

CREATE TRIGGER IF NOT EXISTS transcripts_au
AFTER UPDATE ON transcripts
BEGIN
  INSERT INTO transcripts_fts(transcripts_fts, rowid, transcript)
  VALUES('delete', old.rowid, old.transcript);
  INSERT INTO transcripts_fts(rowid, transcript)
  VALUES (new.rowid, new.transcript);
END;

-- Backfill/rebuild index for existing rows.
INSERT INTO transcripts_fts(transcripts_fts) VALUES('rebuild');
