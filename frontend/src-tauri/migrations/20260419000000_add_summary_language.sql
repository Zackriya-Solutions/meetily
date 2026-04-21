-- Migration: Add summary language preference to settings table
-- Stores BCP-47 language tag (e.g. "en-GB", "en-US", "es", "zh") to direct
-- the LLM to produce summaries in a specific output language.
-- NULL preserves current behaviour (model decides based on transcript).
ALTER TABLE settings ADD COLUMN summaryLanguage TEXT;
