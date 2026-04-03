-- Migration: Add Custom OpenAI Configuration

-- This column stores: {endpoint, apiKey, model, maxTokens, temperature, topP}
-- Allows users to configure their own OpenAI-compatible endpoints
ALTER TABLE settings ADD COLUMN customOpenAIConfig TEXT;
