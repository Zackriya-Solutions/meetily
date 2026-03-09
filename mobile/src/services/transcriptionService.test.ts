import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock authService
vi.mock('./authService', () => ({
  getAccessToken: vi.fn().mockResolvedValue('mock-token-123'),
  authFetch: vi.fn().mockResolvedValue({
    ok: true,
    json: () => Promise.resolve({}),
    status: 200,
  }),
}))

// Mock config
vi.mock('./config', () => ({
  config: {
    apiUrl: 'https://api.test.com',
  },
}))

import {
  uploadForTranscription,
  pollTranscriptionStatus,
  getTranscriptionQuota,
} from './transcriptionService'
import { getAccessToken, authFetch } from './authService'

describe('transcriptionService', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('uploadForTranscription', () => {
    it('creates FormData and sends with auth header', async () => {
      const mockBlob = new Blob(['audio-data'], { type: 'audio/webm' })
      const mockFetch = vi.fn()
        // First call: fetch the audio URI
        .mockResolvedValueOnce({
          ok: true,
          blob: () => Promise.resolve(mockBlob),
        })
        // Second call: upload to API
        .mockResolvedValueOnce({
          ok: true,
          json: () => Promise.resolve({ transcription_id: 'txn-456' }),
        })
      global.fetch = mockFetch

      const result = await uploadForTranscription('file:///audio.webm', 'meeting-1', {
        language: 'en',
        provider: 'deepgram',
      })

      expect(result).toBe('txn-456')

      // First fetch: audio file
      expect(mockFetch).toHaveBeenCalledWith('file:///audio.webm')

      // Second fetch: upload
      const uploadCall = mockFetch.mock.calls[1]
      expect(uploadCall[0]).toBe('https://api.test.com/api/transcription/upload')
      expect(uploadCall[1].method).toBe('POST')
      expect(uploadCall[1].headers.Authorization).toBe('Bearer mock-token-123')
      expect(uploadCall[1].body).toBeInstanceOf(FormData)
    })

    it('throws when not authenticated', async () => {
      vi.mocked(getAccessToken).mockResolvedValueOnce(null)

      await expect(
        uploadForTranscription('file:///audio.webm', 'meeting-1'),
      ).rejects.toThrow('Not authenticated')
    })

    it('throws on upload failure', async () => {
      const mockFetch = vi.fn()
        .mockResolvedValueOnce({
          ok: true,
          blob: () => Promise.resolve(new Blob(['data'])),
        })
        .mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: () => Promise.resolve({ detail: 'Server error' }),
        })
      global.fetch = mockFetch

      await expect(
        uploadForTranscription('file:///audio.webm', 'meeting-1'),
      ).rejects.toThrow('Server error')
    })

    it('handles non-JSON error response', async () => {
      const mockFetch = vi.fn()
        .mockResolvedValueOnce({
          ok: true,
          blob: () => Promise.resolve(new Blob(['data'])),
        })
        .mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: () => Promise.reject(new Error('not json')),
        })
      global.fetch = mockFetch

      await expect(
        uploadForTranscription('file:///audio.webm', 'meeting-1'),
      ).rejects.toThrow('Upload failed')
    })

    it('sends optional provider and language in FormData', async () => {
      const mockFetch = vi.fn()
        .mockResolvedValueOnce({
          ok: true,
          blob: () => Promise.resolve(new Blob(['data'])),
        })
        .mockResolvedValueOnce({
          ok: true,
          json: () => Promise.resolve({ transcription_id: 'txn-789' }),
        })
      global.fetch = mockFetch

      await uploadForTranscription('file:///audio.webm', 'meeting-1', {
        provider: 'whisper',
        language: 'fr',
      })

      const formData = mockFetch.mock.calls[1][1].body as FormData
      expect(formData.get('meeting_id')).toBe('meeting-1')
      expect(formData.get('provider')).toBe('whisper')
      expect(formData.get('language')).toBe('fr')
    })
  })

  describe('pollTranscriptionStatus', () => {
    it('calls correct endpoint and returns status', async () => {
      const mockStatus = {
        id: 'txn-123',
        status: 'completed',
        transcript: { text: 'Hello world', segments: [], duration_seconds: 60, language: 'en' },
      }
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve(mockStatus),
      } as Response)

      const result = await pollTranscriptionStatus('txn-123')

      expect(authFetch).toHaveBeenCalledWith('/api/transcription/txn-123/status')
      expect(result).toEqual(mockStatus)
    })

    it('throws on non-ok response', async () => {
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: false,
        status: 404,
      } as Response)

      await expect(pollTranscriptionStatus('txn-bad')).rejects.toThrow(
        'Status check failed: 404',
      )
    })
  })

  describe('getTranscriptionQuota', () => {
    it('returns quota data', async () => {
      const quotaData = { remaining_minutes: 100, plan_limit: 200, used_minutes: 100 }
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve(quotaData),
      } as Response)

      const result = await getTranscriptionQuota()

      expect(authFetch).toHaveBeenCalledWith('/api/transcription/quota')
      expect(result).toEqual(quotaData)
    })

    it('throws on failure', async () => {
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: false,
        status: 500,
      } as Response)

      await expect(getTranscriptionQuota()).rejects.toThrow('Quota check failed')
    })
  })
})
