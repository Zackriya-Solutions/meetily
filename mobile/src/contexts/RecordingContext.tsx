'use client'

import React, { createContext, useContext, useState, useCallback, useRef, useEffect } from 'react'
import * as audioRecorder from '@/services/audioRecorder'
import { meetingRepository } from '@/services/meetingRepository'
import { trackMeetingCreated, trackEvent } from '@/services/usageService'

interface RecordingContextValue {
  isRecording: boolean
  isPaused: boolean
  duration: number
  startRecording: (title?: string) => Promise<void>
  stopRecording: () => Promise<string | null> // returns meeting_id
  pauseRecording: () => Promise<void>
  resumeRecording: () => Promise<void>
}

const RecordingContext = createContext<RecordingContextValue | null>(null)

export function useRecording(): RecordingContextValue {
  const ctx = useContext(RecordingContext)
  if (!ctx) throw new Error('useRecording must be used within RecordingProvider')
  return ctx
}

export function RecordingProvider({ children }: { children: React.ReactNode }) {
  const [isRecording, setIsRecording] = useState(false)
  const [isPaused, setIsPaused] = useState(false)
  const [duration, setDuration] = useState(0)
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const meetingIdRef = useRef<string | null>(null)
  const titleRef = useRef<string>('')

  // Duration timer
  useEffect(() => {
    if (isRecording && !isPaused) {
      timerRef.current = setInterval(() => {
        setDuration((d) => d + 1)
      }, 1000)
    } else {
      if (timerRef.current) {
        clearInterval(timerRef.current)
        timerRef.current = null
      }
    }
    return () => {
      if (timerRef.current) clearInterval(timerRef.current)
    }
  }, [isRecording, isPaused])

  const startRecording = useCallback(async (title?: string) => {
    try {
      const hasPermission = await audioRecorder.requestMicrophonePermission()
      if (!hasPermission) {
        throw new Error('Microphone permission denied')
      }

      titleRef.current = title || 'Untitled Meeting'

      // Create meeting in local DB
      const meeting = await meetingRepository.createMeeting(titleRef.current)
      meetingIdRef.current = meeting.meeting_id
      trackMeetingCreated(meeting.meeting_id)

      // Start actual audio capture
      await audioRecorder.startRecording()

      setDuration(0)
      setIsPaused(false)
      setIsRecording(true)
    } catch (e) {
      console.error('[Recording] Failed to start:', e)
      throw e
    }
  }, [])

  const stopRecording = useCallback(async (): Promise<string | null> => {
    const meetingId = meetingIdRef.current
    if (!meetingId) return null

    try {
      // Stop audio capture and save file
      const audioUri = await audioRecorder.stopRecording(meetingId)

      // Update meeting with duration and queue audio upload
      await meetingRepository.updateMeeting(meetingId, {
        duration_seconds: duration,
        status: 'pending_upload',
      })
      await meetingRepository.queueAudioUpload(meetingId, audioUri)

      trackEvent('recording_completed', duration, { meeting_id: meetingId })
    } catch (e) {
      console.error('[Recording] Failed to stop:', e)
      // Still mark recording as stopped in UI
      if (meetingId) {
        await meetingRepository.updateMeeting(meetingId, { status: 'error' })
      }
    }

    setIsRecording(false)
    setIsPaused(false)
    meetingIdRef.current = null

    return meetingId
  }, [duration])

  const pauseRecording = useCallback(async () => {
    audioRecorder.pauseRecording()
    setIsPaused(true)
  }, [])

  const resumeRecording = useCallback(async () => {
    audioRecorder.resumeRecording()
    setIsPaused(false)
  }, [])

  return (
    <RecordingContext.Provider
      value={{ isRecording, isPaused, duration, startRecording, stopRecording, pauseRecording, resumeRecording }}
    >
      {children}
    </RecordingContext.Provider>
  )
}
