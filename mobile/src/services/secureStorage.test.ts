import { describe, it, expect, vi, beforeEach } from 'vitest'
import { Preferences } from '@capacitor/preferences'
import { secureGet, secureSet, secureRemove } from './secureStorage'

describe('secureStorage', () => {
  // secure-storage is mocked to throw in setup.ts, so it's unavailable.
  // We import the module statically (vi.mock hoisting handles the mock before import).

  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('secureGet', () => {
    it('falls back to Preferences when secure storage is unavailable', async () => {
      ;(Preferences.get as any).mockResolvedValueOnce({ value: 'pref-value' })

      const result = await secureGet('test_key')
      expect(result).toBe('pref-value')
      expect(Preferences.get).toHaveBeenCalledWith({ key: 'test_key' })
    })

    it('falls back to localStorage when Preferences returns null', async () => {
      ;(Preferences.get as any).mockResolvedValueOnce({ value: null })
      localStorage.setItem('test_key', 'local-value')

      const result = await secureGet('test_key')
      expect(result).toBe('local-value')
    })

    it('falls back to localStorage when Preferences throws', async () => {
      ;(Preferences.get as any).mockRejectedValueOnce(new Error('fail'))
      localStorage.setItem('test_key', 'local-fallback')

      const result = await secureGet('test_key')
      expect(result).toBe('local-fallback')
    })

    it('returns null when all backends fail', async () => {
      ;(Preferences.get as any).mockRejectedValueOnce(new Error('fail'))
      // localStorage is empty (cleared in beforeEach of setup.ts)

      const result = await secureGet('nonexistent')
      expect(result).toBeNull()
    })
  })

  describe('secureSet', () => {
    it('writes to Preferences when secure storage is unavailable', async () => {
      await secureSet('key1', 'val1')
      expect(Preferences.set).toHaveBeenCalledWith({ key: 'key1', value: 'val1' })
    })

    it('falls back to localStorage when Preferences throws', async () => {
      ;(Preferences.set as any).mockRejectedValueOnce(new Error('fail'))

      await secureSet('key2', 'val2')
      expect(localStorage.setItem).toHaveBeenCalledWith('key2', 'val2')
    })
  })

  describe('secureRemove', () => {
    it('removes from all backends', async () => {
      localStorage.setItem('cleanup_key', 'data')

      await secureRemove('cleanup_key')

      expect(Preferences.remove).toHaveBeenCalledWith({ key: 'cleanup_key' })
      expect(localStorage.removeItem).toHaveBeenCalledWith('cleanup_key')
    })

    it('does not throw if Preferences.remove fails', async () => {
      ;(Preferences.remove as any).mockRejectedValueOnce(new Error('fail'))

      await expect(secureRemove('key')).resolves.toBeUndefined()
      expect(localStorage.removeItem).toHaveBeenCalledWith('key')
    })
  })
})
