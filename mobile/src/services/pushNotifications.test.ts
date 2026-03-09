import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  requestNotificationPermission,
  notifyTranscriptionComplete,
  notifySummaryComplete,
  initNotifications,
} from './pushNotifications'

describe('pushNotifications', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('requestNotificationPermission', () => {
    it('returns false when local notifications plugin is unavailable', async () => {
      // The setup.ts mocks @capacitor/local-notifications to throw
      const result = await requestNotificationPermission()
      expect(result).toBe(false)
    })
  })

  describe('notifyTranscriptionComplete', () => {
    it('handles gracefully when schedule rejects', async () => {
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

      await notifyTranscriptionComplete('Team Standup', 'meeting-1')

      // Plugin is available but schedule rejects — hits catch with console.warn
      expect(warnSpy).toHaveBeenCalledWith(
        '[Notifications] Failed to schedule:',
        expect.any(Error),
      )

      warnSpy.mockRestore()
    })
  })

  describe('notifySummaryComplete', () => {
    it('handles gracefully when schedule rejects', async () => {
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

      await notifySummaryComplete('Weekly Review', 'meeting-2')

      expect(warnSpy).toHaveBeenCalledWith(
        '[Notifications] Failed to schedule:',
        expect.any(Error),
      )

      warnSpy.mockRestore()
    })
  })

  describe('initNotifications', () => {
    it('calls requestPermission and handles false result', async () => {
      const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})

      await initNotifications()

      // Since plugin is unavailable, permission is false, so no "granted" log
      expect(logSpy).not.toHaveBeenCalledWith('[Notifications] Permission granted')

      logSpy.mockRestore()
    })
  })
})
