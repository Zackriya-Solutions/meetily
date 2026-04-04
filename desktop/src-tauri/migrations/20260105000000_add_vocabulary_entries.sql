-- v0.2.0 deep M6 vocabulary foundation
-- Scoped vocabulary entries for transcript display/export/summary correction.

CREATE TABLE IF NOT EXISTS vocabulary_entries (
  id TEXT PRIMARY KEY,
  scope_type TEXT NOT NULL CHECK (scope_type IN ('global', 'meeting')),
  scope_id TEXT,
  source_text TEXT NOT NULL,
  target_text TEXT NOT NULL,
  case_sensitive INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_vocabulary_entries_scope
ON vocabulary_entries(scope_type, scope_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_vocabulary_entries_scope_source_ci
ON vocabulary_entries(scope_type, COALESCE(scope_id, ''), lower(source_text))
WHERE case_sensitive = 0;

CREATE UNIQUE INDEX IF NOT EXISTS idx_vocabulary_entries_scope_source_cs
ON vocabulary_entries(scope_type, COALESCE(scope_id, ''), source_text)
WHERE case_sensitive = 1;
