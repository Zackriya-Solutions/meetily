'use client'

import React, { useState, useCallback } from 'react'
import { useRouter } from 'next/navigation'
import { useRecording } from '@/contexts/RecordingContext'
import { Mic, Square, Pause, Play } from 'lucide-react'

export default function RecordingScreen() {
  const router = useRouter()
  const { isRecording, isPaused, duration, startRecording, stopRecording, pauseRecording, resumeRecording } = useRecording()
  const [meetingTitle, setMeetingTitle] = useState('')

  const formatDuration = (seconds: number) => {
    const h = Math.floor(seconds / 3600)
    const m = Math.floor((seconds % 3600) / 60)
    const s = Math.floor(seconds % 60)
    return h > 0
      ? `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
      : `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }

  const handleStart = useCallback(async () => {
    await startRecording(meetingTitle || undefined)
  }, [startRecording, meetingTitle])

  const handleStop = useCallback(async () => {
    const meetingId = await stopRecording()
    if (meetingId) {
      router.push(`/meeting/${meetingId}`)
    }
  }, [stopRecording, router])

  return (
    <div className="flex flex-col items-center justify-center h-full px-6">
      {/* Title input */}
      {!isRecording && (
        <div className="w-full max-w-sm mb-8">
          <input
            type="text"
            value={meetingTitle}
            onChange={(e) => setMeetingTitle(e.target.value)}
            placeholder="Meeting title (optional)"
            className="w-full px-4 py-3 border border-gray-300 rounded-xl text-center text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>
      )}

      {/* Duration display */}
      <div className="text-5xl font-light text-gray-900 mb-8 tabular-nums">
        {formatDuration(duration)}
      </div>

      {/* Recording controls */}
      <div className="flex items-center gap-6">
        {isRecording ? (
          <>
            {/* Pause / Resume */}
            <button
              onClick={isPaused ? resumeRecording : pauseRecording}
              className="w-14 h-14 rounded-full bg-gray-100 flex items-center justify-center active:bg-gray-200"
            >
              {isPaused ? (
                <Play className="w-6 h-6 text-gray-700" />
              ) : (
                <Pause className="w-6 h-6 text-gray-700" />
              )}
            </button>

            {/* Stop */}
            <button
              onClick={handleStop}
              className="w-20 h-20 rounded-full bg-red-600 flex items-center justify-center active:bg-red-700 shadow-lg"
            >
              <Square className="w-8 h-8 text-white" fill="white" />
            </button>

            {/* Spacer for centering */}
            <div className="w-14" />
          </>
        ) : (
          /* Start recording */
          <button
            onClick={handleStart}
            className="w-20 h-20 rounded-full bg-red-600 flex items-center justify-center active:bg-red-700 shadow-lg"
          >
            <Mic className="w-8 h-8 text-white" />
          </button>
        )}
      </div>

      {/* Status text */}
      <p className="text-sm text-gray-500 mt-6">
        {isRecording
          ? isPaused
            ? 'Recording paused'
            : 'Recording in progress...'
          : 'Tap to start recording'}
      </p>
    </div>
  )
}
