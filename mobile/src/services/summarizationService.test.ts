import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock authService
vi.mock('./authService', () => ({
  authFetch: vi.fn().mockResolvedValue({
    ok: true,
    json: () => Promise.resolve({}),
    status: 200,
  }),
}))

import { startSummarization, pollSummaryStatus } from './summarizationService'
import { authFetch } from './authService'

describe('summarizationService', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('startSummarization', () => {
    it('sends correct payload with defaults', async () => {
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ meeting_id: 'meeting-1' }),
      } as Response)

      const result = await startSummarization('meeting-1')

      expect(result).toBe('meeting-1')
      expect(authFetch).toHaveBeenCalledWith('/api/summarize', {
        method: 'POST',
        body: JSON.stringify({
          meeting_id: 'meeting-1',
          provider: 'claude',
          model: undefined,
          custom_prompt: undefined,
        }),
      })
    })

    it('sends custom options', async () => {
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ meeting_id: 'meeting-2' }),
      } as Response)

      await startSummarization('meeting-2', {
        provider: 'groq',
        model: 'llama-3',
        customPrompt: 'Summarize briefly',
      })

      expect(authFetch).toHaveBeenCalledWith('/api/summarize', {
        method: 'POST',
        body: JSON.stringify({
          meeting_id: 'meeting-2',
          provider: 'groq',
          model: 'llama-3',
          custom_prompt: 'Summarize briefly',
        }),
      })
    })

    it('throws on failure with error detail', async () => {
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: false,
        status: 400,
        json: () => Promise.resolve({ detail: 'No transcript available' }),
      } as Response)

      await expect(startSummarization('meeting-1')).rejects.toThrow(
        'No transcript available',
      )
    })

    it('throws with fallback message when error JSON parse fails', async () => {
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: false,
        status: 500,
        json: () => Promise.reject(new Error('not json')),
      } as Response)

      await expect(startSummarization('meeting-1')).rejects.toThrow(
        'Summarization failed',
      )
    })

    it('throws with status code when no detail in response', async () => {
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: false,
        status: 503,
        json: () => Promise.resolve({}),
      } as Response)

      await expect(startSummarization('meeting-1')).rejects.toThrow(
        'Summarization failed: 503',
      )
    })
  })

  describe('pollSummaryStatus', () => {
    it('calls correct endpoint and returns status', async () => {
      const mockStatus = {
        status: 'completed',
        meeting_id: 'meeting-1',
        data: { MeetingName: 'Standup' },
      }
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve(mockStatus),
      } as Response)

      const result = await pollSummaryStatus('meeting-1')

      expect(authFetch).toHaveBeenCalledWith('/api/summarize/meeting-1/status')
      expect(result).toEqual(mockStatus)
    })

    it('throws on non-ok response', async () => {
      vi.mocked(authFetch).mockResolvedValueOnce({
        ok: false,
        status: 404,
      } as Response)

      await expect(pollSummaryStatus('meeting-bad')).rejects.toThrow(
        'Summary status check failed: 404',
      )
    })
  })
})
