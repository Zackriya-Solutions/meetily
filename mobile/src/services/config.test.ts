import { describe, it, expect, vi, beforeEach } from 'vitest'

describe('config', () => {
  const originalEnv = { ...process.env }

  beforeEach(() => {
    // Reset module cache so detectEnvironment() runs fresh
    vi.resetModules()
    process.env = { ...originalEnv }
  })

  afterAll(() => {
    process.env = originalEnv
  })

  async function loadConfig() {
    const mod = await import('./config')
    return mod.config
  }

  describe('environment detection', () => {
    it('returns "production" when NEXT_PUBLIC_APP_ENV is production', async () => {
      process.env.NEXT_PUBLIC_APP_ENV = 'production'
      const config = await loadConfig()
      expect(config.environment).toBe('production')
    })

    it('returns "staging" when NEXT_PUBLIC_APP_ENV is staging', async () => {
      process.env.NEXT_PUBLIC_APP_ENV = 'staging'
      const config = await loadConfig()
      expect(config.environment).toBe('staging')
    })

    it('returns "development" when NODE_ENV is development', async () => {
      delete process.env.NEXT_PUBLIC_APP_ENV
      process.env.NODE_ENV = 'development'
      const config = await loadConfig()
      expect(config.environment).toBe('development')
    })

    it('defaults to "production" when no env hints are present', async () => {
      delete process.env.NEXT_PUBLIC_APP_ENV
      process.env.NODE_ENV = 'production'
      const config = await loadConfig()
      expect(config.environment).toBe('production')
    })
  })

  describe('API URL resolution', () => {
    it('uses NEXT_PUBLIC_CLOUD_API_URL when set', async () => {
      process.env.NEXT_PUBLIC_CLOUD_API_URL = 'https://custom.api.example.com'
      const config = await loadConfig()
      expect(config.apiUrl).toBe('https://custom.api.example.com')
    })

    it('strips trailing slash from NEXT_PUBLIC_CLOUD_API_URL', async () => {
      process.env.NEXT_PUBLIC_CLOUD_API_URL = 'https://custom.api.example.com/'
      const config = await loadConfig()
      expect(config.apiUrl).toBe('https://custom.api.example.com')
    })

    it('falls back to production URL', async () => {
      delete process.env.NEXT_PUBLIC_CLOUD_API_URL
      process.env.NEXT_PUBLIC_APP_ENV = 'production'
      const config = await loadConfig()
      expect(config.apiUrl).toBe('https://api.iqcapture.app')
    })

    it('falls back to staging URL', async () => {
      delete process.env.NEXT_PUBLIC_CLOUD_API_URL
      process.env.NEXT_PUBLIC_APP_ENV = 'staging'
      const config = await loadConfig()
      expect(config.apiUrl).toBe('https://api-staging.iqcapture.app')
    })

    it('falls back to development URL', async () => {
      delete process.env.NEXT_PUBLIC_CLOUD_API_URL
      delete process.env.NEXT_PUBLIC_APP_ENV
      process.env.NODE_ENV = 'development'
      const config = await loadConfig()
      expect(config.apiUrl).toBe('http://localhost:5167')
    })
  })

  describe('static config values', () => {
    it('has version 0.1.0', async () => {
      const config = await loadConfig()
      expect(config.version).toBe('0.1.0')
    })

    it('has transcriptionPollInterval of 3000ms', async () => {
      const config = await loadConfig()
      expect(config.transcriptionPollInterval).toBe(3_000)
    })
  })

  describe('interval values per environment', () => {
    it('uses shorter intervals in development', async () => {
      delete process.env.NEXT_PUBLIC_APP_ENV
      process.env.NODE_ENV = 'development'
      const config = await loadConfig()
      expect(config.usageFlushInterval).toBe(30_000)
      expect(config.syncInterval).toBe(30_000)
    })

    it('uses longer intervals in production', async () => {
      process.env.NEXT_PUBLIC_APP_ENV = 'production'
      const config = await loadConfig()
      expect(config.usageFlushInterval).toBe(60_000)
      expect(config.syncInterval).toBe(60_000)
    })
  })
})
