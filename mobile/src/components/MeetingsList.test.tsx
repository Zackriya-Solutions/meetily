import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import MeetingsList from './MeetingsList'
import { Meeting } from '@/types'

const mockForceSync = vi.fn().mockResolvedValue(undefined)
const mockGetMeetings = vi.fn()

vi.mock('@/contexts/SyncContext', () => ({
  useSync: () => ({
    isOnline: true,
    isSyncing: false,
    pendingCount: 0,
    lastSyncedAt: null,
    forceSync: mockForceSync,
  }),
}))

vi.mock('@/services/meetingRepository', () => ({
  meetingRepository: {
    getMeetings: () => mockGetMeetings(),
  },
}))

function makeMeeting(overrides: Partial<Meeting> = {}): Meeting {
  return {
    meeting_id: 'meeting-1',
    title: 'Test Meeting',
    created_at: '2025-06-15T10:00:00Z',
    updated_at: '2025-06-15T10:00:00Z',
    status: 'completed',
    sync_status: 'synced',
    version: 1,
    ...overrides,
  }
}

describe('MeetingsList', () => {
  beforeEach(() => {
    mockGetMeetings.mockResolvedValue([])
    mockForceSync.mockResolvedValue(undefined)
  })

  it('shows loading spinner initially', () => {
    mockGetMeetings.mockReturnValue(new Promise(() => {})) // never resolves
    const { container } = render(<MeetingsList />)
    expect(container.querySelector('.animate-spin')).toBeInTheDocument()
  })

  it('shows empty state when no meetings', async () => {
    mockGetMeetings.mockResolvedValue([])
    render(<MeetingsList />)
    await waitFor(() => {
      expect(screen.getByText('No meetings yet')).toBeInTheDocument()
    })
    expect(screen.getByText('Tap the Record tab to start your first meeting.')).toBeInTheDocument()
  })

  it('renders meeting cards when meetings exist', async () => {
    mockGetMeetings.mockResolvedValue([
      makeMeeting({ meeting_id: 'm1', title: 'Sprint Planning' }),
      makeMeeting({ meeting_id: 'm2', title: 'Retro' }),
    ])
    render(<MeetingsList />)
    await waitFor(() => {
      expect(screen.getByText('Sprint Planning')).toBeInTheDocument()
    })
    expect(screen.getByText('Retro')).toBeInTheDocument()
  })

  it('renders meetings as links to detail pages', async () => {
    mockGetMeetings.mockResolvedValue([
      makeMeeting({ meeting_id: 'abc-123', title: 'My Meeting' }),
    ])
    render(<MeetingsList />)
    await waitFor(() => {
      expect(screen.getByText('My Meeting')).toBeInTheDocument()
    })
    const link = screen.getByText('My Meeting').closest('a')
    expect(link).toHaveAttribute('href', '/meeting/abc-123')
  })

  it('shows Meetings heading', async () => {
    mockGetMeetings.mockResolvedValue([])
    render(<MeetingsList />)
    await waitFor(() => {
      expect(screen.getByText('Meetings')).toBeInTheDocument()
    })
  })

  it('shows Refresh button', async () => {
    mockGetMeetings.mockResolvedValue([])
    render(<MeetingsList />)
    await waitFor(() => {
      expect(screen.getByText('Refresh')).toBeInTheDocument()
    })
  })

  it('calls forceSync and reloads meetings on Refresh click', async () => {
    const user = userEvent.setup()
    mockGetMeetings.mockResolvedValue([])
    render(<MeetingsList />)

    await waitFor(() => {
      expect(screen.getByText('Refresh')).toBeInTheDocument()
    })

    await user.click(screen.getByText('Refresh'))

    expect(mockForceSync).toHaveBeenCalled()
  })

  it('handles getMeetings failure gracefully', async () => {
    mockGetMeetings.mockRejectedValue(new Error('DB error'))
    render(<MeetingsList />)
    await waitFor(() => {
      expect(screen.getByText('No meetings yet')).toBeInTheDocument()
    })
  })
})
