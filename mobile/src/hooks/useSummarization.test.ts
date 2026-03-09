import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { useSummarization } from './useSummarization'

// ── Mock services ──
vi.mock('@/services/summarizationService', () => ({
  startSummarization: vi.fn(),
  pollSummaryStatus: vi.fn(),
}))

vi.mock('@/services/meetingRepository', () => ({
  meetingRepository: {
    getMeeting: vi.fn(),
    updateMeeting: vi.fn(),
  },
}))

vi.mock('@/services/pushNotifications', () => ({
  notifySummaryComplete: vi.fn(),
}))

import { startSummarization, pollSummaryStatus } from '@/services/summarizationService'
import { meetingRepository } from '@/services/meetingRepository'
import { notifySummaryComplete } from '@/services/pushNotifications'

// Helper to flush all pending microtasks (resolved promises) under fake timers
const flushPromises = () => act(async () => { await vi.advanceTimersByTimeAsync(0) })

describe('useSummarization', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useFakeTimers()
    vi.mocked(meetingRepository.getMeeting).mockResolvedValue({
      meeting_id: 'm1',
      title: 'Test Meeting',
      created_at: '2025-01-01T00:00:00Z',
      updated_at: '2025-01-01T00:00:00Z',
      status: 'summarizing',
      sync_status: 'synced',
      version: 1,
    })
    vi.mocked(meetingRepository.updateMeeting).mockResolvedValue(undefined)
    vi.mocked(startSummarization).mockResolvedValue('m1')
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('has initial state with null status and not polling', () => {
    const { result } = renderHook(() => useSummarization('m1'))

    expect(result.current.status).toBeNull()
    expect(result.current.isPolling).toBe(false)
    expect(result.current.isStarting).toBe(false)
  })

  it('generate() does nothing when meetingId is null', async () => {
    const { result } = renderHook(() => useSummarization(null))

    await act(async () => {
      await result.current.generate()
    })

    expect(startSummarization).not.toHaveBeenCalled()
  })

  it('generate() starts summarization and begins polling', async () => {
    vi.mocked(pollSummaryStatus).mockResolvedValue({
      status: 'processing',
      meeting_id: 'm1',
    })

    const { result } = renderHook(() => useSummarization('m1'))

    await act(async () => {
      await result.current.generate({ provider: 'claude', model: 'opus' })
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(meetingRepository.updateMeeting).toHaveBeenCalledWith('m1', { status: 'summarizing' })
    expect(startSummarization).toHaveBeenCalledWith('m1', {
      provider: 'claude',
      model: 'opus',
    })
    expect(result.current.isPolling).toBe(true)
    expect(result.current.isStarting).toBe(false)

    expect(pollSummaryStatus).toHaveBeenCalledWith('m1')
  })

  it('polls summary status periodically', async () => {
    vi.mocked(pollSummaryStatus).mockResolvedValue({
      status: 'processing',
      meeting_id: 'm1',
    })

    const { result } = renderHook(() => useSummarization('m1'))

    await act(async () => {
      await result.current.generate()
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(pollSummaryStatus).toHaveBeenCalledTimes(1)

    // Advance timer for next poll
    vi.mocked(pollSummaryStatus).mockClear()
    vi.mocked(pollSummaryStatus).mockResolvedValue({
      status: 'processing',
      meeting_id: 'm1',
    })

    await act(async () => {
      vi.advanceTimersByTime(3000)
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(pollSummaryStatus).toHaveBeenCalled()
  })

  it('updates meeting when summary completes', async () => {
    const summaryData = { MeetingName: 'Test', _section_order: ['summary'] }
    vi.mocked(pollSummaryStatus).mockResolvedValue({
      status: 'completed',
      meeting_id: 'm1',
      summary: summaryData,
    })

    const { result } = renderHook(() => useSummarization('m1'))

    await act(async () => {
      await result.current.generate()
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(result.current.status?.status).toBe('completed')
    expect(result.current.isPolling).toBe(false)

    expect(meetingRepository.getMeeting).toHaveBeenCalledWith('m1')
    expect(meetingRepository.updateMeeting).toHaveBeenCalledWith('m1', {
      summary: summaryData,
      status: 'completed',
    })
    expect(notifySummaryComplete).toHaveBeenCalledWith('Test Meeting', 'm1')
  })

  it('uses fallback title when meeting is null', async () => {
    vi.mocked(meetingRepository.getMeeting).mockResolvedValue(null)
    vi.mocked(pollSummaryStatus).mockResolvedValue({
      status: 'completed',
      meeting_id: 'm1',
      summary: { MeetingName: 'Test' },
    })

    const { result } = renderHook(() => useSummarization('m1'))

    await act(async () => {
      await result.current.generate()
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(notifySummaryComplete).toHaveBeenCalledWith('Meeting', 'm1')
  })

  it('stops polling on failure and updates meeting status to completed', async () => {
    vi.mocked(pollSummaryStatus).mockResolvedValue({
      status: 'failed',
      meeting_id: 'm1',
      error: 'LLM error',
    })

    const { result } = renderHook(() => useSummarization('m1'))

    await act(async () => {
      await result.current.generate()
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(result.current.status?.status).toBe('failed')
    expect(result.current.isPolling).toBe(false)

    // On summary failure, meeting status is set to 'completed' (not 'error')
    // generate() first sets status to 'summarizing', then the poll failure sets it to 'completed'
    expect(meetingRepository.updateMeeting).toHaveBeenLastCalledWith('m1', { status: 'completed' })
  })

  it('handles generate() error by reverting meeting status and rethrowing', async () => {
    vi.mocked(startSummarization).mockRejectedValue(new Error('Start failed'))

    const { result } = renderHook(() => useSummarization('m1'))

    let error: Error | undefined
    await act(async () => {
      try {
        await result.current.generate()
      } catch (e) {
        error = e as Error
      }
    })

    expect(error?.message).toBe('Start failed')

    // generate() first calls updateMeeting with 'summarizing', then in the catch with 'completed'
    expect(meetingRepository.updateMeeting).toHaveBeenCalledWith('m1', { status: 'summarizing' })
    expect(meetingRepository.updateMeeting).toHaveBeenLastCalledWith('m1', { status: 'completed' })
    expect(result.current.isStarting).toBe(false)
    expect(result.current.isPolling).toBe(false)
  })

  it('cleans up polling on unmount', async () => {
    vi.mocked(pollSummaryStatus).mockResolvedValue({
      status: 'processing',
      meeting_id: 'm1',
    })

    const { result, unmount } = renderHook(() => useSummarization('m1'))

    await act(async () => {
      await result.current.generate()
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(result.current.isPolling).toBe(true)

    unmount()

    vi.mocked(pollSummaryStatus).mockClear()
    act(() => {
      vi.advanceTimersByTime(10000)
    })

    expect(pollSummaryStatus).not.toHaveBeenCalled()
  })

  it('does not start a second generate while one is in progress', async () => {
    // Make startSummarization resolve slowly - updateMeeting resolves first,
    // then startSummarization hangs
    let resolveStart!: () => void
    vi.mocked(startSummarization).mockImplementation(
      () => new Promise<string>((resolve) => {
        resolveStart = () => resolve('m1')
      }),
    )

    vi.mocked(pollSummaryStatus).mockResolvedValue({
      status: 'processing',
      meeting_id: 'm1',
    })

    const { result } = renderHook(() => useSummarization('m1'))

    // Start first generate (will hang at startSummarization)
    // Don't await it — it won't resolve until we call resolveStart
    const firstGenerate = result.current.generate()

    // Flush microtasks so updateMeeting resolves and setIsStarting(true) is called
    // Then re-render so the hook picks up isStarting=true
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(result.current.isStarting).toBe(true)

    // Try second generate with the updated hook reference (isStarting=true)
    await act(async () => {
      await result.current.generate()
    })

    // Only one call to startSummarization
    expect(startSummarization).toHaveBeenCalledTimes(1)

    // Resolve the first one to clean up
    await act(async () => {
      resolveStart()
      await vi.advanceTimersByTimeAsync(0)
    })
    await act(async () => { await firstGenerate })
  })
})
