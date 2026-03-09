import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import React from 'react'
import { RecordingProvider, useRecording } from './RecordingContext'

// ── Mock services ──
vi.mock('@/services/audioRecorder', () => ({
  requestMicrophonePermission: vi.fn(),
  startRecording: vi.fn(),
  stopRecording: vi.fn(),
  pauseRecording: vi.fn(),
  resumeRecording: vi.fn(),
}))

vi.mock('@/services/meetingRepository', () => ({
  meetingRepository: {
    createMeeting: vi.fn(),
    updateMeeting: vi.fn(),
    queueAudioUpload: vi.fn(),
  },
}))

vi.mock('@/services/usageService', () => ({
  trackMeetingCreated: vi.fn(),
  trackEvent: vi.fn(),
}))

import * as audioRecorder from '@/services/audioRecorder'
import { meetingRepository } from '@/services/meetingRepository'
import { trackMeetingCreated, trackEvent } from '@/services/usageService'

const mockMeeting = {
  meeting_id: 'meeting-123',
  title: 'Test Meeting',
  created_at: '2025-01-01T00:00:00Z',
  updated_at: '2025-01-01T00:00:00Z',
  status: 'recording' as const,
  sync_status: 'local_only' as const,
  version: 1,
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <RecordingProvider>{children}</RecordingProvider>
}

describe('RecordingContext', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useFakeTimers()

    vi.mocked(audioRecorder.requestMicrophonePermission).mockResolvedValue(true)
    vi.mocked(audioRecorder.startRecording).mockResolvedValue(undefined)
    vi.mocked(audioRecorder.stopRecording).mockResolvedValue('file:///recording.webm')
    vi.mocked(meetingRepository.createMeeting).mockResolvedValue(mockMeeting)
    vi.mocked(meetingRepository.updateMeeting).mockResolvedValue(undefined)
    vi.mocked(meetingRepository.queueAudioUpload).mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('throws when useRecording is used outside RecordingProvider', () => {
    expect(() => {
      renderHook(() => useRecording())
    }).toThrow('useRecording must be used within RecordingProvider')
  })

  it('has initial state: not recording, not paused, duration 0', () => {
    const { result } = renderHook(() => useRecording(), { wrapper })

    expect(result.current.isRecording).toBe(false)
    expect(result.current.isPaused).toBe(false)
    expect(result.current.duration).toBe(0)
  })

  describe('startRecording', () => {
    it('requests mic permission, creates meeting, starts audio recorder', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      await act(async () => {
        await result.current.startRecording('My Meeting')
      })

      expect(audioRecorder.requestMicrophonePermission).toHaveBeenCalled()
      expect(meetingRepository.createMeeting).toHaveBeenCalledWith('My Meeting')
      expect(trackMeetingCreated).toHaveBeenCalledWith('meeting-123')
      expect(audioRecorder.startRecording).toHaveBeenCalled()
      expect(result.current.isRecording).toBe(true)
      expect(result.current.isPaused).toBe(false)
      expect(result.current.duration).toBe(0)
    })

    it('uses "Untitled Meeting" when no title provided', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      await act(async () => {
        await result.current.startRecording()
      })

      expect(meetingRepository.createMeeting).toHaveBeenCalledWith('Untitled Meeting')
    })

    it('throws error if mic permission denied', async () => {
      vi.mocked(audioRecorder.requestMicrophonePermission).mockResolvedValue(false)

      const { result } = renderHook(() => useRecording(), { wrapper })

      await expect(
        act(async () => {
          await result.current.startRecording('Test')
        }),
      ).rejects.toThrow('Microphone permission denied')

      expect(result.current.isRecording).toBe(false)
      expect(audioRecorder.startRecording).not.toHaveBeenCalled()
    })
  })

  describe('stopRecording', () => {
    it('stops audio, updates meeting, returns meeting ID', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      // Start recording first
      await act(async () => {
        await result.current.startRecording('Test')
      })

      let meetingId: string | null = null
      await act(async () => {
        meetingId = await result.current.stopRecording()
      })

      expect(meetingId).toBe('meeting-123')
      expect(audioRecorder.stopRecording).toHaveBeenCalledWith('meeting-123')
      expect(meetingRepository.updateMeeting).toHaveBeenCalledWith('meeting-123', {
        duration_seconds: 0,
        status: 'pending_upload',
      })
      expect(meetingRepository.queueAudioUpload).toHaveBeenCalledWith(
        'meeting-123',
        'file:///recording.webm',
      )
      expect(trackEvent).toHaveBeenCalledWith('recording_completed', 0, {
        meeting_id: 'meeting-123',
      })
      expect(result.current.isRecording).toBe(false)
      expect(result.current.isPaused).toBe(false)
    })

    it('returns null when no meeting is in progress', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      let meetingId: string | null = 'should-be-null'
      await act(async () => {
        meetingId = await result.current.stopRecording()
      })

      expect(meetingId).toBeNull()
    })

    it('marks meeting as error when stop fails', async () => {
      vi.mocked(audioRecorder.stopRecording).mockRejectedValue(new Error('Stop failed'))

      const { result } = renderHook(() => useRecording(), { wrapper })

      await act(async () => {
        await result.current.startRecording('Test')
      })

      let meetingId: string | null = null
      await act(async () => {
        meetingId = await result.current.stopRecording()
      })

      expect(meetingId).toBe('meeting-123')
      expect(meetingRepository.updateMeeting).toHaveBeenCalledWith('meeting-123', {
        status: 'error',
      })
      expect(result.current.isRecording).toBe(false)
    })
  })

  describe('pauseRecording', () => {
    it('pauses audio and sets isPaused', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      await act(async () => {
        await result.current.startRecording('Test')
      })

      await act(async () => {
        await result.current.pauseRecording()
      })

      expect(audioRecorder.pauseRecording).toHaveBeenCalled()
      expect(result.current.isPaused).toBe(true)
    })
  })

  describe('resumeRecording', () => {
    it('resumes audio and clears isPaused', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      await act(async () => {
        await result.current.startRecording('Test')
      })

      await act(async () => {
        await result.current.pauseRecording()
      })
      expect(result.current.isPaused).toBe(true)

      await act(async () => {
        await result.current.resumeRecording()
      })

      expect(audioRecorder.resumeRecording).toHaveBeenCalled()
      expect(result.current.isPaused).toBe(false)
    })
  })

  describe('duration timer', () => {
    it('increments when recording', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      await act(async () => {
        await result.current.startRecording('Test')
      })

      expect(result.current.duration).toBe(0)

      act(() => {
        vi.advanceTimersByTime(3000)
      })

      expect(result.current.duration).toBe(3)
    })

    it('stops incrementing when paused', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      await act(async () => {
        await result.current.startRecording('Test')
      })

      act(() => {
        vi.advanceTimersByTime(2000)
      })
      expect(result.current.duration).toBe(2)

      await act(async () => {
        await result.current.pauseRecording()
      })

      act(() => {
        vi.advanceTimersByTime(3000)
      })

      // Duration should not have increased
      expect(result.current.duration).toBe(2)
    })

    it('resumes incrementing after unpause', async () => {
      const { result } = renderHook(() => useRecording(), { wrapper })

      await act(async () => {
        await result.current.startRecording('Test')
      })

      act(() => {
        vi.advanceTimersByTime(2000)
      })
      expect(result.current.duration).toBe(2)

      await act(async () => {
        await result.current.pauseRecording()
      })

      await act(async () => {
        await result.current.resumeRecording()
      })

      act(() => {
        vi.advanceTimersByTime(2000)
      })

      expect(result.current.duration).toBe(4)
    })
  })
})
