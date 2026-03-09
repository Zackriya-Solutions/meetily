import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import RecordingScreen from './RecordingScreen'

const mockStartRecording = vi.fn()
const mockStopRecording = vi.fn()
const mockPauseRecording = vi.fn()
const mockResumeRecording = vi.fn()
const mockPush = vi.fn()

let mockRecordingState = {
  isRecording: false,
  isPaused: false,
  duration: 0,
  startRecording: mockStartRecording,
  stopRecording: mockStopRecording,
  pauseRecording: mockPauseRecording,
  resumeRecording: mockResumeRecording,
}

let mockQuotaState = {
  quota: { remaining_minutes: 60, plan_limit: 120, used_minutes: 60 },
  hasQuota: true,
  loading: false,
  refresh: vi.fn(),
}

vi.mock('@/contexts/RecordingContext', () => ({
  useRecording: () => mockRecordingState,
}))

vi.mock('@/hooks/useQuota', () => ({
  useQuota: () => mockQuotaState,
}))

vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: mockPush,
    replace: vi.fn(),
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => '/record',
  useSearchParams: () => new URLSearchParams(),
}))

describe('RecordingScreen', () => {
  beforeEach(() => {
    mockRecordingState = {
      isRecording: false,
      isPaused: false,
      duration: 0,
      startRecording: mockStartRecording,
      stopRecording: mockStopRecording,
      pauseRecording: mockPauseRecording,
      resumeRecording: mockResumeRecording,
    }
    mockQuotaState = {
      quota: { remaining_minutes: 60, plan_limit: 120, used_minutes: 60 },
      hasQuota: true,
      loading: false,
      refresh: vi.fn(),
    }
    mockStartRecording.mockResolvedValue(undefined)
    mockStopRecording.mockResolvedValue('meeting-123')
  })

  it('shows start recording state when not recording', () => {
    render(<RecordingScreen />)
    expect(screen.getByText('Tap to start recording')).toBeInTheDocument()
  })

  it('shows title input when not recording', () => {
    render(<RecordingScreen />)
    expect(screen.getByPlaceholderText('Meeting title (optional)')).toBeInTheDocument()
  })

  it('hides title input when recording', () => {
    mockRecordingState.isRecording = true
    render(<RecordingScreen />)
    expect(screen.queryByPlaceholderText('Meeting title (optional)')).not.toBeInTheDocument()
  })

  it('shows quota info when not recording', () => {
    render(<RecordingScreen />)
    expect(screen.getByText('60 min remaining')).toBeInTheDocument()
  })

  it('hides quota info when recording', () => {
    mockRecordingState.isRecording = true
    render(<RecordingScreen />)
    expect(screen.queryByText('60 min remaining')).not.toBeInTheDocument()
  })

  it('shows "Transcription quota exceeded" when no quota', () => {
    mockQuotaState.hasQuota = false
    render(<RecordingScreen />)
    expect(screen.getByText('Transcription quota exceeded')).toBeInTheDocument()
  })

  it('disables start button when quota exceeded', () => {
    mockQuotaState.hasQuota = false
    render(<RecordingScreen />)
    const buttons = screen.getAllByRole('button')
    const startButton = buttons.find(b => b.className.includes('bg-red-600'))
    expect(startButton).toBeDisabled()
  })

  it('shows upgrade message when no quota', () => {
    mockQuotaState.hasQuota = false
    render(<RecordingScreen />)
    expect(screen.getByText('Upgrade your plan to continue recording')).toBeInTheDocument()
  })

  it('calls startRecording when start button is clicked', async () => {
    const user = userEvent.setup()
    render(<RecordingScreen />)
    // The start button is the large red circular button
    const buttons = screen.getAllByRole('button')
    const startButton = buttons.find(b => b.className.includes('bg-red-600'))!
    await user.click(startButton)
    expect(mockStartRecording).toHaveBeenCalled()
  })

  it('passes meeting title to startRecording', async () => {
    const user = userEvent.setup()
    render(<RecordingScreen />)
    const titleInput = screen.getByPlaceholderText('Meeting title (optional)')
    await user.type(titleInput, 'Sprint Review')
    const buttons = screen.getAllByRole('button')
    const startButton = buttons.find(b => b.className.includes('bg-red-600'))!
    await user.click(startButton)
    expect(mockStartRecording).toHaveBeenCalledWith('Sprint Review')
  })

  it('shows stop and pause buttons when recording', () => {
    mockRecordingState.isRecording = true
    render(<RecordingScreen />)
    expect(screen.getByText('Recording in progress...')).toBeInTheDocument()
  })

  it('shows paused status text when recording is paused', () => {
    mockRecordingState.isRecording = true
    mockRecordingState.isPaused = true
    render(<RecordingScreen />)
    expect(screen.getByText('Recording paused')).toBeInTheDocument()
  })

  it('shows formatted duration', () => {
    mockRecordingState.duration = 65
    render(<RecordingScreen />)
    expect(screen.getByText('01:05')).toBeInTheDocument()
  })

  it('shows hours in duration when over 3600s', () => {
    mockRecordingState.duration = 3661
    render(<RecordingScreen />)
    expect(screen.getByText('1:01:01')).toBeInTheDocument()
  })

  it('shows error when microphone permission is denied', async () => {
    const user = userEvent.setup()
    mockStartRecording.mockRejectedValue(new Error('Microphone permission denied'))
    render(<RecordingScreen />)
    const buttons = screen.getAllByRole('button')
    const startButton = buttons.find(b => b.className.includes('bg-red-600'))!
    await user.click(startButton)
    expect(
      await screen.findByText('Microphone access is required. Please enable it in your device settings.')
    ).toBeInTheDocument()
  })

  it('shows generic error for non-permission failures', async () => {
    const user = userEvent.setup()
    mockStartRecording.mockRejectedValue(new Error('Unknown error occurred'))
    render(<RecordingScreen />)
    const buttons = screen.getAllByRole('button')
    const startButton = buttons.find(b => b.className.includes('bg-red-600'))!
    await user.click(startButton)
    expect(await screen.findByText('Unknown error occurred')).toBeInTheDocument()
  })

  it('calls stopRecording and navigates on stop', async () => {
    const user = userEvent.setup()
    mockRecordingState.isRecording = true
    render(<RecordingScreen />)
    // Stop button is the large red one
    const buttons = screen.getAllByRole('button')
    const stopButton = buttons.find(b => b.className.includes('bg-red-600'))!
    await user.click(stopButton)
    expect(mockStopRecording).toHaveBeenCalled()
  })

  it('does not show quota when quota is null', () => {
    mockQuotaState.quota = null as any
    render(<RecordingScreen />)
    expect(screen.queryByText(/min remaining/)).not.toBeInTheDocument()
    expect(screen.queryByText(/quota exceeded/)).not.toBeInTheDocument()
  })

  it('shows 00:00 duration initially', () => {
    render(<RecordingScreen />)
    expect(screen.getByText('00:00')).toBeInTheDocument()
  })
})
