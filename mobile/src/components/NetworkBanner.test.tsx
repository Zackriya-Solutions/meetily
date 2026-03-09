import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import NetworkBanner from './NetworkBanner'

const mockUseSync = vi.fn()

vi.mock('@/contexts/SyncContext', () => ({
  useSync: () => mockUseSync(),
}))

describe('NetworkBanner', () => {
  beforeEach(() => {
    mockUseSync.mockReturnValue({
      isOnline: true,
      pendingCount: 0,
      isSyncing: false,
      lastSyncedAt: null,
      forceSync: vi.fn(),
    })
  })

  it('renders nothing when online and no pending items', () => {
    const { container } = render(<NetworkBanner />)
    expect(container.innerHTML).toBe('')
  })

  it('shows offline banner when not online', () => {
    mockUseSync.mockReturnValue({
      isOnline: false,
      pendingCount: 0,
      isSyncing: false,
      lastSyncedAt: null,
      forceSync: vi.fn(),
    })
    render(<NetworkBanner />)
    expect(screen.getByText("You're offline. Changes will sync when you reconnect.")).toBeInTheDocument()
  })

  it('applies red styling when offline', () => {
    mockUseSync.mockReturnValue({
      isOnline: false,
      pendingCount: 0,
      isSyncing: false,
      lastSyncedAt: null,
      forceSync: vi.fn(),
    })
    render(<NetworkBanner />)
    const banner = screen.getByText("You're offline. Changes will sync when you reconnect.").closest('div')
    expect(banner?.className).toContain('bg-red-50')
    expect(banner?.className).toContain('text-red-700')
  })

  it('shows pending count when online with pending items', () => {
    mockUseSync.mockReturnValue({
      isOnline: true,
      pendingCount: 3,
      isSyncing: false,
      lastSyncedAt: null,
      forceSync: vi.fn(),
    })
    render(<NetworkBanner />)
    expect(screen.getByText('Syncing 3 pending items...')).toBeInTheDocument()
  })

  it('uses singular "item" when pendingCount is 1', () => {
    mockUseSync.mockReturnValue({
      isOnline: true,
      pendingCount: 1,
      isSyncing: false,
      lastSyncedAt: null,
      forceSync: vi.fn(),
    })
    render(<NetworkBanner />)
    expect(screen.getByText('Syncing 1 pending item...')).toBeInTheDocument()
  })

  it('applies yellow styling when online with pending items', () => {
    mockUseSync.mockReturnValue({
      isOnline: true,
      pendingCount: 2,
      isSyncing: false,
      lastSyncedAt: null,
      forceSync: vi.fn(),
    })
    render(<NetworkBanner />)
    const banner = screen.getByText('Syncing 2 pending items...').closest('div')
    expect(banner?.className).toContain('bg-yellow-50')
    expect(banner?.className).toContain('text-yellow-700')
  })

  it('does not show offline message when online', () => {
    mockUseSync.mockReturnValue({
      isOnline: true,
      pendingCount: 5,
      isSyncing: false,
      lastSyncedAt: null,
      forceSync: vi.fn(),
    })
    render(<NetworkBanner />)
    expect(screen.queryByText(/offline/i)).not.toBeInTheDocument()
  })
})
