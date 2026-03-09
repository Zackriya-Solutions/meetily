import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import MeetingCard from './MeetingCard'
import { Meeting } from '@/types'

function makeMeeting(overrides: Partial<Meeting> = {}): Meeting {
  return {
    meeting_id: 'test-123',
    title: 'Team Standup',
    created_at: '2025-06-15T10:30:00Z',
    updated_at: '2025-06-15T10:30:00Z',
    status: 'completed',
    sync_status: 'synced',
    version: 1,
    ...overrides,
  }
}

describe('MeetingCard', () => {
  it('renders meeting title', () => {
    render(<MeetingCard meeting={makeMeeting()} />)
    expect(screen.getByText('Team Standup')).toBeInTheDocument()
  })

  it('renders "Untitled Meeting" when title is empty', () => {
    render(<MeetingCard meeting={makeMeeting({ title: '' })} />)
    expect(screen.getByText('Untitled Meeting')).toBeInTheDocument()
  })

  it('renders formatted date', () => {
    render(<MeetingCard meeting={makeMeeting()} />)
    // date-fns format: "MMM d, yyyy h:mm a"
    expect(screen.getByText(/Jun 15, 2025/)).toBeInTheDocument()
  })

  it('shows Completed status for completed meetings', () => {
    render(<MeetingCard meeting={makeMeeting({ status: 'completed' })} />)
    expect(screen.getByText('Completed')).toBeInTheDocument()
  })

  it('shows Recording status for recording meetings', () => {
    render(<MeetingCard meeting={makeMeeting({ status: 'recording' })} />)
    expect(screen.getByText('Recording')).toBeInTheDocument()
  })

  it('shows Transcribing status', () => {
    render(<MeetingCard meeting={makeMeeting({ status: 'transcribing' })} />)
    expect(screen.getByText('Transcribing')).toBeInTheDocument()
  })

  it('shows Pending upload status', () => {
    render(<MeetingCard meeting={makeMeeting({ status: 'pending_upload' })} />)
    expect(screen.getByText('Pending upload')).toBeInTheDocument()
  })

  it('shows Error status', () => {
    render(<MeetingCard meeting={makeMeeting({ status: 'error' })} />)
    expect(screen.getByText('Error')).toBeInTheDocument()
  })

  it('shows duration when available', () => {
    render(<MeetingCard meeting={makeMeeting({ duration_seconds: 125 })} />)
    expect(screen.getByText('2m 5s')).toBeInTheDocument()
  })

  it('does not show duration when not available', () => {
    render(<MeetingCard meeting={makeMeeting({ duration_seconds: undefined })} />)
    expect(screen.queryByText(/\d+m/)).not.toBeInTheDocument()
  })

  it('shows "Not synced" indicator for local_only sync status', () => {
    render(<MeetingCard meeting={makeMeeting({ sync_status: 'local_only' })} />)
    expect(screen.getByText('Not synced')).toBeInTheDocument()
  })

  it('does not show "Not synced" for synced meetings', () => {
    render(<MeetingCard meeting={makeMeeting({ sync_status: 'synced' })} />)
    expect(screen.queryByText('Not synced')).not.toBeInTheDocument()
  })

  it('applies animate-spin class for recording status', () => {
    const { container } = render(<MeetingCard meeting={makeMeeting({ status: 'recording' })} />)
    const spinIcon = container.querySelector('.animate-spin')
    expect(spinIcon).toBeInTheDocument()
  })

  it('does not apply animate-spin for completed status', () => {
    const { container } = render(<MeetingCard meeting={makeMeeting({ status: 'completed' })} />)
    const spinIcon = container.querySelector('.animate-spin')
    expect(spinIcon).not.toBeInTheDocument()
  })

  it('shows duration with 0 seconds', () => {
    render(<MeetingCard meeting={makeMeeting({ duration_seconds: 60 })} />)
    expect(screen.getByText('1m 0s')).toBeInTheDocument()
  })
})
