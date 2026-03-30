/**
 * Centralized configuration for the mobile app.
 *
 * API URL resolution priority:
 * 1. NEXT_PUBLIC_CLOUD_API_URL env var (set at build time)
 * 2. Runtime detection from Capacitor config (future)
 * 3. Fallback defaults per environment
 */

function detectEnvironment(): 'production' | 'staging' | 'development' {
  const envMode = process.env.NEXT_PUBLIC_APP_ENV
  if (envMode === 'production' || envMode === 'staging') return envMode

  // In dev server
  if (process.env.NODE_ENV === 'development') return 'development'

  // Static export (cap build) defaults to production
  return 'production'
}

const ENV_DEFAULTS: Record<string, string> = {
  production: 'https://live-iqcapture-api-ekg4haafg2gubegb.uksouth-01.azurewebsites.net',
  staging: 'https://api-staging.iqcapture.app',
  development: 'http://localhost:5167',
}

// Override: allow runtime API URL from Capacitor server config or env
// This enables testing on physical devices against a local network backend
const RUNTIME_API_URL = typeof window !== 'undefined'
  ? (window as any).__IQ_API_URL
  : undefined

const environment = detectEnvironment()

export const config = {
  environment,

  /** Cloud API base URL (no trailing slash) */
  apiUrl: (process.env.NEXT_PUBLIC_CLOUD_API_URL || ENV_DEFAULTS[environment] || '').replace(/\/$/, ''),

  /** App version */
  version: '0.1.0',

  /** Usage event flush interval (ms) */
  usageFlushInterval: environment === 'development' ? 30_000 : 60_000,

  /** Sync interval (ms) */
  syncInterval: environment === 'development' ? 30_000 : 60_000,

  /** Transcription polling interval (ms) */
  transcriptionPollInterval: 3_000,
}
