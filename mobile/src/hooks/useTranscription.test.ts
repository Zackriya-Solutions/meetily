import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { useTranscription } from './useTranscription'

// ── Mock services ──
vi.mock('@/services/transcriptionService', () => ({
  pollTranscriptionStatus: vi.fn(),
}))

vi.mock('@/services/meetingRepository', () => ({
  meetingRepository: {
    getMeeting: vi.fn(),
    updateMeeting: vi.fn(),
  },
}))

vi.mock('@/services/pushNotifications', () => ({
  notifyTranscriptionComplete: vi.fn(),
}))

import { pollTranscriptionStatus } from '@/services/transcriptionService'
import { meetingRepository } from '@/services/meetingRepository'
import { notifyTranscriptionComplete } from '@/services/pushNotifications'

// Helper to flush all pending microtasks (resolved promises) under fake timers
const flushPromises = () => act(async () => { await vi.advanceTimersByTimeAsync(0) })

describe('useTranscription', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useFakeTimers()
    vi.mocked(meetingRepository.getMeeting).mockResolvedValue({
      meeting_id: 'm1',
      title: 'Test Meeting',
      created_at: '2025-01-01T00:00:00Z',
      updated_at: '2025-01-01T00:00:00Z',
      status: 'transcribing',
      sync_status: 'synced',
      version: 1,
    })
    vi.mocked(meetingRepository.updateMeeting).mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('does nothing when transcriptionId is null', () => {
    const { result } = renderHook(() => useTranscription(null, null))

    expect(result.current.status).toBeNull()
    expect(result.current.isPolling).toBe(false)
    expect(pollTranscriptionStatus).not.toHaveBeenCalled()
  })

  it('starts polling when transcriptionId is provided', async () => {
    vi.mocked(pollTranscriptionStatus).mockResolvedValue({
      id: 't1',
      status: 'processing',
      progress: 50,
    })

    const { result } = renderHook(() => useTranscription('t1', 'm1'))

    // Should start polling immediately
    expect(result.current.isPolling).toBe(true)

    await flushPromises()

    expect(pollTranscriptionStatus).toHaveBeenCalledWith('t1')
    expect(result.current.status).toEqual({
      id: 't1',
      status: 'processing',
      progress: 50,
    })
  })

  it('updates meeting when transcription completes', async () => {
    const completedStatus = {
      id: 't1',
      status: 'completed' as const,
      transcript: {
        text: 'Hello world',
        segments: [{ text: 'Hello world', start: 0, end: 1, confidence: 0.95 }],
        duration_seconds: 60,
        language: 'en',
      },
    }
    vi.mocked(pollTranscriptionStatus).mockResolvedValue(completedStatus)

    const { result } = renderHook(() => useTranscription('t1', 'm1'))

    await flushPromises()

    expect(result.current.status?.status).toBe('completed')
    expect(meetingRepository.getMeeting).toHaveBeenCalledWith('m1')
    expect(meetingRepository.updateMeeting).toHaveBeenCalledWith('m1', {
      transcript_text: 'Hello world',
      transcript_segments: [{ text: 'Hello world', start: 0, end: 1, confidence: 0.95 }],
      duration_seconds: 60,
      status: 'completed',
    })
    expect(notifyTranscriptionComplete).toHaveBeenCalledWith('Test Meeting', 'm1')
    expect(result.current.isPolling).toBe(false)
  })

  it('stops polling on completion', async () => {
    vi.mocked(pollTranscriptionStatus).mockResolvedValue({
      id: 't1',
      status: 'completed',
      transcript: {
        text: 'Done',
        segments: [],
        duration_seconds: 30,
        language: 'en',
      },
    })

    const { result } = renderHook(() => useTranscription('t1', 'm1'))

    await flushPromises()

    expect(result.current.isPolling).toBe(false)

    // Clear mock call count and advance timer — should not poll again
    vi.mocked(pollTranscriptionStatus).mockClear()

    await act(async () => {
      vi.advanceTimersByTime(10000)
    })

    expect(pollTranscriptionStatus).not.toHaveBeenCalled()
  })

  it('stops polling on failure and marks meeting as error', async () => {
    vi.mocked(pollTranscriptionStatus).mockResolvedValue({
      id: 't1',
      status: 'failed',
      error: 'Transcription failed',
    })

    const { result } = renderHook(() => useTranscription('t1', 'm1'))

    await flushPromises()

    expect(result.current.status?.status).toBe('failed')
    expect(result.current.isPolling).toBe(false)
    expect(meetingRepository.updateMeeting).toHaveBeenCalledWith('m1', { status: 'error' })
  })

  it('continues polling when status is processing', async () => {
    vi.mocked(pollTranscriptionStatus).mockResolvedValue({
      id: 't1',
      status: 'processing',
      progress: 50,
    })

    const { result } = renderHook(() => useTranscription('t1', 'm1'))

    await flushPromises()

    expect(result.current.status?.status).toBe('processing')

    // Still polling
    expect(result.current.isPolling).toBe(true)

    // Advance timer to trigger another poll
    vi.mocked(pollTranscriptionStatus).mockClear()
    vi.mocked(pollTranscriptionStatus).mockResolvedValue({
      id: 't1',
      status: 'processing',
      progress: 75,
    })

    await act(async () => {
      vi.advanceTimersByTime(3000)
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(pollTranscriptionStatus).toHaveBeenCalled()
  })

  it('uses fallback meeting title when meeting is null', async () => {
    vi.mocked(meetingRepository.getMeeting).mockResolvedValue(null)
    vi.mocked(pollTranscriptionStatus).mockResolvedValue({
      id: 't1',
      status: 'completed',
      transcript: {
        text: 'Hello',
        segments: [],
        duration_seconds: 10,
        language: 'en',
      },
    })

    renderHook(() => useTranscription('t1', 'm1'))

    await flushPromises()

    expect(notifyTranscriptionComplete).toHaveBeenCalledWith('Meeting', 'm1')
  })

  it('handles poll errors gracefully without stopping', async () => {
    const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    vi.mocked(pollTranscriptionStatus).mockRejectedValue(new Error('Network error'))

    const { result } = renderHook(() => useTranscription('t1', 'm1'))

    await flushPromises()

    expect(consoleSpy).toHaveBeenCalled()

    // Should still be polling after error
    expect(result.current.isPolling).toBe(true)

    consoleSpy.mockRestore()
  })

  it('cleans up polling on unmount', async () => {
    vi.mocked(pollTranscriptionStatus).mockResolvedValue({
      id: 't1',
      status: 'processing',
    })

    const { result, unmount } = renderHook(() => useTranscription('t1', 'm1'))

    await flushPromises()

    expect(result.current.isPolling).toBe(true)

    unmount()

    // Advancing timers should not cause new calls
    vi.mocked(pollTranscriptionStatus).mockClear()
    act(() => {
      vi.advanceTimersByTime(10000)
    })

    expect(pollTranscriptionStatus).not.toHaveBeenCalled()
  })
})
