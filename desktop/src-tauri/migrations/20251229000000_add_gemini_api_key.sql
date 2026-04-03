-- Migration: Add Gemini API Key to settings table
-- Adds support for Google Gemini AI provider

ALTER TABLE settings ADD COLUMN geminiApiKey TEXT;
