import { describe, it, expect, vi, beforeEach } from 'vitest'
import { Preferences } from '@capacitor/preferences'
import {
  isBiometricAvailable,
  getBiometricType,
  authenticateWithBiometrics,
  isBiometricEnabled,
  setBiometricEnabled,
  checkBiometricOnResume,
} from './biometricAuth'

describe('biometricAuth', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('isBiometricAvailable', () => {
    it('returns false when biometric plugin is unavailable', async () => {
      // The setup.ts mocks @aparajita/capacitor-biometric-auth to throw
      const result = await isBiometricAvailable()
      expect(result).toBe(false)
    })
  })

  describe('getBiometricType', () => {
    it('returns "Biometrics" when plugin is unavailable', async () => {
      const result = await getBiometricType()
      expect(result).toBe('Biometrics')
    })
  })

  describe('authenticateWithBiometrics', () => {
    it('returns true (passthrough) when plugin is unavailable', async () => {
      const result = await authenticateWithBiometrics()
      expect(result).toBe(true)
    })

    it('returns true with custom reason when plugin is unavailable', async () => {
      const result = await authenticateWithBiometrics('Custom reason')
      expect(result).toBe(true)
    })
  })

  describe('isBiometricEnabled', () => {
    it('returns false when no preference is set', async () => {
      const result = await isBiometricEnabled()
      expect(result).toBe(false)
      expect(Preferences.get).toHaveBeenCalledWith({ key: 'biometric_enabled' })
    })

    it('returns true when preference is "true"', async () => {
      vi.mocked(Preferences.get).mockResolvedValueOnce({ value: 'true' })

      const result = await isBiometricEnabled()
      expect(result).toBe(true)
    })

    it('returns false when preference is "false"', async () => {
      vi.mocked(Preferences.get).mockResolvedValueOnce({ value: 'false' })

      const result = await isBiometricEnabled()
      expect(result).toBe(false)
    })

    it('returns false when Preferences.get throws', async () => {
      vi.mocked(Preferences.get).mockRejectedValueOnce(new Error('Storage error'))

      const result = await isBiometricEnabled()
      expect(result).toBe(false)
    })
  })

  describe('setBiometricEnabled', () => {
    it('sets preference to "true" when enabled', async () => {
      await setBiometricEnabled(true)
      expect(Preferences.set).toHaveBeenCalledWith({
        key: 'biometric_enabled',
        value: 'true',
      })
    })

    it('sets preference to "false" when disabled', async () => {
      await setBiometricEnabled(false)
      expect(Preferences.set).toHaveBeenCalledWith({
        key: 'biometric_enabled',
        value: 'false',
      })
    })
  })

  describe('checkBiometricOnResume', () => {
    it('returns true when biometric is not enabled', async () => {
      vi.mocked(Preferences.get).mockResolvedValueOnce({ value: null })

      const result = await checkBiometricOnResume()
      expect(result).toBe(true)
    })

    it('returns true when biometric is enabled but not available (plugin missing)', async () => {
      vi.mocked(Preferences.get).mockResolvedValueOnce({ value: 'true' })

      const result = await checkBiometricOnResume()
      // Plugin unavailable -> isBiometricAvailable returns false -> returns true
      expect(result).toBe(true)
    })
  })
})
