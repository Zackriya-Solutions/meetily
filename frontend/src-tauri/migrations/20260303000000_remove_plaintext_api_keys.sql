-- Migration: Remove plaintext API keys from database
-- API keys are now stored in the platform keychain (macOS Keychain, etc.)
-- This migration clears any remaining plaintext keys from the database.
-- The application will read keys from the keychain going forward.

UPDATE settings SET
    groqApiKey = NULL,
    openaiApiKey = NULL,
    anthropicApiKey = NULL,
    ollamaApiKey = NULL,
    openRouterApiKey = NULL
WHERE id = '1';

UPDATE transcript_settings SET
    whisperApiKey = NULL,
    deepgramApiKey = NULL,
    elevenLabsApiKey = NULL,
    groqApiKey = NULL,
    openaiApiKey = NULL
WHERE id = '1';
