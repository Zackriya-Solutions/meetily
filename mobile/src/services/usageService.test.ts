import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

// Mock uuid
vi.mock('uuid', () => ({
  v4: vi.fn(() => 'mock-event-uuid'),
}))

// Mock authService
vi.mock('./authService', () => ({
  getAccessToken: vi.fn(() => Promise.resolve('mock-token')),
  authFetch: vi.fn(() =>
    Promise.resolve({
      ok: true,
      json: () => Promise.resolve({ ingested: 2 }),
    }),
  ),
}))

// Mock deviceService
vi.mock('./deviceService', () => ({
  getDeviceId: vi.fn(() => Promise.resolve('mock-device-id')),
}))

import {
  trackEvent,
  trackMeetingCreated,
  trackSummaryGenerated,
  trackCloudTranscriptionMinutes,
  trackAudioUploadBytes,
  trackSessionStarted,
  trackSessionEnded,
  flushEvents,
  startPeriodicFlush,
  stopPeriodicFlush,
  initUsageService,
} from './usageService'
import { getAccessToken, authFetch } from './authService'

const mockAuthFetch = authFetch as ReturnType<typeof vi.fn>
const mockGetAccessToken = getAccessToken as ReturnType<typeof vi.fn>

describe('usageService', () => {
  beforeEach(async () => {
    vi.useFakeTimers()
    stopPeriodicFlush()
    // Drain any leftover events from previous tests by flushing with a valid token
    await flushEvents()
    vi.clearAllMocks()
  })

  afterEach(() => {
    stopPeriodicFlush()
    vi.useRealTimers()
  })

  describe('trackEvent()', () => {
    it('adds event to buffer with client_event_id and timestamp', async () => {
      trackEvent('test_event', 42, { foo: 'bar' })

      await flushEvents()

      expect(mockAuthFetch).toHaveBeenCalledWith(
        '/api/usage/events',
        expect.objectContaining({
          method: 'POST',
          body: expect.stringContaining('"event_type":"test_event"'),
        }),
      )
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.device_id).toBe('mock-device-id')
      expect(body.events[0].client_event_id).toBe('mock-event-uuid')
      expect(body.events[0].value).toBe(42)
      expect(body.events[0].metadata).toEqual({ foo: 'bar' })
      expect(body.events[0].timestamp).toBeDefined()
    })
  })

  describe('convenience trackers', () => {
    it('trackMeetingCreated calls trackEvent with meeting_created', async () => {
      trackMeetingCreated('m-1')
      await flushEvents()
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events[0].event_type).toBe('meeting_created')
      expect(body.events[0].metadata.meeting_id).toBe('m-1')
    })

    it('trackMeetingCreated works without meetingId', async () => {
      trackMeetingCreated()
      await flushEvents()
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events[0].event_type).toBe('meeting_created')
      expect(body.events[0].metadata).toBeUndefined()
    })

    it('trackSummaryGenerated includes llm info', async () => {
      trackSummaryGenerated('m-2', 'openai', 'gpt-4')
      await flushEvents()
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events[0].event_type).toBe('summary_generated')
      expect(body.events[0].metadata.llm_provider).toBe('openai')
      expect(body.events[0].metadata.llm_model).toBe('gpt-4')
    })

    it('trackCloudTranscriptionMinutes passes minutes as value', async () => {
      trackCloudTranscriptionMinutes('m-3', 5.5)
      await flushEvents()
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events[0].event_type).toBe('cloud_transcription_minutes')
      expect(body.events[0].value).toBe(5.5)
    })

    it('trackAudioUploadBytes passes bytes as value', async () => {
      trackAudioUploadBytes('m-4', 1024)
      await flushEvents()
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events[0].event_type).toBe('audio_upload_bytes')
      expect(body.events[0].value).toBe(1024)
    })

    it('trackSessionStarted sends value 1', async () => {
      trackSessionStarted()
      await flushEvents()
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events[0].event_type).toBe('session_started')
      expect(body.events[0].value).toBe(1)
    })

    it('trackSessionEnded sends duration as value', async () => {
      trackSessionEnded(30)
      await flushEvents()
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events[0].event_type).toBe('session_ended')
      expect(body.events[0].value).toBe(30)
    })
  })

  describe('flushEvents()', () => {
    it('does nothing when buffer is empty', async () => {
      await flushEvents()
      expect(mockAuthFetch).not.toHaveBeenCalled()
    })

    it('does nothing when no access token', async () => {
      mockGetAccessToken.mockResolvedValueOnce(null)
      trackEvent('test', 1)
      await flushEvents()
      expect(mockAuthFetch).not.toHaveBeenCalled()
    })

    it('sends buffer via authFetch and clears on success', async () => {
      trackEvent('ev1', 1)
      trackEvent('ev2', 2)

      await flushEvents()

      expect(mockAuthFetch).toHaveBeenCalledTimes(1)
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events).toHaveLength(2)

      // Buffer should be empty now - flushing again should do nothing
      vi.clearAllMocks()
      await flushEvents()
      expect(mockAuthFetch).not.toHaveBeenCalled()
    })

    it('re-buffers events on failure', async () => {
      trackEvent('ev-fail', 1)

      mockAuthFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
        json: () => Promise.resolve({}),
      })

      await flushEvents()

      // Events should be re-buffered - flushing again should send them
      vi.clearAllMocks()
      mockAuthFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ ingested: 1 }),
      })

      await flushEvents()
      expect(mockAuthFetch).toHaveBeenCalledTimes(1)
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      expect(body.events[0].event_type).toBe('ev-fail')
    })

    it('handles fetch exceptions gracefully', async () => {
      trackEvent('ev-exc', 1)
      mockAuthFetch.mockRejectedValueOnce(new Error('network error'))

      // Should not throw
      await expect(flushEvents()).resolves.toBeUndefined()
    })
  })

  describe('startPeriodicFlush / stopPeriodicFlush', () => {
    it('starts a periodic interval that calls flushEvents', async () => {
      trackEvent('periodic', 1)
      startPeriodicFlush(1000)

      await vi.advanceTimersByTimeAsync(1000)

      expect(mockAuthFetch).toHaveBeenCalled()

      stopPeriodicFlush()
    })

    it('does not create duplicate intervals', () => {
      startPeriodicFlush(1000)
      startPeriodicFlush(1000) // second call should be no-op

      trackEvent('dup-test', 1)
      vi.advanceTimersByTime(1000)

      stopPeriodicFlush()
    })

    it('stopPeriodicFlush clears the interval', async () => {
      startPeriodicFlush(1000)
      stopPeriodicFlush()

      trackEvent('stopped', 1)
      await vi.advanceTimersByTimeAsync(2000)

      // authFetch should not be called since we stopped
      expect(mockAuthFetch).not.toHaveBeenCalled()
    })

    it('stopPeriodicFlush is safe to call when not started', () => {
      expect(() => stopPeriodicFlush()).not.toThrow()
    })
  })

  describe('initUsageService()', () => {
    it('tracks session_started and starts periodic flush', async () => {
      initUsageService()

      // Session started event should be buffered
      // Trigger flush via the periodic interval
      await vi.advanceTimersByTimeAsync(60_000)

      expect(mockAuthFetch).toHaveBeenCalled()
      const body = JSON.parse((mockAuthFetch.mock.calls[0][1] as any).body)
      const sessionEvent = body.events.find((e: any) => e.event_type === 'session_started')
      expect(sessionEvent).toBeDefined()

      stopPeriodicFlush()
    })
  })
})
