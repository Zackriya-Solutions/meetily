'use client'

import React, { createContext, useContext, useState, useCallback, useRef, useEffect } from 'react'

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

  // TODO: Phase 5 — integrate with Capacitor microphone plugin
  // and meetingRepository for local storage + sync queue

  const startRecording = useCallback(async (title?: string) => {
    console.log('[Recording] Start recording:', title)
    setDuration(0)
    setIsPaused(false)
    setIsRecording(true)
    // Phase 5: Will call Capacitor microphone plugin
  }, [])

  const stopRecording = useCallback(async (): Promise<string | null> => {
    console.log('[Recording] Stop recording, duration:', duration)
    setIsRecording(false)
    setIsPaused(false)
    // Phase 5: Will stop microphone, save audio file,
    // create meeting in SQLite, queue sync
    // Return meeting_id for navigation
    return null
  }, [duration])

  const pauseRecording = useCallback(async () => {
    console.log('[Recording] Pause')
    setIsPaused(true)
    // Phase 5: Will pause Capacitor recording
  }, [])

  const resumeRecording = useCallback(async () => {
    console.log('[Recording] Resume')
    setIsPaused(false)
    // Phase 5: Will resume Capacitor recording
  }, [])

  return (
    <RecordingContext.Provider
      value={{ isRecording, isPaused, duration, startRecording, stopRecording, pauseRecording, resumeRecording }}
    >
      {children}
    </RecordingContext.Provider>
  )
}
