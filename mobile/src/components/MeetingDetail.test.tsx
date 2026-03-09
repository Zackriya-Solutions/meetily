import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import MeetingDetail from './MeetingDetail'
import { Meeting } from '@/types'

const mockBack = vi.fn()
const mockGetMeeting = vi.fn()
const mockGenerate = vi.fn().mockResolvedValue(undefined)

vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
    back: mockBack,
    prefetch: vi.fn(),
  }),
  usePathname: () => '/meeting/test-123',
  useSearchParams: () => new URLSearchParams(),
}))

vi.mock('@/services/meetingRepository', () => ({
  meetingRepository: {
    getMeeting: (...args: any[]) => mockGetMeeting(...args),
  },
}))

vi.mock('@/hooks/useSummarization', () => ({
  useSummarization: () => ({
    generate: mockGenerate,
    isPolling: false,
    isStarting: false,
    status: null,
  }),
}))

vi.mock('@/hooks/useTranscription', () => ({
  useTranscription: () => ({
    status: null,
    isPolling: false,
  }),
}))

function makeMeeting(overrides: Partial<Meeting> = {}): Meeting {
  return {
    meeting_id: 'test-123',
    title: 'Sprint Review',
    created_at: '2025-06-15T10:00:00Z',
    updated_at: '2025-06-15T10:00:00Z',
    status: 'completed',
    sync_status: 'synced',
    version: 1,
    transcript_text: 'Hello everyone, welcome to the sprint review.',
    ...overrides,
  }
}

describe('MeetingDetail', () => {
  beforeEach(() => {
    mockGetMeeting.mockResolvedValue(makeMeeting())
    mockGenerate.mockResolvedValue(undefined)
  })

  it('shows loading spinner initially', () => {
    mockGetMeeting.mockReturnValue(new Promise(() => {})) // never resolves
    const { container } = render(<MeetingDetail meetingId="test-123" />)
    expect(container.querySelector('.animate-spin')).toBeInTheDocument()
  })

  it('renders meeting title after loading', async () => {
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Sprint Review')).toBeInTheDocument()
    })
  })

  it('shows "Untitled Meeting" when title is empty', async () => {
    mockGetMeeting.mockResolvedValue(makeMeeting({ title: '' }))
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Untitled Meeting')).toBeInTheDocument()
    })
  })

  it('shows "Meeting not found" when meeting is null', async () => {
    mockGetMeeting.mockResolvedValue(null)
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Meeting not found')).toBeInTheDocument()
    })
  })

  it('shows Go back button when meeting not found', async () => {
    mockGetMeeting.mockResolvedValue(null)
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Go back')).toBeInTheDocument()
    })
  })

  it('calls router.back when Go back is clicked', async () => {
    const user = userEvent.setup()
    mockGetMeeting.mockResolvedValue(null)
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Go back')).toBeInTheDocument()
    })
    await user.click(screen.getByText('Go back'))
    expect(mockBack).toHaveBeenCalled()
  })

  it('shows Transcript tab by default', async () => {
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Transcript')).toBeInTheDocument()
    })
    const transcriptTab = screen.getByText('Transcript').closest('button')
    expect(transcriptTab?.className).toContain('border-blue-600')
  })

  it('shows Summary tab', async () => {
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Summary')).toBeInTheDocument()
    })
  })

  it('renders transcript content in transcript tab', async () => {
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Hello everyone, welcome to the sprint review.')).toBeInTheDocument()
    })
  })

  it('switches to summary tab on click', async () => {
    const user = userEvent.setup()
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Transcript')).toBeInTheDocument()
    })
    await user.click(screen.getByText('Summary'))
    const summaryTab = screen.getByText('Summary').closest('button')
    expect(summaryTab?.className).toContain('border-blue-600')
    // Transcript tab should no longer be active
    const transcriptTab = screen.getByText('Transcript').closest('button')
    expect(transcriptTab?.className).toContain('border-transparent')
  })

  it('switches back to transcript tab on click', async () => {
    const user = userEvent.setup()
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Summary')).toBeInTheDocument()
    })
    await user.click(screen.getByText('Summary'))
    await user.click(screen.getByText('Transcript'))
    const transcriptTab = screen.getByText('Transcript').closest('button')
    expect(transcriptTab?.className).toContain('border-blue-600')
  })

  it('shows status indicator for pending_upload', async () => {
    mockGetMeeting.mockResolvedValue(makeMeeting({ status: 'pending_upload' }))
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Waiting to upload audio...')).toBeInTheDocument()
    })
  })

  it('shows status indicator for uploading', async () => {
    mockGetMeeting.mockResolvedValue(makeMeeting({ status: 'uploading' }))
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Uploading audio...')).toBeInTheDocument()
    })
  })

  it('shows status indicator for transcribing', async () => {
    mockGetMeeting.mockResolvedValue(makeMeeting({ status: 'transcribing' }))
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Transcription in progress...')).toBeInTheDocument()
    })
  })

  it('shows status indicator for summarizing', async () => {
    mockGetMeeting.mockResolvedValue(makeMeeting({ status: 'summarizing' }))
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Generating summary...')).toBeInTheDocument()
    })
  })

  it('does not show status indicator for completed', async () => {
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Sprint Review')).toBeInTheDocument()
    })
    expect(screen.queryByText('Waiting to upload audio...')).not.toBeInTheDocument()
    expect(screen.queryByText('Uploading audio...')).not.toBeInTheDocument()
    expect(screen.queryByText('Transcription in progress...')).not.toBeInTheDocument()
  })

  it('calls getMeeting with the provided meetingId', async () => {
    render(<MeetingDetail meetingId="abc-456" />)
    await waitFor(() => {
      expect(mockGetMeeting).toHaveBeenCalledWith('abc-456')
    })
  })

  it('handles getMeeting failure gracefully', async () => {
    mockGetMeeting.mockRejectedValue(new Error('DB error'))
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Meeting not found')).toBeInTheDocument()
    })
  })

  it('shows summary view content when on summary tab with summary data', async () => {
    const user = userEvent.setup()
    mockGetMeeting.mockResolvedValue(
      makeMeeting({
        status: 'completed',
        summary: {
          _section_order: ['overview'],
          overview: {
            title: 'Overview',
            blocks: [{ id: 'b1', type: 'text', content: 'Summary content here.', color: '' }],
          },
        },
      })
    )
    render(<MeetingDetail meetingId="test-123" />)
    await waitFor(() => {
      expect(screen.getByText('Summary')).toBeInTheDocument()
    })
    await user.click(screen.getByText('Summary'))
    expect(screen.getByText('Overview')).toBeInTheDocument()
    expect(screen.getByText('Summary content here.')).toBeInTheDocument()
  })
})
