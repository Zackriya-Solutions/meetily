'use client'

import { useState, useEffect, useCallback, useRef } from 'react'
import { TranscriptionStatus } from '@/types'
import { pollTranscriptionStatus } from '@/services/transcriptionService'
import { meetingRepository } from '@/services/meetingRepository'
import { notifyTranscriptionComplete } from '@/services/pushNotifications'

const POLL_INTERVAL_MS = 3000

/**
 * Hook that polls transcription status and updates the local meeting
 * when transcription completes.
 */
export function useTranscription(transcriptionId: string | null, meetingId: string | null) {
  const [status, setStatus] = useState<TranscriptionStatus | null>(null)
  const [isPolling, setIsPolling] = useState(false)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const stopPolling = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current)
      intervalRef.current = null
    }
    setIsPolling(false)
  }, [])

  useEffect(() => {
    if (!transcriptionId) return

    setIsPolling(true)

    const poll = async () => {
      try {
        const result = await pollTranscriptionStatus(transcriptionId)
        setStatus(result)

        if (result.status === 'completed') {
          stopPolling()
          // Update local meeting with transcript
          if (meetingId && result.transcript) {
            const meeting = await meetingRepository.getMeeting(meetingId)
            await meetingRepository.updateMeeting(meetingId, {
              transcript_text: result.transcript.text,
              transcript_segments: result.transcript.segments,
              duration_seconds: result.transcript.duration_seconds,
              status: 'completed',
            })
            notifyTranscriptionComplete(meeting?.title || 'Meeting', meetingId)
          }
        } else if (result.status === 'failed') {
          stopPolling()
          if (meetingId) {
            await meetingRepository.updateMeeting(meetingId, { status: 'error' })
          }
        }
      } catch (e) {
        console.warn('[useTranscription] Poll error:', e)
      }
    }

    // Initial poll
    poll()
    intervalRef.current = setInterval(poll, POLL_INTERVAL_MS)

    return () => stopPolling()
  }, [transcriptionId, meetingId, stopPolling])

  return { status, isPolling }
}
