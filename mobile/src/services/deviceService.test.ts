import { describe, it, expect, vi, beforeEach } from 'vitest'
import { Preferences } from '@capacitor/preferences'

// Mock uuid to return deterministic values
vi.mock('uuid', () => ({
  v4: vi.fn(() => 'mock-uuid-1234'),
}))

describe('deviceService', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.clearAllMocks()
  })

  async function loadModule() {
    return import('./deviceService')
  }

  describe('getDeviceId()', () => {
    it('generates and persists a UUID device ID via Preferences', async () => {
      const { getDeviceId } = await loadModule()
      ;(Preferences.get as any).mockResolvedValueOnce({ value: null })

      const id = await getDeviceId()

      expect(id).toBe('mock-uuid-1234')
      expect(Preferences.set).toHaveBeenCalledWith({ key: 'device_id', value: 'mock-uuid-1234' })
    })

    it('returns existing device ID from Preferences', async () => {
      const { getDeviceId } = await loadModule()
      ;(Preferences.get as any).mockResolvedValueOnce({ value: 'existing-id' })

      const id = await getDeviceId()
      expect(id).toBe('existing-id')
      expect(Preferences.set).not.toHaveBeenCalled()
    })

    it('returns cached device ID on subsequent calls', async () => {
      const { getDeviceId } = await loadModule()
      ;(Preferences.get as any).mockResolvedValueOnce({ value: 'cached-id' })

      const id1 = await getDeviceId()
      const id2 = await getDeviceId()

      expect(id1).toBe('cached-id')
      expect(id2).toBe('cached-id')
      // Preferences.get should only be called once (cached after first call)
      expect(Preferences.get).toHaveBeenCalledTimes(1)
    })

    it('falls back to localStorage when Preferences throws', async () => {
      const { getDeviceId } = await loadModule()
      ;(Preferences.get as any).mockRejectedValueOnce(new Error('not available'))
      localStorage.setItem('device_id', 'local-stored-id')

      const id = await getDeviceId()
      expect(id).toBe('local-stored-id')
    })

    it('generates new ID and saves to localStorage when Preferences fails', async () => {
      const { getDeviceId } = await loadModule()
      ;(Preferences.get as any).mockRejectedValueOnce(new Error('not available'))
      // localStorage is empty (cleared in setup beforeEach)
      ;(Preferences.set as any).mockRejectedValueOnce(new Error('not available'))

      const id = await getDeviceId()
      expect(id).toBe('mock-uuid-1234')
      expect(localStorage.setItem).toHaveBeenCalledWith('device_id', 'mock-uuid-1234')
    })
  })

  describe('getPlatform()', () => {
    const originalUserAgent = navigator.userAgent

    afterEach(() => {
      Object.defineProperty(navigator, 'userAgent', {
        value: originalUserAgent,
        writable: true,
        configurable: true,
      })
    })

    it('detects iOS from iPhone user agent', async () => {
      Object.defineProperty(navigator, 'userAgent', {
        value: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)',
        writable: true,
        configurable: true,
      })
      const { getPlatform } = await loadModule()
      expect(getPlatform()).toBe('ios')
    })

    it('detects iOS from iPad user agent', async () => {
      Object.defineProperty(navigator, 'userAgent', {
        value: 'Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X)',
        writable: true,
        configurable: true,
      })
      const { getPlatform } = await loadModule()
      expect(getPlatform()).toBe('ios')
    })

    it('detects Android', async () => {
      Object.defineProperty(navigator, 'userAgent', {
        value: 'Mozilla/5.0 (Linux; Android 14; Pixel 8)',
        writable: true,
        configurable: true,
      })
      const { getPlatform } = await loadModule()
      expect(getPlatform()).toBe('android')
    })

    it('returns "mobile" for unknown user agents', async () => {
      Object.defineProperty(navigator, 'userAgent', {
        value: 'Mozilla/5.0 (X11; Linux x86_64)',
        writable: true,
        configurable: true,
      })
      const { getPlatform } = await loadModule()
      expect(getPlatform()).toBe('mobile')
    })
  })
})
