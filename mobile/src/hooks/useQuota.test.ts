import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { useQuota } from './useQuota'

// ── Mock services ──
vi.mock('@/services/transcriptionService', () => ({
  getTranscriptionQuota: vi.fn(),
}))

let mockIsAuthenticated = false
vi.mock('@/contexts/AuthContext', () => ({
  useAuth: () => ({ isAuthenticated: mockIsAuthenticated }),
}))

import { getTranscriptionQuota } from '@/services/transcriptionService'

describe('useQuota', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockIsAuthenticated = false
  })

  it('does not fetch when not authenticated', async () => {
    mockIsAuthenticated = false

    const { result } = renderHook(() => useQuota())

    // Give effects time to run
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    expect(getTranscriptionQuota).not.toHaveBeenCalled()
    expect(result.current.quota).toBeNull()
  })

  it('fetches quota when authenticated', async () => {
    mockIsAuthenticated = true
    vi.mocked(getTranscriptionQuota).mockResolvedValue({
      remaining_minutes: 45,
      plan_limit: 60,
      used_minutes: 15,
    })

    const { result } = renderHook(() => useQuota())

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
      expect(result.current.quota).toEqual({
        remaining_minutes: 45,
        plan_limit: 60,
        used_minutes: 15,
      })
    })

    expect(getTranscriptionQuota).toHaveBeenCalled()
  })

  it('returns hasQuota=true when remaining_minutes > 0', async () => {
    mockIsAuthenticated = true
    vi.mocked(getTranscriptionQuota).mockResolvedValue({
      remaining_minutes: 10,
      plan_limit: 60,
      used_minutes: 50,
    })

    const { result } = renderHook(() => useQuota())

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    expect(result.current.hasQuota).toBe(true)
  })

  it('returns hasQuota=false when remaining_minutes is 0', async () => {
    mockIsAuthenticated = true
    vi.mocked(getTranscriptionQuota).mockResolvedValue({
      remaining_minutes: 0,
      plan_limit: 60,
      used_minutes: 60,
    })

    const { result } = renderHook(() => useQuota())

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    expect(result.current.hasQuota).toBe(false)
  })

  it('returns hasQuota=true when quota is null (unknown — default allow)', async () => {
    mockIsAuthenticated = false

    const { result } = renderHook(() => useQuota())

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    // quota is null, so hasQuota defaults to true
    expect(result.current.quota).toBeNull()
    expect(result.current.hasQuota).toBe(true)
  })

  it('returns hasQuota=true when fetch fails (quota stays null)', async () => {
    mockIsAuthenticated = true
    vi.mocked(getTranscriptionQuota).mockRejectedValue(new Error('Network error'))

    const { result } = renderHook(() => useQuota())

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    // Quota failed to load — should default to allowing
    expect(result.current.quota).toBeNull()
    expect(result.current.hasQuota).toBe(true)
  })

  it('refresh() re-fetches quota', async () => {
    mockIsAuthenticated = true
    vi.mocked(getTranscriptionQuota).mockResolvedValue({
      remaining_minutes: 45,
      plan_limit: 60,
      used_minutes: 15,
    })

    const { result } = renderHook(() => useQuota())

    await waitFor(() => {
      expect(result.current.quota).not.toBeNull()
    })

    // Update mock for refresh
    vi.mocked(getTranscriptionQuota).mockResolvedValue({
      remaining_minutes: 30,
      plan_limit: 60,
      used_minutes: 30,
    })

    await act(async () => {
      await result.current.refresh()
    })

    expect(result.current.quota?.remaining_minutes).toBe(30)
    expect(result.current.quota?.used_minutes).toBe(30)
    expect(getTranscriptionQuota).toHaveBeenCalledTimes(2)
  })

  it('sets loading state during fetch', async () => {
    mockIsAuthenticated = true
    let resolveQuota: (value: any) => void
    vi.mocked(getTranscriptionQuota).mockImplementation(
      () => new Promise((resolve) => {
        resolveQuota = resolve
      }),
    )

    const { result } = renderHook(() => useQuota())

    await waitFor(() => {
      expect(result.current.loading).toBe(true)
    })

    await act(async () => {
      resolveQuota!({
        remaining_minutes: 60,
        plan_limit: 60,
        used_minutes: 0,
      })
    })

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })
  })
})
