import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import React from 'react'
import { SyncProvider, useSync } from './SyncContext'

// ── Mock dependencies ──
const mockStart = vi.fn()
const mockStop = vi.fn()
const mockSync = vi.fn().mockResolvedValue(undefined)

vi.mock('@/services/syncService', () => ({
  syncService: {
    start: (...args: any[]) => mockStart(...args),
    stop: (...args: any[]) => mockStop(...args),
    sync: (...args: any[]) => mockSync(...args),
  },
}))

const mockGetPendingCount = vi.fn().mockResolvedValue(0)
const mockGetSyncState = vi.fn().mockResolvedValue(null)

vi.mock('@/services/database', () => ({
  initializeDatabase: vi.fn().mockResolvedValue(undefined),
  getDatabase: vi.fn(() => ({
    getPendingCount: mockGetPendingCount,
    getSyncState: mockGetSyncState,
  })),
}))

let mockIsAuthenticated = false
vi.mock('./AuthContext', () => ({
  useAuth: () => ({ isAuthenticated: mockIsAuthenticated }),
}))

// Helper to flush all pending microtasks (resolved promises) under fake timers
const flushPromises = () => act(async () => { await vi.advanceTimersByTimeAsync(0) })

function wrapper({ children }: { children: React.ReactNode }) {
  return <SyncProvider>{children}</SyncProvider>
}

describe('SyncContext', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useFakeTimers()
    mockIsAuthenticated = false
    mockGetPendingCount.mockResolvedValue(0)
    mockGetSyncState.mockResolvedValue(null)
    // Ensure navigator.onLine is true
    Object.defineProperty(navigator, 'onLine', { value: true, writable: true })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('throws when useSync is used outside SyncProvider', () => {
    // Suppress React error boundary console output
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    expect(() => {
      renderHook(() => useSync())
    }).toThrow('useSync must be used within SyncProvider')
    consoleSpy.mockRestore()
  })

  it('has initial state: online, not syncing, 0 pending', async () => {
    const { result } = renderHook(() => useSync(), { wrapper })

    await flushPromises()

    expect(result.current.isOnline).toBe(true)
    expect(result.current.isSyncing).toBe(false)
    expect(result.current.pendingCount).toBe(0)
    expect(result.current.lastSyncedAt).toBeNull()
  })

  it('starts sync service when authenticated', async () => {
    mockIsAuthenticated = true

    renderHook(() => useSync(), { wrapper })

    await flushPromises()

    expect(mockStart).toHaveBeenCalled()
  })

  it('stops sync service when not authenticated', async () => {
    mockIsAuthenticated = false

    renderHook(() => useSync(), { wrapper })

    await flushPromises()

    expect(mockStop).toHaveBeenCalled()
  })

  it('forceSync triggers sync and updates counts', async () => {
    mockIsAuthenticated = true
    mockGetPendingCount.mockResolvedValue(3)
    mockGetSyncState.mockResolvedValue('2025-01-01T00:00:00Z')

    const { result } = renderHook(() => useSync(), { wrapper })

    await flushPromises()

    expect(result.current.isOnline).toBe(true)

    // Reset mock to return updated values after sync
    mockGetPendingCount.mockResolvedValue(1)
    mockGetSyncState.mockResolvedValue('2025-06-01T12:00:00Z')

    await act(async () => {
      await result.current.forceSync()
    })

    expect(mockSync).toHaveBeenCalled()
    expect(result.current.pendingCount).toBe(1)
    expect(result.current.lastSyncedAt).toBe('2025-06-01T12:00:00Z')
    expect(result.current.isSyncing).toBe(false)
  })

  it('forceSync does nothing when offline', async () => {
    Object.defineProperty(navigator, 'onLine', { value: false, writable: true })

    const { result } = renderHook(() => useSync(), { wrapper })

    await flushPromises()

    expect(result.current.isOnline).toBe(false)

    await act(async () => {
      await result.current.forceSync()
    })

    expect(mockSync).not.toHaveBeenCalled()
  })

  it('network status listeners update isOnline', async () => {
    const { result } = renderHook(() => useSync(), { wrapper })

    await flushPromises()

    expect(result.current.isOnline).toBe(true)

    // Simulate going offline
    act(() => {
      window.dispatchEvent(new Event('offline'))
    })

    expect(result.current.isOnline).toBe(false)

    // Simulate coming back online
    act(() => {
      window.dispatchEvent(new Event('online'))
    })

    expect(result.current.isOnline).toBe(true)
  })

  it('polls pending count periodically', async () => {
    mockGetPendingCount.mockResolvedValue(5)
    mockGetSyncState.mockResolvedValue('2025-03-01T00:00:00Z')

    const { result } = renderHook(() => useSync(), { wrapper })

    // Wait for initial poll to resolve
    await flushPromises()

    expect(result.current.pendingCount).toBe(5)
    expect(result.current.lastSyncedAt).toBe('2025-03-01T00:00:00Z')

    // Update mock and advance timer for next poll
    mockGetPendingCount.mockResolvedValue(2)

    await act(async () => {
      vi.advanceTimersByTime(5000)
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(result.current.pendingCount).toBe(2)
  })
})
